//! SQLCipher-encrypted on-device store — the ONLY durable client store
//! (2026-07-14 requirement: Signal's model; relays never hold history).
//! Two tables: `kv` for the engine's state blobs (MLS snapshot, queue
//! credentials) and `seen` for delivered message ids, so the at-least-once
//! dedup set survives restart. `commit` writes both in one transaction —
//! callers persist BEFORE acking, so a crash between the two redelivers
//! into an already-updated seen set instead of replaying into MLS.

use std::path::Path;

use rusqlite::{params, Connection};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("wrong key or not a chat database")]
    BadKey,
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the encrypted database at `path`. `key` is the
    /// SQLCipher passphrase — platform callers derive it from the OS
    /// keystore; it never lives in this file's schema.
    pub fn open(path: &Path, key: &str) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "key", key)?;
        // First real read is where a wrong key surfaces ("file is not a
        // database") — map it to a loud, typed refusal.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|_| StoreError::BadKey)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS seen (id BLOB PRIMARY KEY);",
        )?;
        Ok(Store { conn })
    }

    pub fn get(&self, k: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT v FROM kv WHERE k = ?1")?;
        let mut rows = stmt.query(params![k])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn seen_ids(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id FROM seen")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Atomically upsert `kv` entries and insert `seen` ids (duplicates
    /// ignored — redelivery makes them normal).
    pub fn commit(&mut self, kv: &[(&str, &[u8])], seen: &[&[u8]]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        for (k, v) in kv {
            tx.execute(
                "INSERT INTO kv (k, v) VALUES (?1, ?2)
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                params![k, v],
            )?;
        }
        for id in seen {
            tx.execute("INSERT OR IGNORE INTO seen (id) VALUES (?1)", params![id])?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_key_refuses_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let mut store = Store::open(&path, "correct horse").unwrap();
            store.commit(&[("a", b"1")], &[]).unwrap();
        }
        assert!(matches!(
            Store::open(&path, "battery staple"),
            Err(StoreError::BadKey)
        ));
        let reopened = Store::open(&path, "correct horse").unwrap();
        assert_eq!(reopened.get("a").unwrap().unwrap(), b"1");
    }

    #[test]
    fn commit_is_atomic_and_seen_dedups() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("s.db"), "k").unwrap();
        store.commit(&[("x", b"old")], &[b"id1"]).unwrap();
        store.commit(&[("x", b"new")], &[b"id1", b"id2"]).unwrap();
        assert_eq!(store.get("x").unwrap().unwrap(), b"new");
        let mut ids = store.seen_ids().unwrap();
        ids.sort();
        assert_eq!(ids, vec![b"id1".to_vec(), b"id2".to_vec()]);
    }

    #[test]
    fn file_on_disk_is_not_plaintext_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let mut store = Store::open(&path, "k").unwrap();
        store
            .commit(&[("needle", b"the quick brown fox")], &[])
            .unwrap();
        drop(store);
        let bytes = std::fs::read(&path).unwrap();
        // Plain SQLite files start with this exact magic; SQLCipher files
        // start with the KDF salt instead.
        assert!(!bytes.starts_with(b"SQLite format 3"));
        assert!(!contains(&bytes, b"the quick brown fox"));
        assert!(!contains(&bytes, b"needle"));
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
