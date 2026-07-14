package chat.app.directory

// Android emulator's loopback to the host machine is 10.0.2.2, not localhost.
actual fun defaultDirectoryBaseUrl(): String = "http://10.0.2.2:7444"
