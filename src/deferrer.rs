//! The hourly cycle over the configured services.

use std::error::Error;

use jiff::Timestamp;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Service;
use crate::ovh::Client;
use crate::schedule;

/// Writes every service's target for `now`, one after another. A failure is logged and the
/// next service still runs; a raised `shutdown` stops before the next service.
pub async fn run_cycle(
    client: &Client,
    services: &[Service],
    now: Timestamp,
    dry_run: bool,
    shutdown: &watch::Receiver<bool>,
) {
    for service in services {
        if *shutdown.borrow() {
            break;
        }
        defer(client, service, now, dry_run).await;
    }
}

/// Runs a cycle now and then shortly after every full UTC hour until `shutdown` is raised.
pub async fn run(
    client: &Client,
    services: &[Service],
    dry_run: bool,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        run_cycle(client, services, Timestamp::now(), dry_run, &shutdown).await;
        let wait = schedule::until_next_run(Timestamp::now());
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            _ = shutdown.wait_for(|stop| *stop) => break,
        }
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
