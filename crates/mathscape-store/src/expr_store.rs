//! Content-addressed expression store backed by redb.
//!
//! Terms are stored as `StoredTerm` keyed by their blake3 hash (TermRef).
//! This provides O(1) dedup and structural sharing: if two expressions
//! share a subtree, it's stored once.

use mathscape_core::hash::TermRef;
use mathscape_core::term::{StoredTerm, Term};
use redb::{Database, ReadableTableMetadata, TableDefinition};
use std::path::Path;

const TERMS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("terms");

/// The expression store: a redb database mapping TermRef -> StoredTerm.
pub struct ExprStore {
    db: Database,
}

impl ExprStore {
    /// Open or create an expression store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ExprStoreError> {
        let db = Database::create(path).map_err(ExprStoreError::Db)?;
        // Ensure the table exists
        let txn = db.begin_write().map_err(ExprStoreError::Tx)?;
        {
            let _table = txn.open_table(TERMS_TABLE).map_err(ExprStoreError::Table)?;
        }
        txn.commit().map_err(ExprStoreError::Commit)?;
        Ok(ExprStore { db })
    }

    /// Open an in-memory store (for testing).
    pub fn open_in_memory() -> Result<Self, ExprStoreError> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(ExprStoreError::Db)?;
        let txn = db.begin_write().map_err(ExprStoreError::Tx)?;
        {
            let _table = txn.open_table(TERMS_TABLE).map_err(ExprStoreError::Table)?;
        }
        txn.commit().map_err(ExprStoreError::Commit)?;
        Ok(ExprStore { db })
    }

    /// Store a Term, recursively storing all subtrees.
    /// Returns the root TermRef.
    pub fn put(&self, term: &Term) -> Result<TermRef, ExprStoreError> {
        let txn = self.db.begin_write().map_err(ExprStoreError::Tx)?;
        let href = {
            let mut table = txn.open_table(TERMS_TABLE).map_err(ExprStoreError::Table)?;
            self.put_inner(term, &mut table)?
        };
        txn.commit().map_err(ExprStoreError::Commit)?;
        Ok(href)
    }

    /// Store multiple terms in a single transaction.
    pub fn put_batch(&self, terms: &[Term]) -> Result<Vec<TermRef>, ExprStoreError> {
        let txn = self.db.begin_write().map_err(ExprStoreError::Tx)?;
        let refs = {
            let mut table = txn.open_table(TERMS_TABLE).map_err(ExprStoreError::Table)?;
            terms
                .iter()
                .map(|t| self.put_inner(t, &mut table))
                .collect::<Result<Vec<_>, _>>()?
        };
        txn.commit().map_err(ExprStoreError::Commit)?;
        Ok(refs)
    }

    fn put_inner(
        &self,
        term: &Term,
        table: &mut redb::Table<&[u8], &[u8]>,
    ) -> Result<TermRef, ExprStoreError> {
        let stored = self.to_stored(term, table)?;
        let bytes = bincode::serialize(&stored).map_err(|e| ExprStoreError::Serde(e.to_string()))?;
        let href = TermRef::from_bytes(&bytes);
        table
            .insert(href.as_bytes().as_slice(), bytes.as_slice())
            .map_err(ExprStoreError::Storage)?;
        Ok(href)
    }

    fn to_stored(
        &self,
        term: &Term,
        table: &mut redb::Table<&[u8], &[u8]>,
    ) -> Result<StoredTerm, ExprStoreError> {
        match term {
            Term::Point(id) => Ok(StoredTerm::Point(*id)),
            Term::Number(v) => Ok(StoredTerm::Number(v.clone())),
            Term::Var(v) => Ok(StoredTerm::Var(*v)),
            Term::Fn(params, body) => {
                let body_ref = self.put_inner(body, table)?;
                Ok(StoredTerm::Fn(params.clone(), body_ref))
            }
            Term::Apply(func, args) => {
                let func_ref = self.put_inner(func, table)?;
                let arg_refs = args
                    .iter()
                    .map(|a| self.put_inner(a, table))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(StoredTerm::Apply(func_ref, arg_refs))
            }
            Term::Symbol(id, args) => {
                let arg_refs = args
                    .iter()
                    .map(|a| self.put_inner(a, table))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(StoredTerm::Symbol(*id, arg_refs))
            }
        }
    }

    /// Retrieve a StoredTerm by its hash.
    pub fn get_stored(&self, href: &TermRef) -> Result<Option<StoredTerm>, ExprStoreError> {
        let txn = self.db.begin_read().map_err(ExprStoreError::ReadTx)?;
        let table = txn
            .open_table(TERMS_TABLE)
            .map_err(ExprStoreError::Table)?;
        match table
            .get(href.as_bytes().as_slice())
            .map_err(ExprStoreError::Storage)?
        {
            Some(bytes) => {
                let stored: StoredTerm = bincode::deserialize(bytes.value())
                    .map_err(|e| ExprStoreError::Serde(e.to_string()))?;
                Ok(Some(stored))
            }
            None => Ok(None),
        }
    }

    /// Reconstruct a full Term from a TermRef by recursively resolving children.
    pub fn get(&self, href: &TermRef) -> Result<Option<Term>, ExprStoreError> {
        let txn = self.db.begin_read().map_err(ExprStoreError::ReadTx)?;
        let table = txn
            .open_table(TERMS_TABLE)
            .map_err(ExprStoreError::Table)?;
        self.resolve(href, &table)
    }

    fn resolve(
        &self,
        href: &TermRef,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> Result<Option<Term>, ExprStoreError> {
        let bytes = match table
            .get(href.as_bytes().as_slice())
            .map_err(ExprStoreError::Storage)?
        {
            Some(b) => b,
            None => return Ok(None),
        };
        let stored: StoredTerm = bincode::deserialize(bytes.value())
            .map_err(|e| ExprStoreError::Serde(e.to_string()))?;

        let term = match stored {
            StoredTerm::Point(id) => Term::Point(id),
            StoredTerm::Number(v) => Term::Number(v),
            StoredTerm::Var(v) => Term::Var(v),
            StoredTerm::Fn(params, body_ref) => {
                let body = self
                    .resolve(&body_ref, table)?
                    .ok_or(ExprStoreError::MissingRef(body_ref))?;
                Term::Fn(params, Box::new(body))
            }
            StoredTerm::Apply(func_ref, arg_refs) => {
                let func = self
                    .resolve(&func_ref, table)?
                    .ok_or(ExprStoreError::MissingRef(func_ref))?;
                let args = arg_refs
                    .iter()
                    .map(|r| {
                        self.resolve(r, table)?
                            .ok_or(ExprStoreError::MissingRef(*r))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Term::Apply(Box::new(func), args)
            }
            StoredTerm::Symbol(id, arg_refs) => {
                let args = arg_refs
                    .iter()
                    .map(|r| {
                        self.resolve(r, table)?
                            .ok_or(ExprStoreError::MissingRef(*r))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Term::Symbol(id, args)
            }
        };
        Ok(Some(term))
    }

    /// Check if a hash exists in the store.
    pub fn contains(&self, href: &TermRef) -> Result<bool, ExprStoreError> {
        let txn = self.db.begin_read().map_err(ExprStoreError::ReadTx)?;
        let table = txn
            .open_table(TERMS_TABLE)
            .map_err(ExprStoreError::Table)?;
        let exists = table
            .get(href.as_bytes().as_slice())
            .map_err(ExprStoreError::Storage)?
            .is_some();
        Ok(exists)
    }

    /// Count total stored entries.
    pub fn len(&self) -> Result<u64, ExprStoreError> {
        let txn = self.db.begin_read().map_err(ExprStoreError::ReadTx)?;
        let table = txn
            .open_table(TERMS_TABLE)
            .map_err(ExprStoreError::Table)?;
        Ok(table.len().map_err(ExprStoreError::Storage)?)
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> Result<bool, ExprStoreError> {
        Ok(self.len()? == 0)
    }
}

#[derive(Debug)]
pub enum ExprStoreError {
    Db(redb::DatabaseError),
    Tx(redb::TransactionError),
    ReadTx(redb::TransactionError),
    Commit(redb::CommitError),
    Table(redb::TableError),
    Storage(redb::StorageError),
    Serde(String),
    MissingRef(TermRef),
}

impl std::fmt::Display for ExprStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExprStoreError::Db(e) => write!(f, "database error: {e}"),
            ExprStoreError::Tx(e) => write!(f, "transaction error: {e}"),
            ExprStoreError::ReadTx(e) => write!(f, "read transaction error: {e}"),
            ExprStoreError::Commit(e) => write!(f, "commit error: {e}"),
            ExprStoreError::Table(e) => write!(f, "table error: {e}"),
            ExprStoreError::Storage(e) => write!(f, "storage error: {e}"),
            ExprStoreError::Serde(e) => write!(f, "serialization error: {e}"),
            ExprStoreError::MissingRef(r) => write!(f, "missing ref: {r}"),
        }
    }
}

impl std::error::Error for ExprStoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn store_and_retrieve_leaf() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = nat(42);
        let href = store.put(&term).unwrap();
        let retrieved = store.get(&href).unwrap().unwrap();
        assert_eq!(term, retrieved);
    }

    #[test]
    fn store_and_retrieve_apply() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = apply(var(2), vec![nat(3), nat(4)]);
        let href = store.put(&term).unwrap();
        let retrieved = store.get(&href).unwrap().unwrap();
        assert_eq!(term, retrieved);
    }

    #[test]
    fn store_and_retrieve_nested() {
        let store = ExprStore::open_in_memory().unwrap();
        let inner = apply(var(3), vec![nat(2), nat(3)]);
        let term = apply(var(2), vec![inner, nat(4)]);
        let href = store.put(&term).unwrap();
        let retrieved = store.get(&href).unwrap().unwrap();
        assert_eq!(term, retrieved);
    }

    #[test]
    fn store_fn_term() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = Term::Fn(vec![10, 11], Box::new(apply(var(2), vec![var(10), var(11)])));
        let href = store.put(&term).unwrap();
        let retrieved = store.get(&href).unwrap().unwrap();
        assert_eq!(term, retrieved);
    }

    #[test]
    fn store_symbol_term() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = Term::Symbol(5, vec![nat(1), nat(2)]);
        let href = store.put(&term).unwrap();
        let retrieved = store.get(&href).unwrap().unwrap();
        assert_eq!(term, retrieved);
    }

    #[test]
    fn dedup_identical_terms() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = apply(var(2), vec![nat(1), nat(2)]);
        let href1 = store.put(&term).unwrap();
        let href2 = store.put(&term).unwrap();
        assert_eq!(href1, href2);
    }

    #[test]
    fn contains_check() {
        let store = ExprStore::open_in_memory().unwrap();
        let term = nat(99);
        let href = store.put(&term).unwrap();
        assert!(store.contains(&href).unwrap());

        let fake_ref = TermRef::from_bytes(b"nonexistent");
        assert!(!store.contains(&fake_ref).unwrap());
    }

    #[test]
    fn missing_ref_returns_none() {
        let store = ExprStore::open_in_memory().unwrap();
        let fake_ref = TermRef::from_bytes(b"nonexistent");
        assert!(store.get(&fake_ref).unwrap().is_none());
    }

    #[test]
    fn batch_put() {
        let store = ExprStore::open_in_memory().unwrap();
        let terms = vec![nat(1), nat(2), apply(var(2), vec![nat(3), nat(4)])];
        let refs = store.put_batch(&terms).unwrap();
        assert_eq!(refs.len(), 3);
        for (term, href) in terms.iter().zip(refs.iter()) {
            let retrieved = store.get(href).unwrap().unwrap();
            assert_eq!(term, &retrieved);
        }
    }

    #[test]
    fn len_and_is_empty() {
        let store = ExprStore::open_in_memory().unwrap();
        assert!(store.is_empty().unwrap());
        assert_eq!(store.len().unwrap(), 0);

        store.put(&nat(42)).unwrap();
        assert!(!store.is_empty().unwrap());
        assert!(store.len().unwrap() > 0);
    }

    #[test]
    fn structural_sharing() {
        let store = ExprStore::open_in_memory().unwrap();
        // Two terms sharing the subtree nat(0)
        let t1 = apply(var(2), vec![nat(5), nat(0)]);
        let t2 = apply(var(2), vec![nat(9), nat(0)]);
        store.put(&t1).unwrap();
        let count_after_first = store.len().unwrap();
        store.put(&t2).unwrap();
        let count_after_second = store.len().unwrap();
        // Second term shares nat(0) and var(2), so fewer new entries
        assert!(count_after_second < count_after_first * 2);
    }
}
