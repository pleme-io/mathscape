use mathscape_config::{StreamConfig, StreamSubstrate};
use mathscape_core::certification::{CertificationVerdict, Certifier};
use mathscape_core::hash::TermRef;
use mathscape_core::term::Term;
use mathscape_emergence::EmergenceConfig;
use mathscape_emergence::certify::{KernelCertifier, PATTERN_BASE};
use mathscape_stream::writer::DELTA_LOG;
use mathscape_stream::{StreamError, Writer};
use std::path::PathBuf;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("mathscape-emergent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        Self(p)
    }

    fn cfg(&self, until: u64) -> StreamConfig {
        StreamConfig {
            until_epoch: Some(until),
            snapshot_every: 1,
            checkpoint_epochs: 2,
            emergence: EmergenceConfig {
                terms_per_epoch: 2_000,
                law_universe_max: 3_000,
                ..EmergenceConfig::default()
            },
            ..StreamConfig::seeded(7, self.0.to_string_lossy())
        }
    }

    fn deltas(&self) -> String {
        std::fs::read_to_string(self.0.join(DELTA_LOG)).unwrap_or_default()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn primitive(t: &Term) -> bool {
    match t {
        Term::Number(_) | Term::Point(_) => false,
        Term::Var(v) => *v >= PATTERN_BASE,
        Term::Fn(_, body) => primitive(body),
        Term::Apply(f, args) => primitive(f) && args.iter().all(primitive),
        Term::Symbol(_, args) => args.iter().all(primitive),
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
fn the_default_stream_names_operators_and_laws_from_lambda_terms_alone() {
    let dir = Dir::new("default");
    let cfg = dir.cfg(4);
    assert_eq!(cfg.substrate, StreamSubstrate::Emergent);
    let mut w = Writer::open(cfg).unwrap();
    w.run(|r| assert!(r.emergence.is_some() && r.frontier.is_none())).unwrap();
    assert!(w.frontier().is_none());
    let library = w.library();
    let defs = library.iter().filter(|r| r.name.starts_with("def ")).count();
    let laws = library.iter().filter(|r| r.name.starts_with("law ")).count();
    assert!(defs >= 3 && laws >= 1, "{library:?}");
    let kernel = KernelCertifier::default();
    for (i, rule) in library.iter().enumerate() {
        assert!(primitive(&rule.lhs) && primitive(&rule.rhs), "{} reaches outside the primitives", rule.name);
        assert!(matches!(kernel.try_certify(rule, &library[..i]), CertificationVerdict::Certified { .. }), "{}", rule.name);
    }
}

#[test]
fn two_emergent_runs_with_the_same_seed_grow_the_same_hash_chain() {
    let (a, b) = (Dir::new("same-a"), Dir::new("same-b"));
    let first = chain(&a, 4);
    let second = chain(&b, 4);
    assert!(first.len() >= 3, "{first:?}");
    assert_eq!(first, second);
    assert_eq!(a.deltas(), b.deltas());
}

#[test]
fn a_killed_emergent_writer_resumes_its_cursor_and_converges() {
    let (whole, killed) = (Dir::new("whole"), Dir::new("killed"));
    let uninterrupted = chain(&whole, 5);
    let mut w = Writer::open(killed.cfg(5)).unwrap();
    for _ in 0..3 {
        w.step().unwrap();
    }
    let cursor = w.engine().unwrap().cursor();
    let head_at_kill = w.head().expect("a snapshot before the kill");
    drop(w);
    let mut resumed = Writer::open(killed.cfg(5)).unwrap();
    assert_eq!(resumed.resumed_from(), Some(head_at_kill));
    assert_eq!(resumed.epoch(), 3);
    assert_eq!(resumed.engine().unwrap().cursor(), cursor);
    let head = resumed.run(|_| {}).unwrap();
    assert_eq!(head, uninterrupted.last().copied());
    assert_eq!(killed.deltas(), whole.deltas());
}

#[test]
fn one_model_never_changes_substrate() {
    let dir = Dir::new("substrate");
    let mut w = Writer::open(StreamConfig::builtin(7, dir.0.to_string_lossy())).unwrap();
    w.snapshot().unwrap();
    drop(w);
    assert!(matches!(
        Writer::open(dir.cfg(1)),
        Err(StreamError::SubstrateMismatch { config: "emergent", .. })
    ));
    let other = Dir::new("substrate-b");
    let mut w = Writer::open(other.cfg(1)).unwrap();
    w.run(|_| {}).unwrap();
    drop(w);
    assert!(matches!(
        Writer::open(StreamConfig::builtin(7, other.0.to_string_lossy())),
        Err(StreamError::SubstrateMismatch { config: "builtin", .. })
    ));
}
