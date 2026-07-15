package chat.app.directory

import chat.engine.ChatEngine
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Issue #2: re-upload the recovery bundle after a contact change, debounced
 * so a burst of changes becomes exactly one PUT. Call [schedule] from any
 * site that changes contacts (v0.1: pairing completion). A no-op until
 * `backupEnroll` has run — `backupBundleCurrent()` returns null before then,
 * including on a non-persistent engine.
 */
class BackupUploader(
    private val scope: CoroutineScope,
    private val engine: ChatEngine,
    private val client: DirectoryClient,
    private val sessionToken: String,
    private val debounceMs: Long = 30_000,
) {
    private var pending: Job? = null

    fun schedule() {
        pending?.cancel()
        pending = scope.launch {
            delay(debounceMs)
            val bundle = engine.backupBundleCurrent() ?: return@launch
            try {
                client.putBackup(sessionToken, bundle)
            } catch (_: DirectoryException) {
                // Best-effort: the next contact change retries, and restore
                // still works from the last successful upload.
            }
        }
    }
}
