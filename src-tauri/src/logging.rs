use std::sync::OnceLock;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    error::{AppError, AppResult},
    paths::AppPaths,
};

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();
static LOGGING_READY: OnceLock<()> = OnceLock::new();

pub fn init_logging(paths: &AppPaths) -> AppResult<()> {
    if LOGGING_READY.get().is_some() {
        return Ok(());
    }

    let file_appender = tracing_appender::rolling::never(&paths.logs_dir, "blocksmith.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking),
        )
        .try_init()
        .map_err(|error| AppError::Internal(format!("failed to initialize logging: {error}")))?;

    let _ = LOG_GUARD.set(guard);
    let _ = LOGGING_READY.set(());
    tracing::info!("logging initialized at {}", paths.logs_dir.to_string_lossy());
    Ok(())
}

