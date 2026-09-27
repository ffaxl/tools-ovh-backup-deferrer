use std::process::ExitCode;

use ovh_autobackup_deferrer::config::Config;
use ovh_autobackup_deferrer::deferrer;
use ovh_autobackup_deferrer::ovh::Client;
use tokio::sync::watch;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_target(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!(%error, "configuration rejected");
            return ExitCode::FAILURE;
        }
    };
    let client = match Client::connect(config.base_url.clone(), config.credentials.clone()).await {
        Ok(client) => client,
        Err(error) => {
            error!(%error, "provider clock unreachable");
            return ExitCode::FAILURE;
        }
    };

    let (stop, shutdown) = watch::channel(false);
    tokio::spawn(async move {
        termination().await;
        info!("termination requested");
        stop.send_replace(true);
    });

    info!(
        services = config.services.len(),
        dry_run = config.dry_run,
        "started"
    );
    deferrer::run(&client, &config.services, config.dry_run, shutdown).await;
    info!("stopped");
    ExitCode::SUCCESS
}

/// Resolves on `SIGTERM` or Ctrl-C; never, if neither can be listened for.
async fn termination() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            error!(%error, "cannot listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {}
                    () = interrupt => {}
                }
            }
            Err(error) => {
                error!(%error, "cannot listen for SIGTERM");
                interrupt.await;
            }
        }
    }

    #[cfg(not(unix))]
    interrupt.await;
}
