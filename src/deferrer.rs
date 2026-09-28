//! The hourly cycle over the configured services.

use std::error::Error;

use jiff::Timestamp;
use tracing::{info, warn};

use crate::config::Service;
use crate::ovh::Client;
use crate::schedule;

/// Writes every service's target for `now`, one after another. A failure is logged and the
/// next service still runs.
pub async fn run_cycle(client: &Client, services: &[Service], now: Timestamp, dry_run: bool) {
    for service in services {
        defer(client, service, now, dry_run).await;
    }
}

/// Runs a cycle now and then shortly after every full UTC hour, forever.
pub async fn run(client: &Client, services: &[Service], dry_run: bool) {
    loop {
        run_cycle(client, services, Timestamp::now(), dry_run).await;
        tokio::time::sleep(schedule::until_next_run(Timestamp::now())).await;
    }
}

async fn defer(client: &Client, service: &Service, now: Timestamp, dry_run: bool) {
    let name = service.name.as_str();
    let target = schedule::target(now, service.offset);
    if dry_run {
        info!(service = name, %target, outcome = "dry-run");
        return;
    }
    match client.reschedule(name, target).await {
        Ok(()) => info!(service = name, %target, outcome = "written"),
        Err(error) => warn!(service = name, %target, outcome = "failed", error = chain(&error)),
    }
}

/// The error and every cause beneath it, which `Display` alone leaves out.
fn chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
