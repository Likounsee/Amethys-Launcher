use amethys_errors::{AmethysError, ErrorCode};
use tracing_subscriber::{EnvFilter, fmt};

/// Initializes the process-wide structured tracing subscriber.
pub fn initialize() -> Result<(), AmethysError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).with_target(true).try_init().map_err(|error|
        AmethysError::new(ErrorCode::RuntimeInvalidTransition, format!("Could not initialize logging: {error}"))
    )
}
