package chat.app.theme

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * DESIGN.md "Quiet Room" primitives — soft surfaces, full-pill buttons and
 * inputs, 18dp bubbles. Deliberately not Material3's Button/
 * OutlinedTextField/Switch: those read as generic Android chrome, which the
 * system rejects (see DESIGN.md "Mood"). Composable names still carry the
 * legacy Instrument- prefix; renaming them is churn, not design.
 */

@Composable
fun InstrumentButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    loading: Boolean = false,
    primary: Boolean = true,
) {
    val palette = LocalChatPalette.current
    // DT15: loading is an ACTIVE state, not a disabled one. The accent is the
    // only color allowed to mean "we intend this" (DESIGN.md:195), so it must
    // stay lit while the user's intent executes — only a genuinely can't-act
    // button drops to surface2. (Callers pass enabled = !loading, so without
    // this the pill went grey the instant it started working.)
    val interactive = enabled || loading
    val bg = if (!interactive) palette.surface2 else if (primary) palette.accent else palette.surface
    val fg = if (!interactive) palette.muted else if (primary) palette.onAccent else palette.ink
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(50))
            .background(bg)
            .clickable(enabled = enabled && !loading, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        // Always the label — callers pass "Claiming…"/"Verifying…" while loading,
        // so the copy is never dead. A pulsing alpha carries the "working" signal
        // instead of Material's CircularProgressIndicator (a hard-NO in this system).
        val alpha = if (loading) {
            val transition = rememberInfiniteTransition(label = "button-loading")
            transition.animateFloat(
                initialValue = 0.45f,
                targetValue = 1f,
                animationSpec = infiniteRepeatable(tween(650), RepeatMode.Reverse),
                label = "pulse",
            ).value
        } else {
            1f
        }
        Text(text, style = MaterialTheme.typography.labelLarge, color = fg, modifier = Modifier.alpha(alpha))
    }
}

@Composable
fun InstrumentField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    label: String = "",
    placeholder: String = "",
    mono: Boolean = false,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
) {
    val palette = LocalChatPalette.current
    val interaction = remember { MutableInteractionSource() }
    val focused by interaction.collectIsFocusedAsState()
    val textStyle = (if (mono) ChatMonoStyle else MaterialTheme.typography.bodyLarge).copy(color = palette.ink)

    Column(modifier = modifier) {
        if (label.isNotEmpty()) {
            Text(label, style = MaterialTheme.typography.labelSmall, color = palette.muted)
            Spacer(Modifier.height(6.dp))
        }
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(52.dp)
                .clip(RoundedCornerShape(50))
                .background(palette.surface)
                .then(if (focused) Modifier.border(1.5.dp, palette.accent, RoundedCornerShape(50)) else Modifier)
                .padding(horizontal = 20.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (value.isEmpty() && placeholder.isNotEmpty()) {
                Text(placeholder, style = textStyle.copy(color = palette.muted))
            }
            BasicTextField(
                value = value,
                onValueChange = onValueChange,
                singleLine = true,
                textStyle = textStyle,
                cursorBrush = SolidColor(palette.accent),
                interactionSource = interaction,
                keyboardOptions = keyboardOptions,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

// ponytail: curated EU-heavy calling-code→flag map (longer codes first), 🌐 fallback —
// a full ITU table adds nothing until country selection becomes a real picker.
private val FLAGS = listOf(
    "+420" to "🇨🇿", "+421" to "🇸🇰", "+358" to "🇫🇮", "+380" to "🇺🇦", "+351" to "🇵🇹", "+353" to "🇮🇪",
    "+49" to "🇩🇪", "+43" to "🇦🇹", "+48" to "🇵🇱", "+33" to "🇫🇷", "+39" to "🇮🇹", "+34" to "🇪🇸",
    "+31" to "🇳🇱", "+32" to "🇧🇪", "+45" to "🇩🇰", "+46" to "🇸🇪", "+47" to "🇳🇴", "+36" to "🇭🇺",
    "+40" to "🇷🇴", "+30" to "🇬🇷", "+41" to "🇨🇭", "+44" to "🇬🇧", "+1" to "🇺🇸",
)

fun flagFor(code: String): String = FLAGS.firstOrNull { code.startsWith(it.first) }?.second ?: "🌐"

/**
 * Signal's phone-entry pill (DESIGN.md "Layout"): flag + country code in the
 * leading segment, hairline divider, then the number.
 */
@Composable
fun InstrumentPhoneField(
    countryCode: String,
    onCountryCodeChange: (String) -> Unit,
    number: String,
    onNumberChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalChatPalette.current
    val fieldStyle = MaterialTheme.typography.bodyLarge.copy(color = palette.ink)
    val phoneKeyboard = KeyboardOptions(keyboardType = KeyboardType.Phone)

    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(56.dp)
            .clip(RoundedCornerShape(50))
            .background(palette.surface)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // DT7: the flag follows whatever code is actually there (🌐 when blank),
        // never a hardcoded founder-country default.
        Text(flagFor(countryCode), style = fieldStyle)
        Spacer(Modifier.width(8.dp))
        Box(modifier = Modifier.width(52.dp)) {
            if (countryCode.isEmpty()) {
                // Neutral prompt, not a specific country: the real default comes
                // from the caller's locale (defaultDialCode); this only shows if
                // the user clears the field or their region is unlisted.
                Text("+__", style = fieldStyle.copy(color = palette.muted))
            }
            BasicTextField(
                value = countryCode,
                onValueChange = onCountryCodeChange,
                singleLine = true,
                textStyle = fieldStyle,
                cursorBrush = SolidColor(palette.accent),
                keyboardOptions = phoneKeyboard,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Text("⌄", style = MaterialTheme.typography.labelMedium, color = palette.muted)
        Spacer(Modifier.width(12.dp))
        Box(Modifier.width(1.dp).height(24.dp).background(palette.hairline))
        Spacer(Modifier.width(12.dp))
        Box(modifier = Modifier.weight(1f)) {
            if (number.isEmpty()) {
                Text("Your phone number", style = fieldStyle.copy(color = palette.muted))
            }
            BasicTextField(
                value = number,
                onValueChange = onNumberChange,
                singleLine = true,
                textStyle = fieldStyle,
                cursorBrush = SolidColor(palette.accent),
                keyboardOptions = phoneKeyboard,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/**
 * Which token a status chip wears. DESIGN.md "Voice quarantine": `error` only
 * ever means a real failure, so a vendor outage we're waiting out is
 * [Warning], not [Error] — the user did nothing wrong. Getting this backwards
 * both spends the error token and blames the wrong party.
 */
enum class StatusTone { Warning, Error, Positive }

/** Pill chip carrying one line of status, adjacent to the control it's about (never parked at the screen edge). */
@Composable
fun InstrumentStatusChip(text: String, tone: StatusTone, modifier: Modifier = Modifier) {
    val palette = LocalChatPalette.current
    val (fg, bg) = when (tone) {
        StatusTone.Warning -> palette.warning to palette.warningSoft
        StatusTone.Error -> palette.error to palette.errorSoft
        // DESIGN.md: "Success/verified = accent, not green: trust wears the brand color."
        StatusTone.Positive -> palette.accent to palette.accentSoft
    }
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(50))
            .background(bg)
            .padding(horizontal = 14.dp, vertical = 7.dp),
    ) {
        Text(text, style = MaterialTheme.typography.labelMedium, color = fg)
    }
}

@Composable
fun InstrumentToggle(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    /** DT5: false while the real state is still unknown — dimmed and inert, so
     *  it never reads as a confident OFF the user didn't choose. */
    enabled: Boolean = true,
) {
    val palette = LocalChatPalette.current
    Box(
        modifier = modifier
            .width(48.dp)
            .height(28.dp)
            .then(if (enabled) Modifier else Modifier.alpha(0.5f))
            .clip(RoundedCornerShape(50))
            .background(if (checked) palette.accent else palette.surface2)
            .clickable(enabled = enabled) { onCheckedChange(!checked) }
            .padding(3.dp),
        contentAlignment = if (checked) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Box(
            modifier = Modifier
                .size(22.dp)
                .clip(RoundedCornerShape(50))
                .background(if (checked) palette.onAccent else palette.muted),
        )
    }
}

@Composable
fun HairlineDivider(modifier: Modifier = Modifier, color: Color = LocalChatPalette.current.hairline, thickness: Dp = 1.dp) {
    Box(modifier.fillMaxWidth().height(thickness).background(color))
}

/** Two-way segmented switch (e.g. FindPeopleScreen's Username/Phone toggle) — pill chips. */
@Composable
fun InstrumentSegments(labels: List<String>, selected: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val palette = LocalChatPalette.current
    Row(modifier = modifier, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        labels.forEachIndexed { i, label ->
            val isSelected = i == selected
            Box(
                modifier = Modifier
                    .clip(RoundedCornerShape(50))
                    .background(if (isSelected) palette.surface2 else Color.Transparent)
                    .clickable { onSelect(i) }
                    .padding(horizontal = 14.dp, vertical = 8.dp),
            ) {
                Text(label, style = MaterialTheme.typography.labelMedium, color = if (isSelected) palette.ink else palette.muted)
            }
        }
    }
}

/** DESIGN.md "Bottom navigation": floating pill tab bar, selected tab = surface2 pill. */
@Composable
fun InstrumentTabBar(tabs: List<String>, selected: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val palette = LocalChatPalette.current
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp)
            .clip(RoundedCornerShape(50))
            .background(palette.surface)
            .padding(6.dp),
    ) {
        tabs.forEachIndexed { i, label ->
            val isSelected = i == selected
            Box(
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(50))
                    .background(if (isSelected) palette.surface2 else Color.Transparent)
                    .clickable { onSelect(i) }
                    .padding(vertical = 10.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, style = MaterialTheme.typography.labelMedium, color = if (isSelected) palette.ink else palette.muted)
            }
        }
    }
}

/**
 * DESIGN.md "Shape" bubbles: uniformly rounded 18dp; mine = solid accent
 * family + white text, theirs = quiet gray; timestamp trails the last line
 * inline, never its own row.
 *
 * DT3: `pending` dims the bubble while a send is in flight; `failed` renders
 * the FFI's own "Not sent yet · tap to retry" line (`ffi/src/lib.rs:70`) in the
 * `error` tone (a send that didn't leave the device IS a real failure, so
 * `error` is the right token per DESIGN.md, not `warning`). `onRetry` fires on
 * a tap of that line. Both default off, so the other call sites are unchanged.
 */
@Composable
fun MessageBubble(
    body: String,
    mine: Boolean,
    modifier: Modifier = Modifier,
    time: String? = null,
    pending: Boolean = false,
    failed: Boolean = false,
    onRetry: (() -> Unit)? = null,
) {
    val palette = LocalChatPalette.current
    val bg = if (mine) palette.bubbleMine else palette.bubbleTheirs
    val fg = if (mine) palette.onBubbleMine else palette.ink
    Column(
        modifier = modifier.fillMaxWidth(),
        horizontalAlignment = if (mine) Alignment.End else Alignment.Start,
    ) {
        Row(
            modifier = Modifier
                .clip(RoundedCornerShape(18.dp))
                .background(bg)
                .then(if (pending) Modifier.alpha(0.6f) else Modifier)
                .padding(horizontal = 14.dp, vertical = 9.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            Text(
                body,
                style = MaterialTheme.typography.bodyLarge,
                color = fg,
                overflow = TextOverflow.Clip,
                modifier = Modifier.weight(1f, fill = false),
            )
            if (time != null) {
                Spacer(Modifier.width(8.dp))
                Text(time, style = MaterialTheme.typography.labelSmall.copy(fontSize = 11.sp), color = fg.copy(alpha = 0.72f))
            }
        }
        if (failed) {
            Text(
                "Not sent yet · tap to retry",
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 11.sp),
                color = palette.error,
                modifier = Modifier
                    .padding(top = 2.dp, end = 4.dp, start = 4.dp)
                    .then(if (onRetry != null) Modifier.clickable(onClick = onRetry) else Modifier),
            )
        }
    }
}

/** Signal-alike chat-list row (72dp per DESIGN.md "Spacing"), Rosette instead of a photo avatar. */
@Composable
fun ChatListRow(
    displayName: String,
    lastMessage: String?,
    unread: Int,
    verified: Boolean,
    seed: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalChatPalette.current
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(72.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Rosette(seed = seed, verified = verified, size = 48.dp)
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(displayName, style = MaterialTheme.typography.labelLarge, color = palette.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Spacer(Modifier.height(2.dp))
            Text(
                lastMessage ?: "No messages yet",
                style = MaterialTheme.typography.labelMedium,
                color = palette.muted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (unread > 0) {
            Spacer(Modifier.width(8.dp))
            Box(
                modifier = Modifier.clip(RoundedCornerShape(50)).background(palette.accent).padding(horizontal = 7.dp, vertical = 2.dp),
            ) {
                Text(unread.toString(), style = MaterialTheme.typography.labelSmall, color = palette.onAccent)
            }
        }
    }
}
