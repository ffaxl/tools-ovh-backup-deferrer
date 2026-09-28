mod config;
mod cycle;
mod ovh;

use std::process::ExitCode;

use anyhow::{Context, Result};

use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::ovh::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let filter = match EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()
    {
        Ok(filter) => filter,
        Err(error) => {
            eprintln!("RUST_LOG rejected: {error}");
            return ExitCode::FAILURE;
        }
    };
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_target(false)
        .with_env_filter(filter)
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = format!("{error:#}"), "stopped");
            ExitCode::FAILURE
        }
    }
}

/// Returns on SIGTERM, or with whatever prevented the start or failed a cycle; the exit status
/// then makes the failure visible as a restarting pod.
async fn run() -> Result<()> {
    let config = Config::from_env()?;
    let client = Client::new(config.base_url, config.credentials)?;
    let terminated = termination().context("cannot listen for SIGTERM")?;

    info!(
        services = config.services.len(),
        dry_run = config.dry_run,
        "started"
    );
    // Dropping the loop mid-request is harmless: every write is repeated an hour later.
    tokio::select! {
        cycle = cycle::run(&client, &config.services, config.dry_run) => cycle,
        () = terminated => {
            info!("terminated");
            Ok(())
        }
    }
}

/// Resolves on SIGTERM, which a process running as PID 1 in a container would otherwise ignore.
#[cfg(unix)]
fn termination() -> std::io::Result<impl Future<Output = ()>> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    Ok(async move {
        terminate.recv().await;
    })
}

/// Elsewhere there is no SIGTERM, and Ctrl-C ends the process by default.
#[cfg(not(unix))]
fn termination() -> std::io::Result<impl Future<Output = ()>> {
    Ok(std::future::pending())
}
