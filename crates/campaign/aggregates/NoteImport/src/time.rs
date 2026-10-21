//! The time a note is imported and analysed, as its row writes it.

/// `millis` since the epoch as a note's dates are written: RFC 3339, in
/// UTC, to the millisecond (`2026-10-17T10:00:00.000Z`).
#[must_use]
pub fn timestamp(millis: u64) -> String {
    let seconds = millis / 1000;
    let (year, month, day) = civil(seconds / 86_400);
    let time = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        time / 3600,
        time % 3600 / 60,
        time % 60,
        millis % 1000
    )
}

/// The date `days` after 1970-01-01 (H. Hinnant's `civil_from_days`).
const fn civil(days: u64) -> (u64, u64, u64) {
    let days = days + 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The date `days` after 1970-01-01, counted a day at a time.
    fn counted(days: u64) -> (u64, u64, u64) {
        let leap = |year: u64| {
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
        };
        let (mut year, mut left) = (1970, days);
        loop {
            let length = if leap(year) { 366 } else { 365 };
            if left < length {
                break;
            }
            left -= length;
            year += 1;
        }
        let mut month = 1;
        for length in [
            31,
            if leap(year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ] {
            if left < length {
                break;
            }
            left -= length;
            month += 1;
        }
        (year, month, left + 1)
    }

    #[test]
    fn every_day_from_1970_to_2500_is_its_calendar_date() {
        for days in 0..193_944 {
            assert_eq!(civil(days), counted(days), "{days}");
        }
    }

    #[test]
    fn a_timestamp_is_rfc_3339_in_utc_to_the_millisecond() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(1_700_000_123_456), "2023-11-14T22:15:23.456Z");
        assert_eq!(timestamp(1_709_164_799_999), "2024-02-28T23:59:59.999Z");
        assert_eq!(timestamp(1_709_164_800_007), "2024-02-29T00:00:00.007Z");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(timestamp(1_792_234_861_010), "2026-10-17T11:01:01.010Z");
    }
}
