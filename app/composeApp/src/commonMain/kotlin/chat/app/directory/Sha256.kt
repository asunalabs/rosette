package chat.app.directory

// Unkeyed, client-side contact-discovery hash (no server secret involved —
// distinct from the server's Argon2id auth phone_hash/pepper, OQ4,
// untouched). expect/actual per project convention (DirectoryBaseUrl,
// SessionStore) even though today's two actuals are identical, since both
// targets are JVM-family; keeps the door open for a non-JVM iOS actual.
expect fun sha256Hex(input: String): String
