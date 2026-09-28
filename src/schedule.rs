//! Time arithmetic, free of I/O and of the real clock.

use std::time::Duration;

use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

/// The schedule to write at `now`: the start of the previous UTC hour, plus the offset.
pub fn target(now: Timestamp, offset: u8) -> Time {
    let hour = now.to_zoned(TimeZone::UTC).time().hour();
    Time::midnight()
        .wrapping_add(SignedDuration::from_hours(i64::from(hour)))
        .wrapping_add(SignedDuration::from_mins(i64::from(offset) - 60))
}

/// How long to sleep from `now` until ten seconds past the start of the next UTC hour; waking
/// a little after the hour rather than on it leaves no doubt which hour `now` is in.
pub fn until_next_run(now: Timestamp) -> Duration {
    Duration::from_secs((3610 - now.as_second().rem_euclid(3600)).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use jiff::civil::time;

    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn target_is_the_previous_hour_plus_the_offset() {
        for (now, minutes, expected) in [
            ("2026-09-28T15:37:12.5Z", 20, time(14, 20, 0, 0)),
            ("2026-09-28T00:20:00Z", 40, time(23, 40, 0, 0)),
        ] {
            assert_eq!(target(at(now), minutes), expected, "{now} +{minutes}");
        }
    }

    #[test]
    fn next_run_is_ten_seconds_into_the_next_hour() {
        assert_eq!(
            until_next_run(at("2026-09-28T14:59:50Z")),
            Duration::from_secs(20)
        );
    }
}
