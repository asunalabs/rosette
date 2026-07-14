package chat.app.directory

// ponytail: hardcoded per-platform dev default (directory's default port,
// directory/src/main.rs). No settings UI to override it yet — add one
// when there's a real deployed directory host to point at.
expect fun defaultDirectoryBaseUrl(): String
