//! Query-supplied reference time for temporal predicates (ADR-0017).

use crate::error::SpatialError;

/// A point in time the engine can test rule windows against: the naive-local
/// day-of-week (ISO 8601, 1 = Monday .. 7 = Sunday) and hour of day (0..=23).
/// The engine has no wall clock; the query supplies this via its `at` member,
/// so evaluation stays deterministic and pure.
///
/// The fields are private so the documented ranges (day `1..=7`, hour `0..=23`)
/// are enforced at construction: `parse_iso8601` and [`TemporalInstant::new`]
/// are the only ways to build a value, so a malformed programmatic instant
/// cannot be constructed as structurally-valid but semantically impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalInstant {
    /// ISO 8601 day-of-week: 1 = Monday .. 7 = Sunday.
    day_of_week: u8,
    /// Hour of day, 0..=23 (the v1 window granularity).
    hour: u8,
}

impl TemporalInstant {
    /// Build a [`TemporalInstant`] from validated (day, hour) components.
    ///
    /// Returns `None` when `day_of_week` is outside `1..=7` or `hour` is
    /// outside `0..=23`, so callers cannot construct an invalid invariant.
    pub fn new(day_of_week: u8, hour: u8) -> Option<Self> {
        if (1..=7).contains(&day_of_week) && hour <= 23 {
            Some(TemporalInstant {
                day_of_week,
                hour,
            })
        } else {
            None
        }
    }

    /// The ISO 8601 day-of-week (`1 = Monday .. 7 = Sunday`), always in range.
    pub fn day_of_week(&self) -> u8 {
        self.day_of_week
    }

    /// The hour of day (`0..=23`), always in range.
    pub fn hour(&self) -> u8 {
        self.hour
    }

    /// Parse a naive ISO-8601 datetime (`YYYY-MM-DDTHH:MM` or with seconds)
    /// into the local day-of-week + hour. Timezone offsets (`Z`/`±HH:MM`) are
    /// rejected for v1 — windows are local-frame; offset handling is additive
    /// (ADR-0017). Failures return a structured [`SpatialError`]
    /// (`SR_INVALID_QUERY`), matching the other core parsers so a direct caller
    /// receives stable error classification rather than a bare `String`.
    pub fn parse_iso8601(input: &str) -> Result<Self, SpatialError> {
        let bytes = input.as_bytes();
        let with_seconds = bytes.len() == 19;
        // All parse failures share the invalid-query code; the message carries
        // the specific reason for a caller to surface.
        let parse_error = |detail: String| SpatialError::invalid_query(detail);
        if bytes.len() != 16 && !with_seconds {
            return Err(parse_error(format!(
                "expected 'YYYY-MM-DDTHH:MM' (optionally with seconds), got {input:?}"
            )));
        }
        if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' || bytes[13] != b':' {
            return Err(parse_error(format!(
                "expected 'YYYY-MM-DDTHH:MM', got {input:?}"
            )));
        }
        let digits = |lo: usize, hi: usize| (lo..hi).all(|i| bytes[i].is_ascii_digit());
        if !digits(0, 4) || !digits(5, 7) || !digits(8, 10) || !digits(11, 13) || !digits(14, 16) {
            return Err(parse_error(format!("non-numeric field in {input:?}")));
        }
        if with_seconds && (bytes[16] != b':' || !digits(17, 19)) {
            return Err(parse_error(format!("invalid seconds in {input:?}")));
        }
        let field = |lo: usize, hi: usize| {
            bytes[lo..hi]
                .iter()
                .fold(0i64, |acc, &b| acc * 10 + (b - b'0') as i64)
        };
        let year = field(0, 4);
        let month = field(5, 7);
        let day = field(8, 10);
        let hour = field(11, 13);
        let minute = field(14, 16);
        let second = if with_seconds { field(17, 19) } else { 0 };
        if !(1..=12).contains(&month) {
            return Err(parse_error(format!("month out of range in {input:?}")));
        }
        if !(1..=days_in_month(year, month)).contains(&day) {
            return Err(parse_error(format!("day out of range in {input:?}")));
        }
        if hour > 23 {
            return Err(parse_error(format!("hour out of range in {input:?}")));
        }
        if minute > 59 {
            return Err(parse_error(format!("minute out of range in {input:?}")));
        }
        if second > 59 {
            return Err(parse_error(format!("second out of range in {input:?}")));
        }
        Ok(TemporalInstant {
            day_of_week: iso_weekday(year, month, day),
            hour: hour as u8,
        })
    }
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The ISO 8601 day-of-week (1 = Monday .. 7 = Sunday) of a Gregorian date,
/// via Zeller's congruence.
fn iso_weekday(year: i64, month: i64, day: i64) -> u8 {
    let (m, y) = if month < 3 {
        (month + 12, year - 1)
    } else {
        (month, year)
    };
    let k = y % 100;
    let j = y / 100;
    // `h`: 0 = Saturday, 1 = Sunday, 2 = Monday, .., 6 = Friday.
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    ((h + 5) % 7 + 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_dates_to_their_iso_weekday() {
        // 1970-01-01 was a Thursday (4); 2026-08-23 was a Sunday (7).
        assert_eq!(
            TemporalInstant::parse_iso8601("1970-01-01T00:00").unwrap(),
            TemporalInstant::new(4, 0).unwrap()
        );
        assert_eq!(
            TemporalInstant::parse_iso8601("2026-08-23T14:30").unwrap(),
            TemporalInstant::new(7, 14).unwrap()
        );
        // 2026-08-24 was a Monday (1).
        assert_eq!(
            TemporalInstant::parse_iso8601("2026-08-24T09:00")
                .unwrap()
                .day_of_week(),
            1
        );
    }

    #[test]
    fn seconds_are_accepted_and_ignored() {
        let instant = TemporalInstant::parse_iso8601("2026-08-23T14:30:45").unwrap();
        assert_eq!(instant, TemporalInstant::new(7, 14).unwrap());
    }

    #[test]
    fn new_rejects_out_of_range_components() {
        for (day, hour) in [(0, 5), (8, 5), (3, 24), (3, 255)] {
            assert_eq!(TemporalInstant::new(day, hour), None, "{day}:{hour}");
        }
        assert_eq!(TemporalInstant::new(1, 0), Some(TemporalInstant::new(1, 0).unwrap()));
        assert_eq!(TemporalInstant::new(7, 23), Some(TemporalInstant::new(7, 23).unwrap()));
    }

    #[test]
    fn leap_year_days_are_checked() {
        // 2024 is a leap year (Feb 29 is valid); 2026 is not.
        assert!(TemporalInstant::parse_iso8601("2024-02-29T00:00").is_ok());
        assert!(TemporalInstant::parse_iso8601("2026-02-29T00:00").is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "2026-08-23",
            "2026-08-23 14:30",
            "2026/08/23T14:30",
            "2026-08-23T14:30:45Z",
            "2026-08-23T14:30+02:00",
            "2026-08-23T14:30:99",
            "2026-13-01T00:00",
            "2026-02-30T00:00",
            "2026-08-23T24:00",
            "2026-08-23T14:60",
            "not-a-date",
        ] {
            assert!(TemporalInstant::parse_iso8601(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parse_failures_carry_the_invalid_query_code() {
        let error = TemporalInstant::parse_iso8601("not-a-date").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::InvalidQuery);
        assert!(error.message.contains("not-a-date"));
    }
}