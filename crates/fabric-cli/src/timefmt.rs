use std::time::{SystemTime, UNIX_EPOCH};

/// The current UTC time as RFC 3339 with second precision, e.g. `2026-10-19T12:00:00Z`.
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_unix(secs)
}

/// A Unix time as RFC 3339 UTC, second precision.
pub fn rfc3339(secs: u64) -> String {
    format_unix(secs)
}

fn format_unix(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Proleptic Gregorian date to days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` to the Unix time of that day's start (UTC). Invalid dates are `None`.
pub fn parse_date(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = text[..4].parse().ok()?;
    let month: u32 = text[5..7].parse().ok()?;
    let day: u32 = text[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // Rejects days past the month's end, such as 2026-02-30.
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    u64::try_from(days * 86_400).ok()
}

/// RFC 3339 UTC as written by this module (`YYYY-MM-DDTHH:MM:SSZ`) to a Unix time.
pub fn parse_rfc3339(text: &str) -> Option<u64> {
    if text.len() != 20 || !text.ends_with('Z') || text.as_bytes()[10] != b'T' {
        return None;
    }
    let day = parse_date(&text[..10])?;
    let hours: u64 = text[11..13].parse().ok()?;
    let minutes: u64 = text[14..16].parse().ok()?;
    let seconds: u64 = text[17..19].parse().ok()?;
    (hours < 24 && minutes < 60 && seconds < 61)
        .then_some(day + hours * 3600 + minutes * 60 + seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_unix(1_792_454_399), "2026-10-19T23:59:59Z");
    }

    #[test]
    fn parses_what_it_formats() {
        for secs in [0, 951_782_400, 1_792_454_399] {
            assert_eq!(parse_rfc3339(&format_unix(secs)), Some(secs));
        }
        assert_eq!(parse_date("2026-10-19"), Some(1_792_368_000));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("26-10-19"), None);
    }
}
