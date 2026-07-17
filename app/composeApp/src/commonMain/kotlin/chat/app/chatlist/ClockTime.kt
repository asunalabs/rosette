package chat.app.chatlist

/**
 * DT11: epoch millis → local "HH:mm" for the inline bubble timestamp.
 * expect/actual per project convention (`Sha256`, `SessionStore`) — both JVM
 * actuals are identical today, and iOS gets its own when that target lands.
 */
expect fun formatClockTime(epochMs: Long): String
