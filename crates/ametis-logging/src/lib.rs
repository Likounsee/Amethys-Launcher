use ametis_errors::{AmetisError, ErrorCode};
use tracing_subscriber::{EnvFilter, fmt};

/// Initializes the process-wide structured tracing subscriber.
pub fn initialize() -> Result<(), AmetisError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).with_target(true).try_init().map_err(|error|
        AmetisError::new(ErrorCode::RuntimeInvalidTransition, format!("Could not initialize logging: {error}"))
    )
}
