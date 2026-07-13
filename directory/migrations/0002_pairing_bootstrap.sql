-- T25: one-time pairing bootstrap descriptor per user, so a search hit can
-- be turned into an MLS pairing without a QR/link exchange. The stored value
-- is opaque to directory: exactly the base64 ContactLink string
-- ChatEngine::contact_link() already produces for QR codes (a fresh
-- KeyPackage + this user's bootstrap mailbox endpoint) — directory never
-- parses it, just stores and serves it once.
CREATE TABLE pairing_bootstrap (
    user_id BIGINT PRIMARY KEY REFERENCES users (user_id) ON DELETE CASCADE,
    contact_link_b64 TEXT NOT NULL,
    uploaded_at BIGINT NOT NULL
);
