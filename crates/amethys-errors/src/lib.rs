use thiserror::Error;

/// Stable error categories exposed to logs and diagnostic reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    RuntimeInvalidTransition,
    EventNoSubscribers,
    IpcInvalidLength,
    IpcFrameTooLarge,
    IpcSerialization,
    IpcDeserialization,
    IpcUnsupportedVersion,
    IpcResponseMismatch,
    IpcIo,
    ConfigurationRead,
    ConfigurationWrite,
    StorageOpen,
    StorageMigration,
    StorageQuery,
    ModuleAlreadyRegistered,
    ModuleNotFound,
    TaskAlreadyRunning,
    TaskNotFound,
    WorkerAlreadyRunning,
    WorkerNotFound,
    WorkerFailed,
    WorkerShutdownTimeout,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ErrorCode {
    /// Returns the stable public identifier for this category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeInvalidTransition => "AM-CORE-0001",
            Self::EventNoSubscribers => "AM-EVENT-0001",
            Self::IpcInvalidLength => "AM-IPC-0001",
            Self::IpcFrameTooLarge => "AM-IPC-0002",
            Self::IpcSerialization => "AM-IPC-0003",
            Self::IpcDeserialization => "AM-IPC-0004",
            Self::IpcIo => "AM-IPC-0005",
            Self::IpcUnsupportedVersion => "AM-IPC-0006",
            Self::IpcResponseMismatch => "AM-IPC-0007",
            Self::ConfigurationRead => "AM-CONFIG-0001",
            Self::ConfigurationWrite => "AM-CONFIG-0002",
            Self::StorageOpen => "AM-STORAGE-0001",
            Self::StorageMigration => "AM-STORAGE-0002",
            Self::StorageQuery => "AM-STORAGE-0003",
            Self::ModuleAlreadyRegistered => "AM-MODULE-0001",
            Self::ModuleNotFound => "AM-MODULE-0002",
            Self::TaskAlreadyRunning => "AM-TASK-0001",
            Self::TaskNotFound => "AM-TASK-0002",
            Self::WorkerAlreadyRunning => "AM-WORKER-0001",
            Self::WorkerNotFound => "AM-WORKER-0002",
            Self::WorkerFailed => "AM-WORKER-0003",
            Self::WorkerShutdownTimeout => "AM-WORKER-0004",
        }
    }
}

/// Error with a stable code and human-readable explanation.
#[derive(Debug, Error)]
#[error("[{code}] {message}")]
pub struct AmethysError {
    code: ErrorCode,
    message: String,
}

impl AmethysError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::{AmethysError, ErrorCode};

    #[test]
    fn error_codes_are_unique() {
        let codes = [
            ErrorCode::RuntimeInvalidTransition,
            ErrorCode::EventNoSubscribers,
            ErrorCode::IpcInvalidLength,
            ErrorCode::IpcFrameTooLarge,
            ErrorCode::IpcSerialization,
            ErrorCode::IpcDeserialization,
            ErrorCode::IpcUnsupportedVersion,
            ErrorCode::IpcIo,
            ErrorCode::ConfigurationRead,
            ErrorCode::ConfigurationWrite,
            ErrorCode::StorageOpen,
            ErrorCode::StorageMigration,
            ErrorCode::StorageQuery,
            ErrorCode::ModuleAlreadyRegistered,
            ErrorCode::ModuleNotFound,
            ErrorCode::TaskAlreadyRunning,
            ErrorCode::TaskNotFound,
            ErrorCode::WorkerAlreadyRunning,
            ErrorCode::WorkerNotFound,
            ErrorCode::WorkerFailed,
            ErrorCode::WorkerShutdownTimeout,
        ];
        let unique: std::collections::HashSet<_> =
            codes.into_iter().map(ErrorCode::as_str).collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn display_includes_error_code() {
        let error = AmethysError::new(ErrorCode::RuntimeInvalidTransition, "invalid state");
        assert!(error.to_string().contains("AM-CORE-0001"));
    }
}
