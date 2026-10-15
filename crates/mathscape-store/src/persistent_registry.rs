//! `PersistentRegistry` — redb-backed `Registry` impl.
//!
//! Combines an on-disk artifacts table (redb) with an in-memory
//! cache of the full registry contents. On startup, all artifacts
//! are loaded into the cache. `insert` writes-through; `all()`
//! reads from the cache. The status overlay is maintained in memory
//! only (persisting it is Phase J+ work — demotions / migrations are
//! rare enough that reconstructing the overlay from MigrationReports
//! on startup is acceptable).

use mathscape_core::{
    epoch::{Artifact, Registry},
    hash::TermRef,
    lifecycle::ProofStatus,
};
use redb::{Database, ReadableTable, TableDefinition};
use std::collections::HashMap;
use std::path::Path;

const ARTIFACTS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("artifacts");

#[derive(Debug, thiserror::Error)]
pub enum PersistentRegistryError {
    #[error("redb error: {0}")]
    Db(#[from] redb::DatabaseError),
    #[error("redb transaction error: {0}")]
    Tx(#[from] redb::TransactionError),
    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("redb commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("bincode serialize error: {0}")]
    Serialize(#[from] bincode::Error),
}

/// redb-backed Registry that caches all artifacts in memory for
/// fast `all()` access.
pub struct PersistentRegistry {
    db: Database,
    cache: Vec<Artifact>,
    status_overlay: HashMap<TermRef, ProofStatus>,
}

impl PersistentRegistry {
    /// Open or create a persistent registry at `path`. Loads all
    /// artifacts into the in-memory cache on startup.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistentRegistryError> {
        let db = Database::create(path)?;
        // Ensure the table exists.
        let txn = db.begin_write()?;
        {
            let _table = txn.open_table(ARTIFACTS_TABLE)?;
        }
        txn.commit()?;
        let cache = Self::load_all(&db)?;
        Ok(Self {
            db,
            cache,
            status_overlay: HashMap::new(),
        })
    }

    /// Open an in-memory persistent registry for testing.
    pub fn open_in_memory() -> Result<Self, PersistentRegistryError> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())?;
        let txn = db.begin_write()?;
        {
            let _table = txn.open_table(ARTIFACTS_TABLE)?;
        }
        txn.commit()?;
        Ok(Self {
            db,
            cache: Vec::new(),
            status_overlay: HashMap::new(),
        })
    }

    fn load_all(db: &Database) -> Result<Vec<Artifact>, PersistentRegistryError> {
        let txn = db.begin_read().map_err(|e| match e {
            redb::TransactionError::Storage(s) => PersistentRegistryError::Storage(s),
            other => PersistentRegistryError::Tx(other),
        })?;
        let table = txn.open_table(ARTIFACTS_TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_k, v) = entry?;
            let artifact: Artifact = bincode::deserialize(v.value())?;
            out.push(artifact);
        }
        // Deterministic order by content hash for replay stability.
        out.sort_by_key(|a| *a.content_hash.as_bytes());
        Ok(out)
    }

    fn persist(&mut self, artifact: &Artifact) -> Result<(), PersistentRegistryError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(ARTIFACTS_TABLE)?;
            let bytes = bincode::serialize(artifact)?;
            table.insert(artifact.content_hash.as_bytes().as_slice(), bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }
}

impl Registry for PersistentRegistry {
    fn insert(&mut self, artifact: Artifact) {
        if let Err(e) = self.persist(&artifact) {
            tracing::warn!("PersistentRegistry::insert write-through failed: {e}");
        }
        self.cache.push(artifact);
    }

    fn all(&self) -> &[Artifact] {
        &self.cache
    }

    fn mark_status(&mut self, artifact_hash: TermRef, status: ProofStatus) {
        self.status_overlay.insert(artifact_hash, status);
    }

    fn status_of(&self, artifact_hash: TermRef) -> Option<ProofStatus> {
        if let Some(s) = self.status_overlay.get(&artifact_hash) {
            return Some(s.clone());
        }
        self.cache
            .iter()
            .find(|a| a.content_hash == artifact_hash)
            .map(|a| a.certificate.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::{
        epoch::AcceptanceCertificate,
        eval::RewriteRule,
        lifecycle::{DemotionReason, ProofStatus},
        term::Term,
    };

    fn mk_artifact(sym: u32) -> Artifact {
        let rule = RewriteRule {
            name: format!("r{sym}"),
            lhs: Term::Symbol(sym, vec![]),
            rhs: Term::Point(sym as u64),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    #[test]
    fn insert_and_retrieve_in_memory() {
        let mut reg = PersistentRegistry::open_in_memory().unwrap();
        let a = mk_artifact(1);
        let hash = a.content_hash;
        reg.insert(a);
        assert_eq!(reg.len(), 1);
        assert!(reg.find(hash).is_some());
    }

    #[test]
    fn mark_status_overlay_takes_precedence() {
        let mut reg = PersistentRegistry::open_in_memory().unwrap();
        let a = mk_artifact(1);
        let hash = a.content_hash;
        reg.insert(a);
        reg.mark_status(hash, ProofStatus::Demoted(DemotionReason::StaleConjecture));
        assert!(matches!(
            reg.status_of(hash),
            Some(ProofStatus::Demoted(_))
        ));
    }

    #[test]
    fn persist_and_reload_across_open_calls() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        // Drop the file handle so redb can exclusively open.
        drop(file);

        {
            let mut reg = PersistentRegistry::open(&path).unwrap();
            reg.insert(mk_artifact(1));
            reg.insert(mk_artifact(2));
            reg.insert(mk_artifact(3));
            assert_eq!(reg.len(), 3);
        }
        let reg2 = PersistentRegistry::open(&path).unwrap();
        assert_eq!(reg2.len(), 3, "artifacts should survive reopen");

        // Registry root should be stable across reopens.
        let root = reg2.root();
        assert_ne!(root, TermRef([0; 32]));
    }

    #[test]
    fn persistent_registry_root_is_deterministic_across_reopens() {
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        drop(file);

        let root_after_write = {
            let mut reg = PersistentRegistry::open(&path).unwrap();
            reg.insert(mk_artifact(10));
            reg.insert(mk_artifact(20));
            reg.root()
        };
        let root_after_reopen = PersistentRegistry::open(&path).unwrap().root();
        assert_eq!(root_after_write, root_after_reopen);
    }
}
