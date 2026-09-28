//! The hourly cycle over the configured services.

use jiff::Timestamp;
use tracing::{info, warn};

use crate::config::Service;
use crate::ovh::Client;
use crate::schedule;

/// Runs a cycle now and then shortly after every full UTC hour, forever.
pub async fn run(client: &Client, services: &[Service], dry_run: bool) {
    loop {
        run_cycle(client, services, Timestamp::now(), dry_run).await;
        tokio::time::sleep(schedule::until_next_run(Timestamp::now())).await;
    }
}

/// Writes every service's target for `now`, one after another; a failure is logged and the
/// next service still runs.
async fn run_cycle(client: &Client, services: &[Service], now: Timestamp, dry_run: bool) {
    for service in services {
        let name = service.name.as_str();
        let target = schedule::target(now, service.offset);
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

#[cfg(test)]
mod tests {
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

    fn just_after_the_hour() -> Timestamp {
        "2026-09-28T15:00:10Z".parse().unwrap()
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
}
