use ametis_errors::{AmetisError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// User-level application settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub start_agent_with_session: bool,
    pub offline_reminder_days: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { schema_version: 1, start_agent_with_session: true, offline_reminder_days: 7 }
    }
}

impl AppConfig {
    /// Loads configuration; defaults are used only when the file is absent.
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, AmetisError> {
        let path = path.as_ref();
        let bytes = match fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(AmetisError::new(
                ErrorCode::ConfigurationRead,
                format!("Could not read {}: {error}", path.display()),
            )),
        };
        serde_json::from_slice(&bytes).map_err(|error| AmetisError::new(
            ErrorCode::ConfigurationRead,
            format!("Configuration file is invalid: {error}"),
        ))
    }

    /// Saves through a temporary file so an interrupted write cannot truncate the previous file.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<(), AmetisError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| AmetisError::new(
                ErrorCode::ConfigurationWrite,
                format!("Could not create {}: {error}", parent.display()),
            ))?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error|
            AmetisError::new(ErrorCode::ConfigurationWrite, error.to_string())
        )?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, bytes).await.map_err(|error| AmetisError::new(
            ErrorCode::ConfigurationWrite,
            format!("Could not write {}: {error}", temporary.display()),
        ))?;
        fs::rename(&temporary, path).await.map_err(|error| AmetisError::new(
            ErrorCode::ConfigurationWrite,
            format!("Could not replace {}: {error}", path.display()),
        ))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[tokio::test]
    async fn missing_file_returns_defaults() {
        let path = std::env::temp_dir().join(format!("ametis-{}.json", uuid::Uuid::new_v4()));
        assert_eq!(AppConfig::load(path).await.expect("default config"), AppConfig::default());
    }
}
