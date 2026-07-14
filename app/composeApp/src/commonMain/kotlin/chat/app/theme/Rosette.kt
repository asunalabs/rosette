package chat.app.theme

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/**
 * DESIGN.md "The Rosette" — guilloché identicon, ported from the reference
 * algorithm in docs/design/design-preview.html (`rosette()`/`bandPath()`).
 * Deterministic from [seed] — today a conversation/user id or handle string
 * hashed locally; swap [fingerprintBytes] for real MLS key-fingerprint bytes
 * once the UI has them, the curve math underneath doesn't change.
 */
private val INKS: List<Pair<Color, Color>> = listOf(
    Color(0xFF5B3A6C) to Color(0xFFB08CC9),
    Color(0xFF432852) to Color(0xFF9A77B5),
    Color(0xFF6E4555) to Color(0xFFB07E92),
    Color(0xFF7E2A3E) to Color(0xFFC98A97),
    Color(0xFF5B4A68) to Color(0xFF9C89B0),
    Color(0xFF4A5568) to Color(0xFF8B9BB4),
    Color(0xFF2F4858) to Color(0xFF7593A6),
    Color(0xFF365F71) to Color(0xFF6FA3B8),
    Color(0xFF584338) to Color(0xFFA08575),
    Color(0xFF706030) to Color(0xFFB5A253),
    Color(0xFF3C3C3C) to Color(0xFF9A9A9A),
    Color(0xFF432E3B) to Color(0xFF8E7185),
)

// ponytail: FNV-1a over the seed string, not a real hash-derived key
// fingerprint — good enough for a stable per-id/handle identicon. Swap for
// the MLS fingerprint's own bytes once that's threaded into the UI layer.
private fun fingerprintBytes(seed: String, count: Int = 10): IntArray {
    var h = -0x7ee3623b
    for (c in seed) {
        h = h xor c.code
        h *= 0x01000193
    }
    return IntArray(count) { i ->
        h = h xor (i * -0x61c88647)
        h *= 0x01000193
        h = h xor (h ushr 15)
        h and 0xFF
    }
}

private fun bandPath(cx: Float, cy: Float, base: Float, amp1: Float, amp2: Float, k: Int, phase: Float, steps: Int): Path {
    val path = Path()
    for (i in 0..steps) {
        val t = (i.toFloat() / steps) * (2f * PI.toFloat())
        val r = base + amp1 * cos(k * t + phase) + amp2 * cos(2 * k * t)
        val x = cx + r * cos(t)
        val y = cy + r * sin(t)
        if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
    }
    path.close()
    return path
}

@Composable
fun Rosette(seed: String, modifier: Modifier = Modifier, verified: Boolean = false, size: Dp = 48.dp) {
    val palette = LocalChatPalette.current
    val dark = isSystemInDarkTheme()
    val bytes = remember(seed) { fingerprintBytes(seed) }
    val ink = if (dark) INKS[bytes[0] % 12].second else INKS[bytes[0] % 12].first
    val ink2Index = (bytes[0] % 12 + 3 + bytes[1] % 5) % 12
    val ink2 = if (dark) INKS[ink2Index].second else INKS[ink2Index].first
    val k = 5 + bytes[2] % 5
    val base = 26f + bytes[3] % 8
    val amp1 = 6f + bytes[4] % 9
    val amp2 = 1f + bytes[5] % 5
    val phase0 = (bytes[6] % 64) / 64f * (2f * PI.toFloat())
    val sizePx = size.value
    val nLines = if (sizePx >= 56) 5 else 3
    val strokeWidth = if (sizePx >= 56) 1.1f else 1.7f
    val coreRadius = 2.4f + bytes[9] % 4

    Canvas(modifier = modifier.size(size)) {
        val s = min(this.size.width, this.size.height) / 100f
        drawCircle(color = palette.surface, radius = 49f * s, center = Offset(50f * s, 50f * s))
        drawCircle(color = palette.hairline, radius = 49f * s, center = Offset(50f * s, 50f * s), style = Stroke(width = 1f * s))
        scale(s, pivot = Offset.Zero) {
            for (i in 0 until nLines) {
                val shrink = 1f - i * (0.10f + (bytes[7] % 4) * 0.012f)
                val ph = phase0 + i * ((bytes[8] % 16) / 40f)
                val col = if (i == nLines - 2) ink2 else ink
                drawPath(
                    path = bandPath(50f, 50f, base * shrink, amp1 * shrink, amp2, k, ph, 140),
                    color = col,
                    alpha = if (i == 0) 0.95f else 0.75f,
                    style = Stroke(width = strokeWidth),
                )
            }
            drawCircle(color = ink, radius = coreRadius, center = Offset(50f, 50f))
            if (verified) {
                val rw = if (sizePx >= 56) 0.9f else 1.3f
                drawPath(
                    path = bandPath(50f, 50f, 45.5f, 1.6f, 0.6f, k * 3, phase0, 160),
                    color = ink,
                    alpha = 0.95f,
                    style = Stroke(width = rw),
                )
            }
        }
    }
}
