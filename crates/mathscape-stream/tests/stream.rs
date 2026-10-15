use mathscape_config::StreamConfig;
use mathscape_core::hash::TermRef;
use mathscape_stream::writer::DELTA_LOG;
use mathscape_stream::{EpochDelta, StreamError, Writer};
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
            ..StreamConfig::seeded(7, self.0.to_string_lossy())
        }
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
