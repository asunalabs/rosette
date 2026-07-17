package chat.app.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import chat.app.session.Session
import chat.app.theme.HairlineDivider
import chat.app.theme.LocalChatPalette
import chat.app.theme.Rosette

/**
 * Issue #4: minimal settings surface — the Account card and the Change PIN
 * row. Quiet Room styling per DESIGN.md: 16dp cards, statement left, muted
 * explanation under it.
 */
@Composable
fun SettingsScreen(session: Session, onBack: () -> Unit, onChangePin: () -> Unit) {
    val palette = LocalChatPalette.current
    Column(modifier = Modifier.fillMaxSize().background(palette.bg)) {
        Row(
            modifier = Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.clickable(onClick = onBack).padding(8.dp)) {
                Text("←", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
            }
            Spacer(Modifier.width(4.dp))
            Text("Settings", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
        }
        HairlineDivider()

        Column(modifier = Modifier.fillMaxSize().padding(24.dp)) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(16.dp))
                    .background(palette.surface)
                    .padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Rosette(seed = session.handle, size = 44.dp)
                Spacer(Modifier.width(16.dp))
                Column {
                    Text(session.handle, style = MaterialTheme.typography.bodyLarge, color = palette.ink)
                    Text("Account", style = MaterialTheme.typography.labelMedium, color = palette.muted)
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(16.dp))
                    .background(palette.surface)
                    .clickable(onClick = onChangePin)
                    .padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text("Change PIN", style = MaterialTheme.typography.bodyLarge, color = palette.ink)
                    Text(
                        "The recovery PIN for this account",
                        style = MaterialTheme.typography.labelMedium,
                        color = palette.muted,
                    )
                }
                Text("→", style = MaterialTheme.typography.bodyLarge, color = palette.muted)
            }
        }
    }
}
