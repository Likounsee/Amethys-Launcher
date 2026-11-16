use amethys_errors::{AmethysError, ErrorCode};
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
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, AmethysError> {
        let path = path.as_ref();
        let bytes = match fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(AmethysError::new(
                ErrorCode::ConfigurationRead,
                format!("Could not read {}: {error}", path.display()),
            )),
        };
        let config: Self = serde_json::from_slice(&bytes).map_err(|error| AmethysError::new(
            ErrorCode::ConfigurationRead,
            format!("Configuration file is invalid: {error}"),
        ))?;
        if config.schema_version != 1 {
            return Err(AmethysError::new(
                ErrorCode::ConfigurationRead,
                format!(
                    "Unsupported configuration schema version {} (supported: 1).",
                    config.schema_version
                ),
            ));
        }
        Ok(config)
    }

    /// Saves through a temporary file so an interrupted write cannot truncate the previous file.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<(), AmethysError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).await.map_err(|error| AmethysError::new(
                ErrorCode::ConfigurationWrite,
                format!("Could not create {}: {error}", parent.display()),
            ))?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error|
            AmethysError::new(ErrorCode::ConfigurationWrite, error.to_string())
        )?;
        // A unique sibling avoids collisions between concurrent saves and keeps the
        // final rename on the same filesystem.
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temporary, bytes).await.map_err(|error| AmethysError::new(
            ErrorCode::ConfigurationWrite,
            format!("Could not write {}: {error}", temporary.display()),
        ))?;
        if let Err(error) = fs::rename(&temporary, path).await {
            let _cleanup_result = fs::remove_file(&temporary).await;
            return Err(AmethysError::new(
                ErrorCode::ConfigurationWrite,
                format!("Could not replace {}: {error}", path.display()),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[tokio::test]
    async fn missing_file_returns_defaults() {
        let path = std::env::temp_dir().join(format!("amethys-{}.json", uuid::Uuid::new_v4()));
        assert_eq!(AppConfig::load(path).await.expect("default config"), AppConfig::default());
    }

    #[tokio::test]
    async fn load_rejects_unsupported_schema_versions() {
        let path = std::env::temp_dir().join(format!("amethys-{}.json", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, r#"{"schema_version":2}"#)
            .await
            .expect("future-version fixture is written");

        let error = AppConfig::load(&path).await.expect_err("unsupported schema must fail");
        assert_eq!(error.code(), amethys_errors::ErrorCode::ConfigurationRead);
        assert!(error.message().contains("Unsupported configuration schema version 2"));
        tokio::fs::remove_file(path).await.expect("fixture is removed");
    }

    #[tokio::test]
    async fn save_supports_a_relative_filename_in_the_current_directory() {
        let filename = format!("amethys-{}.json", uuid::Uuid::new_v4());
        let path = std::path::Path::new(&filename);
        let config = AppConfig {
            schema_version: 1,
            start_agent_with_session: false,
            offline_reminder_days: 14,
        };

        config.save(path).await.expect("relative config save succeeds");
        assert_eq!(AppConfig::load(path).await.expect("saved config loads"), config);
        tokio::fs::remove_file(path).await.expect("temporary test config is removed");
    }

    #[tokio::test]
    async fn saving_replaces_an_existing_configuration() {
        let path = std::env::temp_dir().join(format!("amethys-{}.json", uuid::Uuid::new_v4()));
        let first = AppConfig::default();
        first.save(&path).await.expect("initial save succeeds");

        let second = AppConfig {
            schema_version: 1,
            start_agent_with_session: false,
            offline_reminder_days: 30,
        };
        second.save(&path).await.expect("replacement save succeeds");
        assert_eq!(AppConfig::load(&path).await.expect("replacement loads"), second);
        tokio::fs::remove_file(path).await.expect("test config is removed");
    }
}
