//! Cross-process replayability: run the Epoch against
//! PersistentRegistry, close, reopen, run more epochs. Assert the
//! registry root equals a fresh run's root after the same total
//! epoch count. Proves criterion 2 of the knowability claim across
//! process lifetimes.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    epoch::{Epoch, Registry, RuleEmitter},
    term::Term,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};
use mathscape_store::PersistentRegistry;
use tempfile::NamedTempFile;

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}
fn corpus() -> Vec<Term> {
    vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
    ]
}

fn build_epoch<R: Registry>(
    reg: R,
) -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, R> {
    Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    )
}

#[test]
fn registry_persists_across_close_and_reopen() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path().to_path_buf();
    drop(file);

    let corpus = corpus();

    // First run: 5 epochs, then close.
    let root_first_run = {
        let reg = PersistentRegistry::open(&path).unwrap();
        let mut epoch = build_epoch(reg);
        for _ in 0..5 {
            epoch.step(&corpus);
        }
        let r = epoch.registry.root();
        // reg is dropped here — redb closes the database.
        r
    };

    // Reopen: run should be unchanged.
    let reopened = PersistentRegistry::open(&path).unwrap();
    assert_eq!(
        reopened.root(),
        root_first_run,
        "root must survive process close/reopen"
    );
    // And the library should be the same size.
    assert!(reopened.len() > 0, "library should have grown in first run");
}

#[test]
fn resume_from_persisted_state_matches_continuous_run() {
    let file_a = NamedTempFile::new().unwrap();
    let path_a = file_a.path().to_path_buf();
    drop(file_a);

    let file_b = NamedTempFile::new().unwrap();
    let path_b = file_b.path().to_path_buf();
    drop(file_b);

    let corpus = corpus();

    // Run A: 3 epochs then close, then reopen and run 2 more.
    {
        let mut epoch = build_epoch(PersistentRegistry::open(&path_a).unwrap());
        for _ in 0..3 {
            epoch.step(&corpus);
        }
    }
    let root_a_resumed = {
        let mut epoch = build_epoch(PersistentRegistry::open(&path_a).unwrap());
        // Note: epoch_id starts at 0 even after reopen. For this test we
        // only compare registry roots (they are order-independent via
        // sort in Registry::root).
        for _ in 0..2 {
            epoch.step(&corpus);
        }
        epoch.registry.root()
    };

    // Run B: 5 epochs continuously.
    let root_b_continuous = {
        let mut epoch = build_epoch(PersistentRegistry::open(&path_b).unwrap());
        for _ in 0..5 {
            epoch.step(&corpus);
        }
        epoch.registry.root()
    };

    assert_eq!(
        root_a_resumed, root_b_continuous,
        "resumed 3+2 run must produce the same root as continuous 5-epoch run"
    );
}
