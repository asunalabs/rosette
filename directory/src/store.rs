//! Persistence (T10 + backing for T2/T6/T15/T19/T24). Real client-server
//! Postgres, not embedded SQLite — directory is a single centrally-run
//! service (unlike relay, which is meant to be self-hosted per-operator,
//! where an embedded single-file DB is the point). Matches how Signal's
//! account/directory-style data lives in a real DB, not a per-node file.
//!
//! Session tokens are the one deliberately non-persistent piece: kept in
//! an in-memory map, not a table. A directory restart means callers
//! re-verify; that's an acceptable ceiling at this stage.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use sqlx::{PgPool, Row};

use crate::search::{PhoneEntry, PrefixIndex};
use crate::username::{self, UsernameError};

pub const PHONE_COOLDOWN_HOURS: i64 = 24;

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error(transparent)]
    Username(#[from] UsernameError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

pub struct DirectoryStore {
    pool: PgPool,
    // Ephemeral, in-process only — see module docs.
    sessions: Mutex<HashMap<String, u64>>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs() as i64
}

impl DirectoryStore {
    /// Production entry point: connects and runs migrations.
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self::from_pool(pool))
    }

    /// Tests use `#[sqlx::test]` to get a fresh migrated DB per test and
    /// hand the resulting pool in here.
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn create_pending_user(&self, phone_hash: &str) -> sqlx::Result<u64> {
        let prefix = crate::search::hash_prefix(phone_hash);
        let row = sqlx::query(
            "INSERT INTO users (phone_hash, phone_hash_prefix, verified, created_at)
             VALUES ($1, $2, false, $3) RETURNING user_id",
        )
        .bind(phone_hash)
        .bind(prefix)
        .bind(now_unix())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("user_id") as u64)
    }

    pub async fn mark_verified(&self, user_id: u64, verified: bool) -> sqlx::Result<()> {
        sqlx::query("UPDATE users SET verified = $1 WHERE user_id = $2")
            .bind(verified)
            .bind(user_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Looks up the most recent user row for a phone hash — used by
    /// `/verify` to recover the pending signup without ever storing the
    /// plaintext phone number: the client resends it fresh at verify time,
    /// same as any real OTP flow.
    pub async fn find_user_by_phone_hash(&self, phone_hash: &str) -> sqlx::Result<Option<u64>> {
        let row = sqlx::query(
            "SELECT user_id FROM users WHERE phone_hash = $1 AND deleted_at IS NULL
             ORDER BY user_id DESC LIMIT 1",
        )
        .bind(phone_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<i64, _>("user_id") as u64))
    }

    pub async fn is_verified(&self, user_id: u64) -> sqlx::Result<Option<bool>> {
        let row = sqlx::query("SELECT verified FROM users WHERE user_id = $1")
            .bind(user_id as i64)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<bool, _>("verified")))
    }

    pub async fn is_phone_in_cooldown(&self, phone_hash: &str) -> sqlx::Result<bool> {
        let cutoff = now_unix() - PHONE_COOLDOWN_HOURS * 3600;
        let row = sqlx::query(
            "SELECT COUNT(*) AS n FROM phone_cooldown WHERE phone_hash = $1 AND deleted_at > $2",
        )
        .bind(phone_hash)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("n") > 0)
    }

    /// Assigns the first free discriminator for `nickname`, widening its
    /// width if the current one is exhausted, and claims it for `user_id`.
    /// Whole read-decide-write sequence runs against one connection so
    /// concurrent claims for the same nickname resolve to *different*
    /// discriminators; the schema's UNIQUE(nickname, discriminator) is the
    /// defense-in-depth backstop if that ever races (see the direct
    /// constraint test in `tests`).
    pub async fn claim_username(
        &self,
        user_id: u64,
        nickname: &str,
    ) -> Result<(u32, u32), ClaimError> {
        username::validate_nickname(nickname)?;
        let mut tx = self.pool.begin().await?;

        let mut width: u32 = sqlx::query("SELECT width FROM nickname_widths WHERE nickname = $1")
            .bind(nickname)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| r.get::<i32, _>("width") as u32)
            .unwrap_or(username::MIN_DISCRIMINATOR_WIDTH);

        let rows = sqlx::query(
            "SELECT discriminator FROM users WHERE nickname = $1 AND discriminator IS NOT NULL",
        )
        .bind(nickname)
        .fetch_all(&mut *tx)
        .await?;
        let taken: std::collections::HashSet<u32> = rows
            .iter()
            .map(|r| r.get::<i32, _>("discriminator") as u32)
            .collect();

        let slot = loop {
            match username::first_free_slot(&taken, width) {
                Ok(slot) => break slot,
                Err(UsernameError::DiscriminatorSpaceExhausted) if width < 9 => width += 1,
                Err(e) => return Err(e.into()),
            }
        };

        sqlx::query(
            "INSERT INTO nickname_widths (nickname, width) VALUES ($1, $2)
             ON CONFLICT (nickname) DO UPDATE SET width = excluded.width",
        )
        .bind(nickname)
        .bind(width as i32)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE users SET nickname = $1, discriminator = $2 WHERE user_id = $3")
            .bind(nickname)
            .bind(slot as i32)
            .bind(user_id as i64)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok((slot, width))
    }

    /// `phone_search_hash` is client-computed (unkeyed SHA-256, no server
    /// secret involved — distinct from the Argon2id `phone_hash`/pepper used
    /// for auth, OQ4, untouched). Required when turning search on; cleared
    /// whenever it's turned off, so a stolen DB only ever exposes the
    /// reversible hash of currently-opted-in accounts, never a lapsed one.
    /// Public username lookup (OQ10's discoverability wedge): claiming a
    /// handle is itself the opt-in, unlike phone search — no `searchable`
    /// gate here.
    pub async fn find_user_by_handle(
        &self,
        nickname: &str,
        discriminator: u32,
    ) -> sqlx::Result<Option<u64>> {
        let row = sqlx::query(
            "SELECT user_id FROM users
             WHERE nickname = $1 AND discriminator = $2 AND deleted_at IS NULL",
        )
        .bind(nickname)
        .bind(discriminator as i32)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<i64, _>("user_id") as u64))
    }

    pub async fn set_searchable(
        &self,
        user_id: u64,
        searchable: bool,
        phone_search_hash: Option<&str>,
    ) -> sqlx::Result<()> {
        let (hash, prefix) = match (searchable, phone_search_hash) {
            (true, Some(h)) => (h, crate::search::hash_prefix(h)),
            _ => ("", ""),
        };
        sqlx::query(
            "UPDATE users SET searchable = $1, phone_search_hash = $2, phone_search_hash_prefix = $3
             WHERE user_id = $4",
        )
        .bind(searchable)
        .bind(hash)
        .bind(prefix)
        .bind(user_id as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Returns the display handle for a user, at the nickname's *current*
    /// width (T6's correctness note) — never the width at claim time.
    pub async fn handle_for(&self, user_id: u64) -> sqlx::Result<Option<String>> {
        let row = sqlx::query(
            "SELECT nickname, discriminator FROM users WHERE user_id = $1 AND nickname IS NOT NULL",
        )
        .bind(user_id as i64)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let nickname: String = row.get("nickname");
        let discriminator: i32 = row.get("discriminator");

        let width: u32 = sqlx::query("SELECT width FROM nickname_widths WHERE nickname = $1")
            .bind(&nickname)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<i32, _>("width") as u32)
            .unwrap_or(username::MIN_DISCRIMINATOR_WIDTH);

        Ok(Some(username::format_handle(
            &nickname,
            discriminator as u32,
            width,
        )))
    }

    /// T15/T19: actually scrubs phone_hash/nickname content, not just a
    /// flag flip — a fresh read of this row after deletion must not
    /// recover either. Discriminator slot is permanently reserved (not
    /// released) — simplest safe choice; revisit if handle scarcity becomes
    /// a real problem (T15's original open question, now decided).
    pub async fn erase_user(&self, user_id: u64) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        let phone_hash: Option<String> =
            sqlx::query("SELECT phone_hash FROM users WHERE user_id = $1")
                .bind(user_id as i64)
                .fetch_optional(&mut *tx)
                .await?
                .map(|r| r.get("phone_hash"));

        if let Some(phone_hash) = phone_hash {
            sqlx::query("INSERT INTO phone_cooldown (phone_hash, deleted_at) VALUES ($1, $2)")
                .bind(phone_hash)
                .bind(now_unix())
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "UPDATE users SET phone_hash = '', phone_hash_prefix = '', searchable = false,
                phone_search_hash = '', phone_search_hash_prefix = '', deleted_at = $1
             WHERE user_id = $2",
        )
        .bind(now_unix())
        .bind(user_id as i64)
        .execute(&mut *tx)
        .await?;
        // T25: an erased user's pairing bootstrap (a live KeyPackage) must
        // not survive the account it was issued for.
        sqlx::query("DELETE FROM pairing_bootstrap WHERE user_id = $1")
            .bind(user_id as i64)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// T25: publish this user's one-time pairing bootstrap (a base64
    /// `ContactLink` — see migration 0002's comment). Upsert: a re-upload
    /// replaces whatever was there, so a client can replenish after its
    /// last one was consumed, or before it's ever been requested.
    pub async fn set_pairing_bootstrap(
        &self,
        user_id: u64,
        contact_link_b64: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO pairing_bootstrap (user_id, contact_link_b64, uploaded_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id) DO UPDATE SET
                contact_link_b64 = excluded.contact_link_b64,
                uploaded_at = excluded.uploaded_at",
        )
        .bind(user_id as i64)
        .bind(contact_link_b64)
        .bind(now_unix())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// T25: one-time fetch — the row is deleted in the same statement it's
    /// read from (`DELETE ... RETURNING`), so two concurrent requesters can
    /// never both receive the same KeyPackage (MLS one-time-use, not just a
    /// convention the caller has to honor). Only serves a target that's
    /// still `searchable` and not deleted — the only legitimate way to have
    /// learned this `user_id` is a directory search result, which already
    /// requires both.
    pub async fn consume_pairing_bootstrap(&self, user_id: u64) -> sqlx::Result<Option<String>> {
        let row = sqlx::query(
            "DELETE FROM pairing_bootstrap
             WHERE user_id = $1
               AND EXISTS (
                   SELECT 1 FROM users
                   WHERE users.user_id = pairing_bootstrap.user_id
                     AND users.searchable = true
                     AND users.deleted_at IS NULL
               )
             RETURNING contact_link_b64",
        )
        .bind(user_id as i64)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<String, _>("contact_link_b64")))
    }

    pub fn create_session(&self, user_id: u64) -> String {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = bytes.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        });
        self.sessions.lock().unwrap().insert(token.clone(), user_id);
        token
    }

    pub fn session_user_id(&self, token: &str) -> Option<u64> {
        self.sessions.lock().unwrap().get(token).copied()
    }
}

#[async_trait::async_trait]
impl PrefixIndex for DirectoryStore {
    async fn bucket(&self, prefix: &str) -> Vec<PhoneEntry> {
        // Buckets on phone_search_hash_prefix (client-computed, unkeyed) —
        // NOT phone_hash_prefix (the keyed Argon2id auth hash, OQ4), which
        // no client can reproduce without the server's secret pepper.
        let rows = sqlx::query(
            "SELECT phone_search_hash, user_id FROM users
             WHERE phone_search_hash_prefix = $1 AND searchable = true AND deleted_at IS NULL",
        )
        .bind(prefix)
        .fetch_all(&self.pool)
        .await;
        match rows {
            Ok(rows) => rows
                .into_iter()
                .map(|r| PhoneEntry {
                    phone_hash: r.get("phone_search_hash"),
                    user_id: r.get::<i64, _>("user_id") as u64,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn unique_constraint_holds_even_on_a_direct_duplicate_insert(pool: PgPool) {
        // Defense-in-depth check of the raw schema constraint, independent
        // of claim_username's app-level slot-picking logic.
        let store = DirectoryStore::from_pool(pool);
        let u1 = store.create_pending_user("hash-a").await.unwrap();
        let u2 = store.create_pending_user("hash-b").await.unwrap();
        store.claim_username(u1, "alice").await.unwrap();

        let result = sqlx::query(
            "UPDATE users SET nickname = 'alice', discriminator = 1 WHERE user_id = $1",
        )
        .bind(u2 as i64)
        .execute(&store.pool)
        .await;
        assert!(
            result.is_err(),
            "duplicate (nickname, discriminator) must be rejected at the DB level"
        );
    }

    #[sqlx::test]
    async fn claim_username_widens_after_99_holders(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        let mut first_user_id = 0;
        for i in 0..99 {
            let u = store
                .create_pending_user(&format!("hash-{i}"))
                .await
                .unwrap();
            if i == 0 {
                first_user_id = u;
            }
            let (slot, width) = store.claim_username(u, "popular").await.unwrap();
            assert_eq!(width, 2, "slot {i} should still fit at width 2");
            assert!(slot <= 99);
        }
        let u100 = store.create_pending_user("hash-100").await.unwrap();
        let (slot, width) = store.claim_username(u100, "popular").await.unwrap();
        assert_eq!(width, 3, "100th holder must widen to width 3");
        assert_eq!(slot, 100);

        // The very first holder (slot 1) must now render at the new width.
        let handle = store.handle_for(first_user_id).await.unwrap().unwrap();
        assert_eq!(handle, "popular#001");
    }

    #[sqlx::test]
    async fn erase_scrubs_phone_hash_and_starts_cooldown(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        let u = store
            .create_pending_user("secret-phone-hash")
            .await
            .unwrap();
        store.claim_username(u, "bob").await.unwrap();
        store
            .set_searchable(u, true, Some(&"a".repeat(64)))
            .await
            .unwrap();

        store.erase_user(u).await.unwrap();

        let row = sqlx::query("SELECT phone_hash FROM users WHERE user_id = $1")
            .bind(u as i64)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let phone_hash: String = row.get("phone_hash");
        assert_eq!(
            phone_hash, "",
            "phone_hash must be scrubbed, not just flagged"
        );

        assert!(store
            .is_phone_in_cooldown("secret-phone-hash")
            .await
            .unwrap());
    }

    #[sqlx::test]
    async fn erased_user_excluded_from_search_even_if_searchable_flag_was_set(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        // auth phone_hash (create_pending_user) and the search hash
        // (set_searchable) are deliberately independent columns now — this
        // one exercises the search hash the client would actually compute.
        let search_hash = format!("findme0{}", "0".repeat(57));
        let u = store
            .create_pending_user("unrelated-auth-hash")
            .await
            .unwrap();
        store
            .set_searchable(u, true, Some(&search_hash))
            .await
            .unwrap();
        let prefix = crate::search::hash_prefix(&search_hash).to_string();
        assert_eq!(PrefixIndex::bucket(&store, &prefix).await.len(), 1);

        store.erase_user(u).await.unwrap();
        assert_eq!(PrefixIndex::bucket(&store, &prefix).await.len(), 0);
    }

    #[sqlx::test]
    async fn opting_out_of_search_clears_the_search_hash(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        let search_hash = format!("optout0{}", "0".repeat(57));
        let u = store.create_pending_user("auth-hash").await.unwrap();
        store
            .set_searchable(u, true, Some(&search_hash))
            .await
            .unwrap();
        let prefix = crate::search::hash_prefix(&search_hash).to_string();
        assert_eq!(PrefixIndex::bucket(&store, &prefix).await.len(), 1);

        store.set_searchable(u, false, None).await.unwrap();
        assert_eq!(
            PrefixIndex::bucket(&store, &prefix).await.len(),
            0,
            "opting out must clear the stored search hash, not just the flag"
        );
    }

    #[sqlx::test]
    async fn find_user_by_handle_matches_claimed_username(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        let u = store.create_pending_user("h").await.unwrap();
        let (slot, _width) = store.claim_username(u, "carol").await.unwrap();

        assert_eq!(
            store.find_user_by_handle("carol", slot).await.unwrap(),
            Some(u)
        );
        assert_eq!(
            store.find_user_by_handle("carol", slot + 1).await.unwrap(),
            None
        );
        assert_eq!(store.find_user_by_handle("nobody", 1).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn session_roundtrip(pool: PgPool) {
        let store = DirectoryStore::from_pool(pool);
        let u = store.create_pending_user("h").await.unwrap();
        let token = store.create_session(u);
        assert_eq!(store.session_user_id(&token), Some(u));
        assert_eq!(store.session_user_id("not-a-real-token"), None);
    }
}
