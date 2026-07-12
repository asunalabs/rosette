package chat.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import chat.engine.ChatEngine

/**
 * Walking-shell screen: constructs the real engine across the FFI seam and
 * shows what it says. NOT a wireframe screen — those are step 6, gated on
 * DT4 (DESIGN.md). This exists so every target has a runnable app proving
 * the whole stack links.
 */
@Composable
fun App() {
    MaterialTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            // remember {} — one engine per composition, as the contract
            // prescribes one per app start.
            val engine = remember { ChatEngine("dev") }
            val conversationCount = remember { engine.conversations().size }
            Column(
                modifier = Modifier.fillMaxSize().padding(24.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(style = MaterialTheme.typography.headlineSmall, text = "chat")
                Text("engine up — $conversationCount conversation(s)")
                Text(
                    style = MaterialTheme.typography.bodySmall,
                    text = "walking skeleton · UI lands after DESIGN.md (DT4)",
                )
            }
        }
    }
}
