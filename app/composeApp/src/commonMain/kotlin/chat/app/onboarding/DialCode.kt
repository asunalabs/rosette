package chat.app.onboarding

/**
 * DT7: the user's SIM/locale region as an ISO 3166 alpha-2 code (e.g. "DE"),
 * or "" if unknown. expect/actual per project convention (`Sha256`,
 * `SessionStore`, `ClockTime`); iOS gets its own actual when that target lands.
 */
expect fun defaultRegionCode(): String

/**
 * DT7: the phone-entry field's default dial code, derived from the user's own
 * locale — never a hardcoded founder-country hint. Empty when the region is
 * unknown or unlisted, so the user fills it themselves rather than the app
 * shipping a confidently wrong `+420`.
 */
fun defaultDialCode(): String = dialCodeFor(defaultRegionCode())

/** Pure region → dial-code lookup (testable without the platform locale). Case-
 *  insensitive; "" for an unknown or unlisted region — never a wrong default. */
internal fun dialCodeFor(region: String): String = DIAL_CODES[region.uppercase()] ?: ""

// EU-27 (the mainstream target audience) plus common neighbours and majors. A
// region not in this map defaults to empty, not a guess: a missing code the user
// fills in is honest; a wrong one submitted on their behalf is the DT7 bug.
private val DIAL_CODES: Map<String, String> = mapOf(
    "AT" to "+43", "BE" to "+32", "BG" to "+359", "HR" to "+385", "CY" to "+357",
    "CZ" to "+420", "DK" to "+45", "EE" to "+372", "FI" to "+358", "FR" to "+33",
    "DE" to "+49", "GR" to "+30", "HU" to "+36", "IE" to "+353", "IT" to "+39",
    "LV" to "+371", "LT" to "+370", "LU" to "+352", "MT" to "+356", "NL" to "+31",
    "PL" to "+48", "PT" to "+351", "RO" to "+40", "SK" to "+421", "SI" to "+386",
    "ES" to "+34", "SE" to "+46",
    "GB" to "+44", "CH" to "+41", "NO" to "+47", "IS" to "+354", "UA" to "+380",
    "US" to "+1", "CA" to "+1",
)
