mod common;

use std::error::Error;

use ovh_autobackup_deferrer::config::Service;
use ovh_autobackup_deferrer::deferrer::run_cycle;
use ovh_autobackup_deferrer::schedule::Offset;
use tokio::sync::watch;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn service(name: &str, minutes: u8) -> Result<Service, Box<dyn Error>> {
    let offset = Offset::from_minutes(minutes).ok_or("offset out of range")?;
    Ok(Service {
        name: name.into(),
        offset,
    })
}

async fn answer_reschedule(server: &MockServer, service: &str, schedule: &str, status: u16) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/1.0/vps/{service}/automatedBackup/reschedule"
        )))
        .and(body_json(serde_json::json!({ "schedule": schedule })))
        .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({ "id": 1 })))
        .expect(1)
        .mount(server)
        .await;
}

async fn nothing_was_sent(server: &MockServer) -> bool {
    server
        .received_requests()
        .await
        .is_some_and(|requests| requests.is_empty())
}

fn just_after_the_hour() -> Result<jiff::Timestamp, Box<dyn Error>> {
    Ok("2026-09-28T15:00:10Z".parse()?)
}

#[tokio::test]
async fn every_service_is_written_at_its_offset_even_after_a_failure() -> Result<(), Box<dyn Error>>
{
    let server = MockServer::start().await;
    answer_reschedule(&server, "vps-a.example", "14:00:00", 500).await;
    answer_reschedule(&server, "vps-b.example", "14:20:00", 200).await;
    let client = common::client(&server)?;
    let services = [service("vps-a.example", 0)?, service("vps-b.example", 20)?];
    let (_running, shutdown) = watch::channel(false);

    run_cycle(&client, &services, just_after_the_hour()?, false, &shutdown).await;
    Ok(())
}

#[tokio::test]
async fn dry_run_writes_nothing() -> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    let client = common::client(&server)?;
    let services = [service("vps-a.example", 0)?, service("vps-b.example", 20)?];
    let (_running, shutdown) = watch::channel(false);

    run_cycle(&client, &services, just_after_the_hour()?, true, &shutdown).await;

    assert!(nothing_was_sent(&server).await);
    Ok(())
}

#[tokio::test]
async fn raised_shutdown_touches_no_service() -> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    let client = common::client(&server)?;
    let (_sender, shutdown) = watch::channel(true);

    run_cycle(
        &client,
        &[service("vps-a.example", 0)?],
        just_after_the_hour()?,
        false,
        &shutdown,
    )
    .await;

    assert!(nothing_was_sent(&server).await);
    Ok(())
}
