package chat.app.onboarding

import platform.Foundation.NSLocale
import platform.Foundation.countryCode
import platform.Foundation.currentLocale

// ISO region (e.g. "DE"), or "" when the locale has none — dialCodeFor()
// already tolerates "" the same way the JVM Locale.getDefault().country path does.
actual fun defaultRegionCode(): String = NSLocale.currentLocale.countryCode ?: ""
