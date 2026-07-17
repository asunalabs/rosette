package chat.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        // DT16: draw behind the system bars so App()'s safeDrawingPadding can
        // inset content and lift the composer above the keyboard. The bars are
        // transparent (Theme.Chat), so the app's own bg shows through.
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent { App() }
    }
}
