package chat.app.chatlist

import platform.Foundation.NSDate
import platform.Foundation.NSDateFormatter
import platform.Foundation.dateWithTimeIntervalSince1970

// 24-hour HH:mm to match the JVM actuals byte-for-byte (their DateTimeFormatter
// pattern is "HH:mm"). Explicit dateFormat forces 24h regardless of the device's
// 12/24h setting; NSDateFormatter defaults to the current time zone, matching
// ZoneId.systemDefault(). Reused instance — clock formatting is main-thread only.
private val CLOCK_FORMAT = NSDateFormatter().apply { dateFormat = "HH:mm" }

actual fun formatClockTime(epochMs: Long): String =
    CLOCK_FORMAT.stringFromDate(NSDate.dateWithTimeIntervalSince1970(epochMs / 1000.0))
