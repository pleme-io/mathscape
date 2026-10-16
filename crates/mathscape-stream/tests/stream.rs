use mathscape_config::{StreamConfig, StreamFrontier, StreamMechanism, StreamNovelty, StreamTrialScale};
use mathscape_core::hash::TermRef;
use mathscape_proof::mechanism::MechanismConfig;
use mathscape_stream::writer::{DELTA_LOG, MotorMechanism, motor_candidates, trial_window};
use std::collections::BTreeSet;
use mathscape_stream::{EpochDelta, MechanismDelta, MechanismEvent, MechanismOutcome, StreamError, Writer};
use std::path::PathBuf;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("mathscape-stream-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        Self(p)
    }

    fn cfg(&self, until: u64) -> StreamConfig {
        StreamConfig {
            until_epoch: Some(until),
            snapshot_every: 1,
            checkpoint_epochs: 3,
            ..StreamConfig::builtin(7, self.0.to_string_lossy())
        }
    }

    fn evolving(&self, until: u64) -> StreamConfig {
        StreamConfig {
            frontier: StreamFrontier {
                enabled: true,
                per_epoch: 128,
                ..StreamFrontier::default()
            },
            mechanism: StreamMechanism {
                plateau_epochs: 2,
                trial_budget: 96,
                ..StreamMechanism::default()
            },
            ..self.cfg(until)
        }
    }

    fn fixed(&self, until: u64) -> StreamConfig {
        StreamConfig {
            novelty: StreamNovelty::Off,
            frontier: StreamFrontier {
                enabled: false,
                ..StreamFrontier::default()
            },
            mechanism: StreamMechanism {
                respond: false,
                ..StreamMechanism::v0_1_8()
            },
            ..self.cfg(until)
        }
    }

    fn mechanism_deltas(&self) -> Vec<MechanismDelta> {
        self.deltas().lines().filter_map(MechanismDelta::from_line).collect()
    }

    fn deltas(&self) -> String {
        std::fs::read_to_string(self.0.join(DELTA_LOG)).unwrap_or_default()
    }

    fn leftovers(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if self.0.join("HEAD.tmp").exists() {
            out.push(self.0.join("HEAD.tmp"));
        }
        for shard in std::fs::read_dir(self.0.join("objects")).unwrap() {
            for e in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                let p = e.unwrap().path();
                if p.file_name().unwrap().to_string_lossy().starts_with(".tmp-") {
                    out.push(p);
                }
            }
        }
        out
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn chain(dir: &Dir, until: u64) -> Vec<TermRef> {
    let mut w = Writer::open(dir.cfg(until)).unwrap();
    let mut hashes = Vec::new();
    let head = w
        .run(|r| {
            if let Some(h) = r.snapshot {
                hashes.push(h);
            }
        })
        .unwrap();
    hashes.push(head.unwrap());
    hashes
}

#[test]
fn two_runs_with_the_same_seed_grow_the_same_hash_chain() {
    let (a, b) = (Dir::new("same-a"), Dir::new("same-b"));
    let first = chain(&a, 12);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = chain(&b, 12);
    assert!(first.len() >= 3, "too few snapshots to compare: {first:?}");
    assert_eq!(first, second);
    assert_eq!(a.deltas(), b.deltas());
}

#[test]
fn a_killed_writer_resumes_from_head_and_converges_on_the_uninterrupted_chain() {
    let (whole, killed) = (Dir::new("whole"), Dir::new("killed"));
    let uninterrupted = chain(&whole, 12);

    let mut w = Writer::open(killed.cfg(12)).unwrap();
    for _ in 0..7 {
        w.step().unwrap();
    }
    let head_at_kill = w.head().expect("a snapshot before the kill");
    drop(w);

    let mut resumed = Writer::open(killed.cfg(12)).unwrap();
    assert_eq!(resumed.resumed_from(), Some(head_at_kill));
    let resume_epoch = resumed.epoch();
    assert!(resume_epoch > 0, "resumed at epoch 0: the writer restarted");
    assert!(!resumed.library().is_empty());
    let head = resumed.run(|_| {}).unwrap();
    assert_eq!(resumed.epochs_run(), 12 - resume_epoch);
    assert_eq!(head, uninterrupted.last().copied());
    assert_eq!(killed.deltas(), whole.deltas());
}

#[test]
fn a_crash_mid_snapshot_never_moves_head_and_its_leftovers_are_swept() {
    let dir = Dir::new("crash");
    let mut w = Writer::open(dir.cfg(40)).unwrap();
    for _ in 0..2 {
        w.step().unwrap();
    }
    let before = w.snapshot().unwrap();
    w.step().unwrap();

    let mut snap = w.model_snapshot();
    let staged = w.store().stage_snapshot(&mut snap).unwrap();
    let next = staged.hash();
    assert_ne!(next, before);
    drop(staged);
    assert_eq!(w.store().head().unwrap(), Some(before));

    let mut snap = w.model_snapshot();
    let staged = w.store().stage_snapshot(&mut snap).unwrap();
    let committed = w.store().commit_object(staged).unwrap();
    let staged_head = w.store().stage_head(committed).unwrap();
    drop(staged_head);
    assert_eq!(w.store().head().unwrap(), Some(before));
    drop(w);
    assert!(!dir.leftovers().is_empty());

    let reopened = Writer::open(dir.cfg(40)).unwrap();
    assert_eq!(reopened.resumed_from(), Some(before));
    assert_eq!(dir.leftovers(), Vec::<PathBuf>::new());
}

#[test]
fn reapplying_a_discovery_or_its_delta_changes_nothing() {
    let (dir, other) = (Dir::new("idem"), Dir::new("idem-other"));
    let mut w = Writer::open(dir.cfg(40)).unwrap();
    let discovery = w.discover(0).unwrap();
    let delta = w.apply(&discovery).expect("epoch 0 certifies something");
    assert!(delta.certified_count() > 0);
    let after = w.model_snapshot().content_hash;

    assert_eq!(w.apply(&discovery), None);
    assert!(!w.merge(&delta));
    let parsed = EpochDelta::from_line(&delta.to_line()).expect("the delta reads back");
    assert!(!w.merge(&parsed));
    assert_eq!(w.model_snapshot().content_hash, after);

    let mut fresh = Writer::open(other.cfg(40)).unwrap();
    assert!(fresh.merge(&parsed));
    assert_eq!(fresh.library_root(), w.library_root());
    assert_eq!(fresh.library_root(), delta.new_root);
    let merged = fresh.model_snapshot().content_hash;
    assert!(!fresh.merge(&parsed));
    assert_eq!(fresh.model_snapshot().content_hash, merged);
}

#[test]
fn a_corrupt_or_misnamed_object_is_refused_with_a_typed_error() {
    let dir = Dir::new("corrupt");
    let mut w = Writer::open(dir.cfg(40)).unwrap();
    w.step().unwrap();
    let head = w.snapshot().unwrap();
    w.step().unwrap();
    let other = w.snapshot().unwrap();
    assert_ne!(head, other);
    let store = w.store().clone();
    drop(w);

    let misnamed = store.object_path(&head);
    std::fs::copy(store.object_path(&other), &misnamed).unwrap();
    store.set_head(head).unwrap();
    assert!(matches!(store.get_snapshot(&head), Err(StreamError::Corrupt { .. })));
    assert!(matches!(Writer::open(dir.cfg(40)), Err(StreamError::Corrupt { .. })));

    let path = store.object_path(&other);
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    store.set_head(other).unwrap();
    assert!(matches!(store.get_snapshot(&other), Err(StreamError::Corrupt { .. })));
    assert!(matches!(Writer::open(dir.cfg(40)), Err(StreamError::Corrupt { .. })));
}

#[test]
fn a_stream_refuses_to_resume_under_a_different_seed() {
    let dir = Dir::new("seed");
    let mut w = Writer::open(dir.cfg(40)).unwrap();
    w.step().unwrap();
    w.snapshot().unwrap();
    drop(w);
    let mut cfg = dir.cfg(40);
    cfg.seed = 8;
    assert!(matches!(Writer::open(cfg), Err(StreamError::SeedMismatch { head: 7, config: 8 })));
}

type EpochTrace = Vec<(u64, usize, Option<MechanismEvent>)>;

fn certified_by_epoch(cfg: StreamConfig) -> (EpochTrace, Option<TermRef>) {
    let mut w = Writer::open(cfg).unwrap();
    let mut out = Vec::new();
    let head = w.run(|r| out.push((r.epoch, r.certified, r.mechanism))).unwrap();
    (out, head)
}

#[test]
fn the_fixed_mechanism_stays_a_configured_value_and_still_saturates_after_epoch_zero() {
    let dir = Dir::new("fixed");
    let (epochs, _) = certified_by_epoch(dir.fixed(12));
    assert!(epochs[0].1 > 0);
    assert_eq!(epochs[1..].iter().map(|e| e.1).sum::<usize>(), 0);
    assert!(epochs.iter().all(|e| e.2.is_none()));
    assert!(dir.mechanism_deltas().is_empty());
}

#[test]
fn a_stream_keeps_certifying_after_epoch_zero_and_mutates_its_mechanism_on_a_plateau() {
    let dir = Dir::new("evolving");
    let (epochs, _) = certified_by_epoch(dir.evolving(14));
    let after_zero: usize = epochs[1..].iter().map(|e| e.1).sum();
    assert!(after_zero > 0, "no certification after epoch 0: {epochs:?}");
    let mutated = epochs
        .iter()
        .position(|e| e.2 == Some(MechanismEvent::Mutated))
        .expect("a plateau answered by a mechanism mutation");
    let after_mutation: usize = epochs[mutated + 1..].iter().map(|e| e.1).sum();
    assert!(after_mutation > 0, "the mutation broke no saturation: {epochs:?}");
    let logged = dir.mechanism_deltas();
    assert!(matches!(logged.first().map(|d| &d.outcome), Some(MechanismOutcome::Mutated(_))));
    assert_eq!(MechanismDelta::from_line(&logged[0].to_line()).as_ref(), Some(&logged[0]));
}

#[test]
fn mechanism_state_survives_a_kill_and_the_resumed_stream_converges() {
    let (whole, killed) = (Dir::new("mech-whole"), Dir::new("mech-killed"));
    let (epochs, uninterrupted) = certified_by_epoch(whole.evolving(14));
    let mutated = epochs
        .iter()
        .position(|e| e.2 == Some(MechanismEvent::Mutated))
        .expect("a mutation inside the run");
    assert!(mutated + 3 < 14, "mutation too late to kill after it: {mutated}");

    let mut w = Writer::open(killed.evolving(14)).unwrap();
    let mut saved = None;
    while saved.is_none() {
        let r = w.step().unwrap();
        if let (Some(h), true) = (r.snapshot, r.epoch > mutated as u64) {
            saved = Some((h, w.mechanism().current.clone(), w.frontier_cursor()));
        }
    }
    let (head_at_kill, at_kill, cursor_at_kill) = saved.unwrap();
    assert_ne!(at_kill, mathscape_proof::mechanism::MechanismConfig::default());
    w.step().unwrap();
    drop(w);

    let mut resumed = Writer::open(killed.evolving(14)).unwrap();
    assert_eq!(resumed.resumed_from(), Some(head_at_kill));
    assert_eq!(resumed.mechanism().current, at_kill);
    assert_eq!(resumed.frontier_cursor(), cursor_at_kill);
    let head = resumed.run(|_| {}).unwrap();
    assert_eq!(head, uninterrupted);
    assert_eq!(killed.deltas(), whole.deltas());
}

#[test]
fn two_runs_with_mutation_on_grow_the_same_hash_chain() {
    let (a, b) = (Dir::new("mut-a"), Dir::new("mut-b"));
    let grow = |d: &Dir| {
        let mut w = Writer::open(d.evolving(14)).unwrap();
        let mut hashes = Vec::new();
        let head = w
            .run(|r| {
                if let Some(h) = r.snapshot {
                    hashes.push(h);
                }
            })
            .unwrap();
        hashes.push(head.unwrap());
        hashes
    };
    let first = grow(&a);
    let second = grow(&b);
    assert!(!a.mechanism_deltas().is_empty(), "no mechanism mutation happened");
    assert_eq!(first, second);
    assert_eq!(a.deltas(), b.deltas());
}

#[test]
fn a_size_bump_trials_every_equation_it_opens_up_to_the_ceiling() {
    let opened = StreamMechanism::default();
    assert_eq!(opened.trial_scale, StreamTrialScale::Opened);
    assert_eq!(trial_window(&opened, 8083, 26826, true, 8083), (8083, 18743));
    assert_eq!(trial_window(&opened, 8083, 8183, true, 8083), (8083, opened.trial_budget));
    assert_eq!(trial_window(&opened, 26826, 140_577, true, 26826), (26826, opened.trial_ceiling));
    assert_eq!(trial_window(&opened, 8083, 26826, false, 8083), (0, opened.trial_budget));
    let fixed = StreamMechanism::v0_1_8();
    assert_eq!(trial_window(&fixed, 8083, 26826, true, 8083), (8083, fixed.trial_budget));
}

#[test]
fn the_mechanism_drives_the_motor_corpus_extract_and_composition() {
    let dir = Dir::new("motor");
    let mut w = Writer::open(dir.evolving(40)).unwrap();
    w.step().unwrap();
    let library = w.library();
    assert!(!library.is_empty());
    let cfg = dir.evolving(40);
    let policy = mathscape_core::policy::LinearPolicy::tensor_seeking_prior();
    let none = BTreeSet::new();
    let run = |m: &MechanismConfig| motor_candidates(&cfg, 1, &library, &policy, &none, Some(&MotorMechanism::of(m))).unwrap();
    let base = MechanismConfig::default();
    let driven = run(&base);
    assert!(!driven.is_empty());
    assert!(run(&MechanismConfig { extract_max_new_rules: 0, ..base.clone() }).is_empty());
    assert!(run(&MechanismConfig { extract_min_shared_size: 99, ..base.clone() }).is_empty());
    let projections: BTreeSet<usize> = [0, 5, 30, 120]
        .iter()
        .map(|cap| run(&MechanismConfig { composition_cap: *cap, ..base.clone() }).len())
        .collect();
    assert!(projections.len() > 1, "composition cap moved nothing: {projections:?}");
    let corpora: BTreeSet<Vec<String>> = [
        base.clone(),
        MechanismConfig { corpus_base_depth: 7, ..base.clone() },
        MechanismConfig { corpus_vocab: vec![2], ..base.clone() },
        MechanismConfig { corpus_seed_from_theorems: true, ..base.clone() },
    ]
    .iter()
    .map(|m| motor_candidates(&cfg, 1, &library, &policy, &library_hashes(&library), Some(&MotorMechanism::of(m))).unwrap())
    .map(|rules| rules.iter().map(|r| format!("{} => {}", r.lhs, r.rhs)).collect())
    .collect();
    assert!(corpora.len() > 1, "every corpus shape produced the same candidates: {corpora:?}");
    let undriven = motor_candidates(&cfg, 1, &library, &policy, &none, None).unwrap();
    assert!(undriven.len() >= library.len());
}

fn library_hashes(library: &[mathscape_core::eval::RewriteRule]) -> BTreeSet<TermRef> {
    library.iter().map(mathscape_core::certification::rule_content_hash).collect()
}

#[test]
fn a_large_library_keeps_the_motor_corpus_inside_its_node_budget() {
    let dir = Dir::new("budget");
    let mut w = Writer::open(dir.evolving(40)).unwrap();
    w.step().unwrap();
    let seed_rules = w.library();
    let library: Vec<_> = seed_rules.iter().cycle().take(150).cloned().collect();
    let cfg = dir.evolving(40);
    let policy = mathscape_core::policy::LinearPolicy::tensor_seeking_prior();
    let wide = MechanismConfig { corpus_vocab: vec![2, 3, 5, 7, 6], ..MechanismConfig::default() };
    let started = std::time::Instant::now();
    let bounded = motor_candidates(&cfg, 0, &library, &policy, &library_hashes(&seed_rules), Some(&MotorMechanism::of(&wide))).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(120), "motor took {:?}", started.elapsed());
    let again = motor_candidates(&cfg, 0, &library, &policy, &library_hashes(&seed_rules), Some(&MotorMechanism::of(&wide))).unwrap();
    assert_eq!(bounded, again);
}
