-- Sessions move out of process memory and into the DB.
--
-- They were deliberately ephemeral, and store.rs justified it: "a directory
-- restart means callers re-verify; that's an acceptable ceiling at this stage."
-- The ceiling was acceptable. The premise was false — callers cannot re-verify.
-- The app persists the token (`SessionStore`), and `App.kt` only shows
-- onboarding when the stored session is null, so a restart did not send anyone
-- back through verification: it left every installed client holding a token the
-- server had forgotten, with `clear()` never called and no 401 handled
-- anywhere. One deploy permanently broke search and pairing for every user,
-- recoverable only by wiping app data.
--
-- Three findings collapse into this table:
--   * the restart lockout above;
--   * the map only ever grew — insert with no remove and no TTL (`created_at`
--     + the read-time cutoff bound it now);
--   * ET2's revocation had to scan every live session, because a
--     `token -> user_id` map has no `user_id -> tokens` direction. The index
--     below is that direction.
--
-- No FK to `users`: erasure *tombstones* a row (`deleted_at` set) rather than
-- deleting it, so `ON DELETE CASCADE` would never fire and would buy nothing.
-- `erase_user` deletes sessions explicitly, in the same transaction, and
-- `session_user_id` joins `users` and re-checks `deleted_at` anyway — the same
-- defense-in-depth ET6 kept on the client gate: one path being correct today is
-- not a reason for the other to be able to express the wrong thing.
CREATE TABLE sessions (
    token TEXT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    created_at BIGINT NOT NULL
);

-- ET2 revocation reads this; nothing else queries by user_id.
CREATE INDEX idx_sessions_user_id ON sessions (user_id);
