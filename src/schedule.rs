//! Time arithmetic, free of I/O and of the real clock.

use std::time::Duration;

use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

const NANOS_PER_HOUR: i128 = 3_600_000_000_000;
const NANOS_PER_SECOND: i128 = 1_000_000_000;
/// Waking a little after the hour rather than on it leaves no doubt which hour `now` is in.
const NANOS_PAST_THE_HOUR: i128 = 10 * NANOS_PER_SECOND;

/// Minutes a service's schedule sits behind the shared target, keeping its window apart from
/// every other service's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset(u8);

impl Offset {
    const MAX_MINUTES: u8 = 59;

    pub fn from_minutes(minutes: u8) -> Option<Self> {
        (minutes <= Self::MAX_MINUTES).then_some(Self(minutes))
    }
}

/// The schedule to write at `now`: the start of the previous UTC hour, less the offset.
pub fn target(now: Timestamp, offset: Offset) -> Time {
    let hour = now.to_zoned(TimeZone::UTC).time().hour();
    Time::midnight()
        .wrapping_add(SignedDuration::from_hours(i64::from(hour)))
        .wrapping_sub(SignedDuration::from_mins(60 + i64::from(offset.0)))
}

/// How long to sleep from `now` until ten seconds past the start of the next UTC hour.
pub fn until_next_run(now: Timestamp) -> Duration {
    let into_hour = now.as_nanosecond().rem_euclid(NANOS_PER_HOUR);
    let remaining = NANOS_PER_HOUR + NANOS_PAST_THE_HOUR - into_hour;
    // `remaining` lies in NANOS_PAST_THE_HOUR + 1..=NANOS_PER_HOUR + NANOS_PAST_THE_HOUR, so
    // both parts fit their targets.
    Duration::new(
        (remaining / NANOS_PER_SECOND) as u64,
        (remaining % NANOS_PER_SECOND) as u32,
    )
}

#[cfg(test)]
mod tests {
    use jiff::civil::time;

    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn offset(minutes: u8) -> Offset {
        Offset::from_minutes(minutes).unwrap()
    }

    #[test]
    fn target_on_the_hour_is_the_previous_hour() {
        assert_eq!(
            target(at("2026-09-28T15:00:00Z"), offset(0)),
            time(14, 0, 0, 0)
        );
    }

    #[test]
    fn target_mid_hour_ignores_minutes_and_seconds() {
        assert_eq!(
            target(at("2026-09-28T15:37:12.5Z"), offset(0)),
            time(14, 0, 0, 0)
        );
    }

    #[test]
    fn target_subtracts_the_offset() {
        assert_eq!(
            target(at("2026-09-28T15:00:00Z"), offset(20)),
            time(13, 40, 0, 0)
        );
        assert_eq!(
            target(at("2026-09-28T15:00:00Z"), offset(40)),
            time(13, 20, 0, 0)
        );
    }

    #[test]
    fn target_wraps_across_midnight() {
        assert_eq!(
            target(at("2026-09-28T00:20:00Z"), offset(40)),
            time(22, 20, 0, 0)
        );
    }

    #[test]
    fn next_run_from_just_before_the_hour_is_ten_seconds_into_it() {
        assert_eq!(
            until_next_run(at("2026-09-28T14:59:59.999Z")),
            Duration::from_millis(10_001)
        );
    }

    #[test]
    fn next_run_from_exactly_on_the_hour_skips_to_the_following_one() {
        assert_eq!(
            until_next_run(at("2026-09-28T15:00:00Z")),
            Duration::from_secs(3610)
        );
    }

    #[test]
    fn next_run_from_just_after_a_run_is_an_hour_away() {
        assert_eq!(
            until_next_run(at("2026-09-28T15:00:10.001Z")),
            Duration::from_millis(3_599_999)
        );
    }

    #[test]
    fn next_run_from_before_the_ten_seconds_still_skips_to_the_next_hour() {
        assert_eq!(
            until_next_run(at("2026-09-28T15:00:05Z")),
            Duration::from_secs(3605)
        );
    }
}
