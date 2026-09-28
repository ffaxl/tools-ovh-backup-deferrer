mod config;
mod deferrer;
mod ovh;
mod schedule;

use std::error::Error;
use std::process::ExitCode;

use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::ovh::Client;

#[tokio::main]
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
            error!(error = chain(error.as_ref()), "startup failed");
            ExitCode::FAILURE
        }
    }
}

/// Returns on SIGTERM, or with whatever prevented the start.
async fn run() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    let client = Client::new(config.base_url, config.credentials)?;
    let terminated =
        termination().map_err(|error| format!("cannot listen for SIGTERM: {error}"))?;

    info!(
        services = config.services.len(),
        dry_run = config.dry_run,
        "started"
    );
    // Dropping the loop mid-request is harmless: every write is repeated an hour later.
    tokio::select! {
        () = deferrer::run(&client, &config.services, config.dry_run) => {}
        () = terminated => info!("terminated"),
    }
    Ok(())
}

/// The error and every cause beneath it; the JSON log records an error's `Display` only.
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
