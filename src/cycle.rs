//! The hourly cycle over the configured services: what to write, and when.

use std::time::Duration;

use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use tracing::{info, warn};

use crate::config::Service;
use crate::ovh::Client;

/// Runs a cycle now and then shortly after every full UTC hour, forever.
pub async fn run(client: &Client, services: &[Service], dry_run: bool) {
    loop {
        run_cycle(client, services, Timestamp::now(), dry_run).await;
        tokio::time::sleep(until_next_run(Timestamp::now())).await;
    }
}

/// Writes every service's target for `now`, one after another; a failure is logged and the
/// next service still runs.
async fn run_cycle(client: &Client, services: &[Service], now: Timestamp, dry_run: bool) {
    for service in services {
        let name = service.name.as_str();
        let target = target(now, service.offset);
        if dry_run {
            info!(service = name, %target, outcome = "dry-run");
            continue;
        }
        match client.reschedule(name, target).await {
            Ok(()) => info!(service = name, %target, outcome = "written"),
            Err(error) => {
                warn!(service = name, %target, outcome = "failed", error = crate::chain(&error));
            }
        }
    }
}

/// The schedule to write at `now`: the start of the previous UTC hour, plus the offset.
fn target(now: Timestamp, offset: u8) -> Time {
    let hour = now.to_zoned(TimeZone::UTC).time().hour();
    Time::midnight()
        .wrapping_add(SignedDuration::from_hours(i64::from(hour)))
        .wrapping_add(SignedDuration::from_mins(i64::from(offset) - 60))
}

/// How long to sleep from `now` until ten seconds past the start of the next UTC hour; waking
/// a little after the hour rather than on it leaves no doubt which hour `now` is in.
fn until_next_run(now: Timestamp) -> Duration {
    Duration::from_secs((3610 - now.as_second().rem_euclid(3600)).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use jiff::civil::time;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::ovh::tests::client_of;

    fn services() -> [Service; 2] {
        [
            Service {
                name: "vps-a.example".into(),
                offset: 0,
            },
            Service {
                name: "vps-b.example".into(),
                offset: 20,
            },
        ]
    }

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn just_after_the_hour() -> Timestamp {
        at("2026-09-28T15:00:10Z")
    }

    #[tokio::test]
    async fn every_service_is_written_at_its_offset_even_after_a_failure() {
        let server = MockServer::start().await;
        for (service, schedule, status) in [
            ("vps-a.example", "14:00:00", 500),
            ("vps-b.example", "14:20:00", 200),
        ] {
            Mock::given(method("POST"))
                .and(path(format!(
                    "/1.0/vps/{service}/automatedBackup/reschedule"
                )))
                .and(body_json(serde_json::json!({ "schedule": schedule })))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;
        }

        run_cycle(
            &client_of(&server),
            &services(),
            just_after_the_hour(),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn dry_run_writes_nothing() {
        let server = MockServer::start().await;

        run_cycle(
            &client_of(&server),
            &services(),
            just_after_the_hour(),
            true,
        )
        .await;

        let requests = server.received_requests().await.unwrap();
        assert!(requests.is_empty());
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
