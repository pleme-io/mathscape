use crate::delta::{Discovery, EpochDelta, MechanismDelta, MechanismOutcome};
use crate::error::StreamError;
use crate::store::ObjectStore;
use mathscape_compress::{CorpusShape, Extractor, Frontier, FrontierSpec, Motor, entailed_by_library};
use mathscape_config::{StreamClock, StreamConfig, StreamGraveyard, StreamMechanism, StreamNovelty, StreamTrialScale};
use mathscape_core::certification::{CertificationVerdict, Certifier};
use mathscape_core::policy::LinearPolicy;
use mathscape_proof::mechanism::{
    MechanismConfig, MechanismPool, TrialResult, graveyard_context, respond_to_saturation,
    respond_to_saturation_in_context,
};
use mathscape_proof::semantic::{ValidationConfig, discover_semantic_projections_with_config};
use mathscape_core::bootstrap::{BootstrapCycleSpec, ExperimentScenario, library_merkle_root};
use mathscape_core::certification::{CertifyingConsumer, DefaultCertifier, rule_content_hash};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::mathscape_map::{BufferedConsumer, EventHub, MapEvent, MapEventConsumer};
use mathscape_core::meta_loop::{HeuristicProposer, MetaLoop, MetaLoopConfig};
use mathscape_core::snapshot::{snapshot_handle_at, trainer_from_snapshot};
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::LiveInferenceHandle;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

pub const DELTA_LOG: &str = "deltas.lisp";

const META_SEED: &str = "stream.seed";
const META_EPOCH: &str = "stream.epoch";
const META_PARENT: &str = "stream.parent";
const META_TOMBSTONES: &str = "stream.tombstones";
const META_CERTIFIED: &str = "stream.certified-deltas";
const META_MECHANISM: &str = "stream.mechanism";
const META_CURSOR: &str = "stream.frontier-cursor";
const META_DRY: &str = "stream.dry-epochs";
const META_SATURATIONS: &str = "stream.saturations";
const MECHANISM_SALT: u64 = 0x4D45_4348_414E_4953;
const CORPUS_TERMS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MechanismEvent {
    Mutated,
    Saturated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochReport {
    pub epoch: u64,
    pub candidates: usize,
    pub certified: usize,
    pub rejected: usize,
    pub library: usize,
    pub snapshot: Option<TermRef>,
    pub frontier: Option<(usize, usize)>,
    pub mechanism: Option<MechanismEvent>,
}

pub struct Writer {
    cfg: StreamConfig,
    store: ObjectStore,
    library: Rc<RefCell<Vec<RewriteRule>>>,
    trainer: Rc<StreamingPolicyTrainer>,
    buffer: Rc<BufferedConsumer>,
    gate: CertifyingConsumer<DefaultCertifier, EventHub>,
    known: BTreeSet<TermRef>,
    tombstones: BTreeSet<TermRef>,
    logged: BTreeSet<TermRef>,
    epoch: u64,
    certified_deltas: u64,
    since_snapshot: u64,
    epochs_since_snapshot: u64,
    dirty: bool,
    head: Option<TermRef>,
    resumed_from: Option<TermRef>,
    epochs_run: u64,
    pool: MechanismPool,
    frontier: Option<Frontier>,
    cursor: usize,
    dry: u64,
    saturations: u32,
}

#[must_use]
pub fn epoch_seed(seed: u64, epoch: u64) -> u64 {
    let mut z = seed ^ epoch.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn meta<'a>(snap: &'a mathscape_core::snapshot::ModelSnapshot, object: &TermRef, key: &'static str) -> Result<&'a str, StreamError> {
    snap.metadata
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| StreamError::BadMetadata { object: object.to_hex(), key })
}

fn meta_u64(snap: &mathscape_core::snapshot::ModelSnapshot, object: &TermRef, key: &'static str) -> Result<u64, StreamError> {
    meta(snap, object, key)?
        .parse()
        .map_err(|_| StreamError::BadMetadata { object: object.to_hex(), key })
}

fn meta_or<T: std::str::FromStr>(
    snap: &mathscape_core::snapshot::ModelSnapshot,
    object: &TermRef,
    key: &'static str,
    absent: T,
) -> Result<T, StreamError> {
    snap.metadata.get(key).map_or(Ok(absent), |v| {
        v.parse().map_err(|_| StreamError::BadMetadata { object: object.to_hex(), key })
    })
}

fn meta_pool(snap: &mathscape_core::snapshot::ModelSnapshot, object: &TermRef) -> Result<MechanismPool, StreamError> {
    let bad = || StreamError::BadMetadata { object: object.to_hex(), key: META_MECHANISM };
    match snap.metadata.get(META_MECHANISM) {
        None => Ok(MechanismPool::new()),
        Some(text) => {
            let forms = tatara_lisp::reader::read(text).map_err(|_| bad())?;
            match forms.as_slice() {
                [one] => MechanismPool::from_sexp(one).ok_or_else(bad),
                _ => Err(bad()),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotorMechanism {
    pub corpus: CorpusShape,
    pub min_support: usize,
    pub min_shared_size: usize,
    pub max_new_rules: usize,
    pub projection_size: usize,
    pub composition_cap: usize,
    pub validator_samples: usize,
    pub validator_max_value: u64,
    pub validator_step_limit: usize,
}

impl MotorMechanism {
    #[must_use]
    pub fn of(m: &MechanismConfig) -> Self {
        Self {
            corpus: CorpusShape {
                term_count: CORPUS_TERMS,
                base_depth: m.corpus_base_depth,
                vocab: m.corpus_vocab.clone(),
                max_value: m.corpus_max_value,
                shells: m.corpus_seed_from_theorems,
            },
            min_support: m.extract_min_matches,
            min_shared_size: m.extract_min_shared_size,
            max_new_rules: m.extract_max_new_rules,
            projection_size: m.candidate_max_size,
            composition_cap: m.composition_cap,
            validator_samples: m.validator_samples,
            validator_max_value: m.validator_max_value,
            validator_step_limit: m.validator_step_limit,
        }
    }
}

fn extractor_for(cfg: &StreamConfig, seed: u64) -> Extractor {
    match cfg.extractor {
        Extractor::Validated {
            step_limit,
            min_support,
            subterm_depth,
            k_samples,
            ..
        } => Extractor::Validated {
            step_limit,
            min_support,
            subterm_depth,
            k_samples,
            seed,
        },
        other => other,
    }
}

pub fn motor_candidates(
    cfg: &StreamConfig,
    epoch: u64,
    library: &[RewriteRule],
    policy: &LinearPolicy,
    known: &BTreeSet<TermRef>,
    drive: Option<&MotorMechanism>,
) -> Result<Vec<RewriteRule>, StreamError> {
    let seed = epoch_seed(cfg.seed, epoch);
    let adaptive = cfg.adaptive_every > 0 && epoch.is_multiple_of(cfg.adaptive_every);
    let extractor = extractor_for(cfg, seed);
    let motor = match drive {
        Some(mm) => Motor::seeded(seed)
            .with_extractor(extractor.with_min_support(mm.min_support))
            .with_corpus(mm.corpus.clone()),
        None => Motor::seeded(seed).with_extractor(extractor),
    }
    .with_corpus_max_nodes(cfg.corpus_max_nodes);
    let meta_loop = MetaLoop::new(
        motor,
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: cfg.phases_per_epoch,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let scenario = ExperimentScenario {
        name: format!("stream-epoch-{epoch}"),
        phases: vec![BootstrapCycleSpec {
            corpus_generator: if adaptive { "adaptive" } else { "default" }.into(),
            law_extractor: "derived-laws".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: cfg.iterations,
            seed_library: library.to_vec(),
            seed_policy: policy.clone(),
            early_stop_after_stable: Some(1),
        }],
    };
    let outcome = meta_loop.run(scenario)?;
    let Some(mm) = drive else {
        return Ok(outcome.final_library().to_vec());
    };
    let mut seen = BTreeSet::new();
    let fresh: Vec<RewriteRule> = outcome
        .final_library()
        .iter()
        .filter(|r| {
            let h = rule_content_hash(r);
            !known.contains(&h) && r.lhs.size() >= mm.min_shared_size && seen.insert(h)
        })
        .take(mm.max_new_rules)
        .cloned()
        .collect();
    let validation = ValidationConfig {
        samples: mm.validator_samples,
        max_value: mm.validator_max_value,
        step_limit: mm.validator_step_limit,
        rng_seed: seed,
    };
    let mut out = fresh.clone();
    for rule in &fresh {
        out.extend(
            discover_semantic_projections_with_config(rule, library, &validation, mm.projection_size, mm.composition_cap)
                .into_iter()
                .map(|(c, _)| c.rule)
                .filter(|r| seen.insert(rule_content_hash(r))),
        );
    }
    Ok(out)
}

#[must_use]
pub fn trial_window(policy: &StreamMechanism, base_len: usize, built_len: usize, extends: bool, cursor: usize) -> (usize, usize) {
    if !extends {
        return (0, policy.trial_budget);
    }
    let budget = match policy.trial_scale {
        StreamTrialScale::Fixed => policy.trial_budget,
        StreamTrialScale::Opened => built_len
            .saturating_sub(base_len)
            .clamp(policy.trial_budget, policy.trial_ceiling.max(policy.trial_budget)),
    };
    (cursor, budget)
}

#[must_use]
pub fn frontier_spec(cfg: &StreamConfig, mechanism: &MechanismConfig) -> FrontierSpec {
    FrontierSpec {
        max_size: mechanism.candidate_max_size,
        vocab: mechanism.candidate_vocab.clone(),
        constants: mechanism.candidate_constants.clone(),
        variables: cfg.frontier.variables,
        samples: mechanism.validator_samples,
        seed: cfg.seed,
    }
}

impl Writer {
    pub fn open(cfg: StreamConfig) -> Result<Self, StreamError> {
        let store = ObjectStore::open(&cfg.data_dir)?;
        let hub = EventHub::new();
        let buffer = Rc::new(BufferedConsumer::new());
        let mut w = match store.head()? {
            Some(head) => {
                let snap = store.get_snapshot(&head)?;
                let seed = meta_u64(&snap, &head, META_SEED)?;
                if seed != cfg.seed {
                    return Err(StreamError::SeedMismatch { head: seed, config: cfg.seed });
                }
                let tombstones = meta(&snap, &head, META_TOMBSTONES)?
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| TermRef::from_hex(s).ok_or(StreamError::BadMetadata { object: head.to_hex(), key: META_TOMBSTONES }))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                let trainer = Rc::new(trainer_from_snapshot(&snap.trainer));
                Self {
                    pool: meta_pool(&snap, &head)?,
                    cursor: meta_or(&snap, &head, META_CURSOR, 0)?,
                    dry: meta_or(&snap, &head, META_DRY, 0)?,
                    saturations: meta_or(&snap, &head, META_SATURATIONS, 0)?,
                    epoch: meta_u64(&snap, &head, META_EPOCH)?,
                    certified_deltas: meta_u64(&snap, &head, META_CERTIFIED)?,
                    known: snap.library.iter().map(rule_content_hash).collect(),
                    library: Rc::new(RefCell::new(snap.library)),
                    trainer,
                    tombstones,
                    head: Some(head),
                    resumed_from: Some(head),
                    ..Self::empty(cfg.clone(), store.clone(), buffer.clone())
                }
            }
            None => Self::empty(cfg.clone(), store.clone(), buffer.clone()),
        };
        hub.subscribe(w.trainer.clone());
        hub.subscribe(buffer);
        w.gate = CertifyingConsumer::new(DefaultCertifier::default(), hub);
        w.logged = store.read_lines(DELTA_LOG)?.iter().map(|l| TermRef::from_bytes(l.as_bytes())).collect();
        w.rebuild_frontier();
        Ok(w)
    }

    fn rebuild_frontier(&mut self) {
        self.frontier = self
            .cfg
            .frontier
            .enabled
            .then(|| Frontier::build(frontier_spec(&self.cfg, &self.pool.current)));
    }

    fn empty(cfg: StreamConfig, store: ObjectStore, buffer: Rc<BufferedConsumer>) -> Self {
        let trainer = Rc::new(StreamingPolicyTrainer::new(cfg.learning_rate));
        Self {
            cfg,
            store,
            library: Rc::new(RefCell::new(Vec::new())),
            trainer,
            buffer,
            gate: CertifyingConsumer::new(DefaultCertifier::default(), EventHub::new()),
            known: BTreeSet::new(),
            tombstones: BTreeSet::new(),
            logged: BTreeSet::new(),
            epoch: 0,
            certified_deltas: 0,
            since_snapshot: 0,
            epochs_since_snapshot: 0,
            dirty: false,
            head: None,
            resumed_from: None,
            epochs_run: 0,
            pool: MechanismPool::new(),
            frontier: None,
            cursor: 0,
            dry: 0,
            saturations: 0,
        }
    }

    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    #[must_use]
    pub fn head(&self) -> Option<TermRef> {
        self.head
    }

    #[must_use]
    pub fn resumed_from(&self) -> Option<TermRef> {
        self.resumed_from
    }

    #[must_use]
    pub fn epochs_run(&self) -> u64 {
        self.epochs_run
    }

    #[must_use]
    pub fn library(&self) -> Vec<RewriteRule> {
        self.library.borrow().clone()
    }

    #[must_use]
    pub fn library_root(&self) -> TermRef {
        library_merkle_root(&self.library.borrow())
    }

    #[must_use]
    pub fn tombstones(&self) -> usize {
        self.tombstones.len()
    }

    #[must_use]
    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    #[must_use]
    pub fn config(&self) -> &StreamConfig {
        &self.cfg
    }

    #[must_use]
    pub fn mechanism(&self) -> &MechanismPool {
        &self.pool
    }

    #[must_use]
    pub fn frontier(&self) -> Option<&Frontier> {
        self.frontier.as_ref()
    }

    #[must_use]
    pub fn frontier_cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn motor_mechanism(&self) -> Option<MotorMechanism> {
        self.cfg.mechanism.drive_motor.then(|| MotorMechanism::of(&self.pool.current))
    }

    pub fn discover(&self, epoch: u64) -> Result<Discovery, StreamError> {
        let seed = epoch_seed(self.cfg.seed, epoch);
        let raw = motor_candidates(
            &self.cfg,
            epoch,
            &self.library(),
            &self.trainer.snapshot(),
            &self.known,
            self.motor_mechanism().as_ref(),
        )?;
        let mut seen = BTreeSet::new();
        let mut candidates: Vec<RewriteRule> = raw
            .into_iter()
            .filter(|r| {
                let h = rule_content_hash(r);
                !self.known.contains(&h) && seen.insert(h)
            })
            .collect();
        let frontier = self.frontier.as_ref().map(|f| {
            let slice = f.slice(self.cursor, self.cfg.frontier.per_epoch);
            candidates.extend(slice.iter().map(mathscape_compress::Equation::rule).filter(|r| {
                let h = rule_content_hash(r);
                !self.known.contains(&h) && !self.tombstones.contains(&h) && seen.insert(h)
            }));
            (self.cursor, self.cursor + slice.len())
        });
        Ok(Discovery {
            epoch,
            seed,
            candidates,
            frontier,
        })
    }

    fn stamp(&self) -> u64 {
        match self.cfg.clock {
            StreamClock::Logical => self.epoch,
            StreamClock::Wall => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }

    pub fn apply(&mut self, discovery: &Discovery) -> Option<EpochDelta> {
        let parent_root = self.library_root();
        drop(self.buffer.drain());
        if let Some((from, to)) = discovery.frontier
            && from == self.cursor
        {
            self.cursor = to;
        }
        for rule in &discovery.candidates {
            let h = rule_content_hash(rule);
            if self.known.contains(&h) || self.tombstones.contains(&h) {
                continue;
            }
            if self.cfg.novelty == StreamNovelty::Entailment && entailed_by_library(rule, &self.library.borrow()) {
                continue;
            }
            let size = self.library.borrow().len();
            let mark = self.buffer.len();
            self.gate.on_event(&MapEvent::CoreGrew {
                prev_core_size: size,
                new_core_size: size + 1,
                added_rule: rule.clone(),
            });
            let verdict = self.buffer.events.borrow()[mark..].iter().find_map(|e| match e {
                MapEvent::RuleCertified { .. } => Some(true),
                MapEvent::RuleRejectedAtCertification { .. } => Some(false),
                _ => None,
            });
            match verdict {
                Some(true) => {
                    self.library.borrow_mut().push(rule.clone());
                    self.known.insert(h);
                }
                Some(false) => {
                    self.tombstones.insert(h);
                }
                None => {}
            }
        }
        let events = self.buffer.drain();
        let changed = events
            .iter()
            .any(|e| matches!(e, MapEvent::RuleCertified { .. } | MapEvent::RuleRejectedAtCertification { .. }));
        changed.then(|| EpochDelta {
            epoch: discovery.epoch,
            seed: discovery.seed,
            parent_root,
            new_root: self.library_root(),
            timestamp: self.stamp(),
            events,
        })
    }

    pub fn merge(&mut self, delta: &EpochDelta) -> bool {
        let fresh = delta.certified().any(|r| !self.known.contains(&rule_content_hash(r)))
            || delta.rejected().any(|r| !self.tombstones.contains(&rule_content_hash(r)));
        if !fresh {
            return false;
        }
        for event in &delta.events {
            self.gate.downstream.publish(event);
            match event {
                MapEvent::RuleCertified { rule, .. } => {
                    if self.known.insert(rule_content_hash(rule)) {
                        self.library.borrow_mut().push(rule.clone());
                    }
                }
                MapEvent::RuleRejectedAtCertification { rule, .. } => {
                    self.tombstones.insert(rule_content_hash(rule));
                }
                _ => {}
            }
        }
        drop(self.buffer.drain());
        true
    }

    fn log(&mut self, delta: &EpochDelta) -> Result<(), StreamError> {
        self.log_line(delta.to_line())
    }

    fn log_line(&mut self, line: String) -> Result<(), StreamError> {
        let h = TermRef::from_bytes(line.as_bytes());
        if self.logged.contains(&h) {
            return Ok(());
        }
        self.store.put_bytes(line.as_bytes())?;
        self.store.append_line(DELTA_LOG, &line)?;
        self.logged.insert(h);
        Ok(())
    }

    pub fn step(&mut self) -> Result<EpochReport, StreamError> {
        let discovery = self.discover(self.epoch)?;
        let delta = self.apply(&discovery);
        let (certified, rejected) = delta
            .as_ref()
            .map_or((0, 0), |d| (d.certified_count(), d.rejected().count()));
        if let Some(d) = &delta {
            self.log(d)?;
            self.dirty = true;
        }
        self.epoch += 1;
        self.epochs_run += 1;
        self.epochs_since_snapshot += 1;
        if certified > 0 {
            self.certified_deltas += 1;
            self.since_snapshot += 1;
            self.dry = 0;
        } else {
            self.dry += 1;
        }
        let mechanism = self.respond_if_saturated(discovery.epoch)?;
        let cadence = certified > 0 && self.since_snapshot >= self.cfg.snapshot_every.max(1);
        let checkpoint = self.cfg.checkpoint_epochs > 0 && self.epochs_since_snapshot >= self.cfg.checkpoint_epochs;
        let snapshot = if cadence || checkpoint { Some(self.snapshot()?) } else { None };
        Ok(EpochReport {
            epoch: discovery.epoch,
            candidates: discovery.candidates.len(),
            certified,
            rejected,
            library: self.library.borrow().len(),
            snapshot,
            frontier: self.frontier.as_ref().map(|f| (self.cursor, f.len())),
            mechanism,
        })
    }

    fn respond_if_saturated(&mut self, epoch: u64) -> Result<Option<MechanismEvent>, StreamError> {
        let Some(frontier) = self.frontier.as_ref() else {
            return Ok(None);
        };
        let policy = self.cfg.mechanism.clone();
        let threshold = policy.plateau_epochs.max(1).saturating_mul(1u64 << self.saturations.min(16));
        if !policy.respond || self.cursor < frontier.len() || self.dry < threshold {
            return Ok(None);
        }
        let base = frontier.spec().clone();
        let base_len = frontier.len();
        let cursor = self.cursor;
        let library = self.library();
        let tombstones = self.tombstones.clone();
        let known = self.known.clone();
        let learned = self.trainer.snapshot();
        let current_motor = MotorMechanism::of(&self.pool.current);
        let cfg = self.cfg.clone();
        let trial = |m: &MechanismConfig| {
            let certifier = DefaultCertifier::default();
            let mut lib = library.clone();
            let mut found = 0;
            let mut novel = 0;
            let mut weigh = |rule: RewriteRule, lib: &mut Vec<RewriteRule>| {
                let h = rule_content_hash(&rule);
                if known.contains(&h) || tombstones.contains(&h) || entailed_by_library(&rule, lib) {
                    return false;
                }
                found += 1;
                if matches!(certifier.try_certify(&rule, lib), CertificationVerdict::Certified { .. }) {
                    lib.push(rule);
                    novel += 1;
                }
                policy.trial_stop_after > 0 && novel >= policy.trial_stop_after
            };
            let spec = frontier_spec(&cfg, m);
            let mut done = false;
            if spec != base {
                let started = std::time::Instant::now();
                let extends = spec.extends(&base);
                let (size, vocab, samples) = (spec.max_size, spec.vocab.len(), spec.samples);
                let built = Frontier::build(spec);
                let (from, budget) = trial_window(&policy, base_len, built.len(), extends, cursor);
                done = built.slice(from, budget).iter().any(|eq| weigh(eq.rule(), &mut lib));
                tracing::info!(epoch, size, vocab, samples, terms = built.terms(), equations = built.len(), from, budget, ms = started.elapsed().as_millis() as u64, "trial frontier");
            }
            let motor = MotorMechanism::of(m);
            if !done && policy.drive_motor && motor != current_motor {
                let started = std::time::Instant::now();
                let candidates = motor_candidates(&cfg, epoch, &library, &learned, &known, Some(&motor)).unwrap_or_default();
                let n = candidates.len();
                let _ = candidates.into_iter().any(|rule| weigh(rule, &mut lib));
                tracing::info!(epoch, corpus_vocab = ?motor.corpus.vocab, candidates = n, ms = started.elapsed().as_millis() as u64, "trial motor");
            }
            TrialResult {
                delta_novelty: novel,
                total_theorems_found: found,
            }
        };
        let seed = epoch_seed(self.cfg.seed ^ MECHANISM_SALT, epoch);
        let context = (policy.graveyard == StreamGraveyard::Context).then(|| self.context_stamp());
        let rounds = policy.mutations_per_round.max(1);
        let response = match context {
            Some(stamp) => respond_to_saturation_in_context(
                &mut self.pool,
                graveyard_context(stamp),
                trial,
                rounds,
                policy.escalation_budget,
                seed,
            ),
            None => respond_to_saturation(&mut self.pool, trial, rounds, policy.escalation_budget, seed),
        };
        let outcome = match response {
            Some((mutation, config)) => {
                let next = frontier_spec(&self.cfg, &config);
                if !next.extends(&base) {
                    self.cursor = 0;
                }
                self.pool.current = config;
                self.rebuild_frontier();
                self.saturations = 0;
                MechanismOutcome::Mutated(mutation)
            }
            None => {
                self.saturations = self.saturations.saturating_add(1);
                MechanismOutcome::Saturated
            }
        };
        self.dry = 0;
        self.dirty = true;
        let event = match outcome {
            MechanismOutcome::Mutated(_) => MechanismEvent::Mutated,
            MechanismOutcome::Saturated => MechanismEvent::Saturated,
        };
        let delta = MechanismDelta {
            epoch,
            seed,
            outcome,
            config: self.pool.current.clone(),
            frontier_cursor: self.cursor,
            context,
        };
        self.log_line(delta.to_line())?;
        Ok(Some(event))
    }

    #[must_use]
    pub fn context_stamp(&self) -> u64 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.pool.current.to_sexp().to_string().as_bytes());
        hasher.update(self.library_root().as_bytes());
        let digest = hasher.finalize();
        let mut word = [0u8; 8];
        word.copy_from_slice(&digest.as_bytes()[..8]);
        u64::from_le_bytes(word)
    }

    pub fn snapshot(&mut self) -> Result<TermRef, StreamError> {
        let mut snap = self.model_snapshot();
        let hash = self.store.put_snapshot(&mut snap)?;
        self.store.set_head(hash)?;
        self.head = Some(hash);
        self.since_snapshot = 0;
        self.epochs_since_snapshot = 0;
        self.dirty = false;
        Ok(hash)
    }

    #[must_use]
    pub fn model_snapshot(&self) -> mathscape_core::snapshot::ModelSnapshot {
        let handle = LiveInferenceHandle::new(self.library.clone(), self.trainer.clone());
        let mut snap = snapshot_handle_at(&handle, self.stamp());
        let tombstones: Vec<String> = self.tombstones.iter().map(TermRef::to_hex).collect();
        snap.metadata.insert(META_SEED.into(), self.cfg.seed.to_string());
        snap.metadata.insert(META_EPOCH.into(), self.epoch.to_string());
        snap.metadata.insert(META_PARENT.into(), self.head.map(|h| h.to_hex()).unwrap_or_default());
        snap.metadata.insert(META_TOMBSTONES.into(), tombstones.join(","));
        snap.metadata.insert(META_CERTIFIED.into(), self.certified_deltas.to_string());
        snap.metadata.insert(META_MECHANISM.into(), self.pool.to_sexp().to_string());
        snap.metadata.insert(META_CURSOR.into(), self.cursor.to_string());
        snap.metadata.insert(META_DRY.into(), self.dry.to_string());
        snap.metadata.insert(META_SATURATIONS.into(), self.saturations.to_string());
        snap.content_hash = snap.compute_content_hash();
        snap
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.cfg.until_epoch.is_some_and(|n| self.epoch >= n)
    }

    pub fn run(&mut self, mut on_epoch: impl FnMut(&EpochReport)) -> Result<Option<TermRef>, StreamError> {
        while !self.finished() {
            let report = self.step()?;
            on_epoch(&report);
        }
        if self.dirty || self.epochs_since_snapshot > 0 || self.head.is_none() {
            self.snapshot()?;
        }
        Ok(self.head)
    }
}
