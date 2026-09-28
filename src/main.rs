use std::process::ExitCode;

use ovh_autobackup_deferrer::config::Config;
use ovh_autobackup_deferrer::deferrer;
use ovh_autobackup_deferrer::ovh::Client;
use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

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

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!(%error, "configuration rejected");
            return ExitCode::FAILURE;
        }
    };
    let client = match Client::new(config.base_url, config.credentials) {
        Ok(client) => client,
        Err(error) => {
            error!(%error, "HTTP client setup failed");
            return ExitCode::FAILURE;
        }
    };
    let terminated = match termination() {
        Ok(terminated) => terminated,
        Err(error) => {
            error!(%error, "cannot listen for SIGTERM");
            return ExitCode::FAILURE;
        }
    };

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
    ExitCode::SUCCESS
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
