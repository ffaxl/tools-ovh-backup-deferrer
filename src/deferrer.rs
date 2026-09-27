//! The hourly cycle over the configured services.

use std::error::Error;

use jiff::Timestamp;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Service;
use crate::ovh::Client;
use crate::schedule;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Written,
    DryRun,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::DryRun => "dry-run",
            Self::Failed => "failed",
        }
    }
}

/// Writes every service's target for `now`, one after another. A failure is logged and the
/// next service still runs; a raised `shutdown` stops before the next service.
pub async fn run_cycle(
    client: &Client,
    services: &[Service],
    now: Timestamp,
    dry_run: bool,
    shutdown: &watch::Receiver<bool>,
) -> Vec<Outcome> {
    let mut outcomes = Vec::with_capacity(services.len());
    for service in services {
        if *shutdown.borrow() {
            break;
        }
        outcomes.push(defer(client, service, now, dry_run).await);
    }
    outcomes
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

async fn defer(client: &Client, service: &Service, now: Timestamp, dry_run: bool) -> Outcome {
    let name = service.name.as_str();
    let target = schedule::target(now, service.offset);
    if dry_run {
        info!(service = name, %target, outcome = Outcome::DryRun.as_str());
        return Outcome::DryRun;
    }
    match client.reschedule(name, target).await {
        Ok(()) => {
            info!(service = name, %target, outcome = Outcome::Written.as_str());
            Outcome::Written
        }
        Err(error) => {
            warn!(service = name, %target, outcome = Outcome::Failed.as_str(), error = chain(&error));
            Outcome::Failed
        }
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
