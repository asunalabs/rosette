package chat.app.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * DESIGN.md "Color" table. THE ACCENT IS A SWAP-POINT: this is the one
 * definition site — every reference elsewhere goes through `LocalChatPalette`
 * or `MaterialTheme.colorScheme`, never a hardcoded hex.
 */
data class ChatPalette(
    val bg: Color,
    val surface: Color,
    val surface2: Color,
    val ink: Color,
    val muted: Color,
    val hairline: Color,
    val accent: Color,
    val accentStrong: Color,
    val accentSoft: Color,
    val bubbleMine: Color,
    val onBubbleMine: Color,
    val bubbleTheirs: Color,
    val error: Color,
    val errorSoft: Color,
    val warning: Color,
    val warningSoft: Color,
    val info: Color,
    val onAccent: Color,
)

private val LightPalette = ChatPalette(
    bg = Color(0xFFFFFFFF),
    surface = Color(0xFFF7F6F6),
    surface2 = Color(0xFFEFEDEE),
    ink = Color(0xFF1A1618),
    muted = Color(0xFF625B5E),
    hairline = Color(0xFFE5E1E3),
    accent = Color(0xFF5B3A6C),
    accentStrong = Color(0xFF432852),
    accentSoft = Color(0xFFEDE4F2),
    bubbleMine = Color(0xFF5B3A6C),
    onBubbleMine = Color(0xFFFFFFFF),
    bubbleTheirs = Color(0xFFF0EEF0),
    error = Color(0xFFC62828),
    errorSoft = Color(0xFFFBE9E7),
    warning = Color(0xFF7A5500),
    warningSoft = Color(0xFFF5ECD4),
    info = Color(0xFF4C5A6B),
    onAccent = Color(0xFFFFFFFF),
)

private val DarkPalette = ChatPalette(
    bg = Color(0xFF0B090C),
    surface = Color(0xFF1C1920),
    surface2 = Color(0xFF262229),
    ink = Color(0xFFECE9EB),
    muted = Color(0xFF948D91),
    hairline = Color(0xFF2A252B),
    accent = Color(0xFFB08CC9),
    accentStrong = Color(0xFFC3A4D8),
    accentSoft = Color(0xFF2B2233),
    bubbleMine = Color(0xFF6B4183),
    onBubbleMine = Color(0xFFFFFFFF),
    bubbleTheirs = Color(0xFF242027),
    error = Color(0xFFF16A6F),
    errorSoft = Color(0xFF35201B),
    warning = Color(0xFFD4A945),
    warningSoft = Color(0xFF322B18),
    info = Color(0xFF94A3B8),
    onAccent = Color(0xFF121013),
)

val LocalChatPalette = compositionLocalOf { LightPalette }

// ponytail: real IBM Plex Sans/Mono OFL TTFs aren't bundled into composeApp
// resources yet (DESIGN.md "Loading"), so this falls back to each platform's
// default sans/monospace. Swap FontFamily.Default/Monospace for loaded
// Font(...) resources once the TTFs land — sizes/weights below already match
// the DESIGN.md scale so nothing else here needs to change.
private val sansFamily = FontFamily.Default
private val monoFamily = FontFamily.Monospace

private fun Double.em() = androidx.compose.ui.unit.TextUnit(this.toFloat(), androidx.compose.ui.unit.TextUnitType.Em)

// DESIGN.md "Typography" — headlineSmall doubles as the one statement style
// (onboarding headlines, pledge, empty states); quarantined by convention,
// not a separate type, since Material3's Typography has no such slot.
private val chatTypography = Typography(
    headlineSmall = TextStyle(fontFamily = sansFamily, fontWeight = FontWeight.Bold, fontSize = 28.sp, letterSpacing = (-0.02).em()),
    bodyLarge = TextStyle(fontFamily = sansFamily, fontWeight = FontWeight.Normal, fontSize = 16.sp, lineHeight = 24.sp),
    labelLarge = TextStyle(fontFamily = sansFamily, fontWeight = FontWeight.SemiBold, fontSize = 16.sp),
    labelMedium = TextStyle(fontFamily = sansFamily, fontWeight = FontWeight.Medium, fontSize = 13.5.sp),
    labelSmall = TextStyle(fontFamily = sansFamily, fontWeight = FontWeight.Medium, fontSize = 12.sp),
)

/** DESIGN.md "Code/crypto facts" — quarantined to OTP entry, safety numbers, fingerprints. */
val ChatMonoStyle = TextStyle(fontFamily = monoFamily, fontWeight = FontWeight.Normal, fontSize = 22.sp, letterSpacing = 0.08.em())

@Composable
fun ChatTheme(content: @Composable () -> Unit) {
    val palette = if (isSystemInDarkTheme()) DarkPalette else LightPalette
    val colorScheme = if (isSystemInDarkTheme()) {
        darkColorScheme(
            primary = palette.accent, onPrimary = palette.onAccent,
            background = palette.bg, onBackground = palette.ink,
            surface = palette.surface, onSurface = palette.ink,
            error = palette.error,
        )
    } else {
        lightColorScheme(
            primary = palette.accent, onPrimary = palette.onAccent,
            background = palette.bg, onBackground = palette.ink,
            surface = palette.surface, onSurface = palette.ink,
            error = palette.error,
        )
    }
    CompositionLocalProvider(LocalChatPalette provides palette) {
        MaterialTheme(colorScheme = colorScheme, typography = chatTypography, content = content)
    }
}
