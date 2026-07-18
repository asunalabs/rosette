package chat.app.directory

// The iOS simulator shares the host's loopback (unlike the Android emulator's
// 10.0.2.2), so localhost reaches a directory running on the dev machine.
actual fun defaultDirectoryBaseUrl(): String = "http://localhost:7444"
