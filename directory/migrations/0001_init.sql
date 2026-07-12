CREATE TABLE users (
    user_id BIGSERIAL PRIMARY KEY,
    phone_hash TEXT NOT NULL,
    phone_hash_prefix TEXT NOT NULL,
    verified BOOLEAN NOT NULL DEFAULT FALSE,
    nickname TEXT,
    discriminator INTEGER,
    searchable BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL,
    deleted_at BIGINT,
    UNIQUE (nickname, discriminator)
);

CREATE INDEX idx_phone_hash_prefix ON users (phone_hash_prefix);

CREATE TABLE nickname_widths (
    nickname TEXT PRIMARY KEY,
    width INTEGER NOT NULL
);

-- OQ5: 24-48h cooldown before a deleted account's phone number can
-- re-register. One row per deletion event; cooldown checked by phone_hash.
CREATE TABLE phone_cooldown (
    phone_hash TEXT NOT NULL,
    deleted_at BIGINT NOT NULL
);

CREATE INDEX idx_phone_cooldown_hash ON phone_cooldown (phone_hash);
