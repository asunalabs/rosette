-- Issue #2: E2E-encrypted recovery bundles. Every payload column is
-- ciphertext, a client-generated salt, or a salted hash — the server can
-- verify the auth hashes (rate-limited retrieval, issue #3) but can never
-- derive a key from this table. An attacker holding a full dump can
-- brute-force w_pin offline (accepted, documented ceiling); the phrase wrap
-- and the blob itself stay strong even then.
-- pin_attempts/locked_until back the 10-attempt lockout enforced by the
-- retrieval endpoint (issue #3).
CREATE TABLE backups (
    user_id BIGINT PRIMARY KEY REFERENCES users (user_id) ON DELETE CASCADE,
    blob BYTEA NOT NULL,
    w_pin BYTEA NOT NULL,
    salt_p BYTEA NOT NULL,
    w_phrase BYTEA NOT NULL,
    salt_f BYTEA NOT NULL,
    auth_pin_hash BYTEA NOT NULL,
    salt_a BYTEA NOT NULL,
    auth_phrase_hash BYTEA NOT NULL,
    salt_pa BYTEA NOT NULL,
    pin_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until BIGINT,
    updated_at BIGINT NOT NULL
);
