-- Contact discovery (T27 follow-up): a SEPARATE, unkeyed SHA-256 hash from
-- the auth phone_hash (which stays Argon2id + secret pepper, OQ4,
-- untouched). The client computes this locally with no secret, matching
-- HIBP-style k-anonymity bucketing already used for /search. Populated ONLY
-- when a user opts into phone search (POST /searchable) — never at signup —
-- so a stolen DB exposes at most the reversible hash of accounts that
-- explicitly chose to be findable, not every registered phone number.
ALTER TABLE users ADD COLUMN phone_search_hash TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN phone_search_hash_prefix TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_phone_search_hash_prefix ON users (phone_search_hash_prefix);
