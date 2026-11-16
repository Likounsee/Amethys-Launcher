use amethys_errors::{AmethysError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::fs;

/// Resolves the shared per-user data directory used by both the agent and CLI.
///
/// AMETHYS_DATA_DIR is an explicit override. Otherwise, the platform's
/// conventional per-user application-data location is used; the current
/// working directory is never used as a fallback.
pub fn data_directory() -> io::Result<PathBuf> {
    resolve_data_directory(
        std::env::var_os("AMETHYS_DATA_DIR"),
        std::env::var_os("APPDATA"),
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("HOME"),
        std::env::var_os("XDG_DATA_HOME"),
    )
}

fn resolve_data_directory(
    override_path: Option<std::ffi::OsString>,
    app_data: Option<std::ffi::OsString>,
    local_app_data: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    xdg_data_home: Option<std::ffi::OsString>,
 ) -> io::Result<PathBuf> {
    #[cfg(unix)]
    let _ = (&app_data, &local_app_data);
    #[cfg(target_os = "macos")]
    let _ = &xdg_data_home;
    #[cfg(windows)]
    let _ = (&home, &xdg_data_home);

    if let Some(path) = override_path.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    #[cfg(windows)]
    {
        let base = app_data.or(local_app_data).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "APPDATA or LOCALAPPDATA must be set to locate Amethys data",
            )
        })?;
        Ok(PathBuf::from(base).join("Amethys"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = home.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "HOME must be set to locate Amethys data",
            )
        })?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Amethys"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = xdg_data_home.filter(|value| Path::new(value).is_absolute()) {
            return Ok(PathBuf::from(xdg).join("amethys"));
        }
        let home = home.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "HOME must be set to locate Amethys data",
            )
        })?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("amethys"))
    }

    #[cfg(not(any(windows, unix)))]
    {
        let _ = (app_data, local_app_data, home, xdg_data_home);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Amethys data directories are not defined for this operating system",
        ))
    }
}

/// Creates the application data directory and applies private Unix permissions.
///
/// On Unix, the directory is restricted to its owner (0700), protecting the
/// SQLite database and local IPC socket stored inside it.
pub fn prepare_data_directory() -> io::Result<PathBuf> {
    let path = data_directory()?;
    std::fs::create_dir_all(&path)?;
    restrict_data_directory_permissions(&path)?;
    Ok(path)
}

fn restrict_data_directory_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

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
        if self.schema_version != 1 {
            return Err(AmethysError::new(
                ErrorCode::ConfigurationWrite,
                format!(
                    "Unsupported configuration schema version {} (supported: 1).",
                    self.schema_version
                ),
            ));
        }

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
        if let Err(error) = fs::write(&temporary, bytes).await {
            let _cleanup_result = fs::remove_file(&temporary).await;
            return Err(AmethysError::new(
                ErrorCode::ConfigurationWrite,
                format!("Could not write {}: {error}", temporary.display()),
            ));
        }
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
    use super::{AppConfig, resolve_data_directory};
    use std::{ffi::OsString, path::Path};
    #[cfg(any(windows, all(unix, not(target_os = "macos"))))]
    use std::io;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn data_directory_prefers_absolute_xdg_location() {
        let path = resolve_data_directory(
            None,
            None,
            None,
            Some(OsString::from("/home/tester")),
            Some(OsString::from("/mnt/data")),
        )
        .expect("XDG data directory resolves");
        assert_eq!(path, Path::new("/mnt/data/amethys"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn data_directory_ignores_relative_xdg_location() {
        let path = resolve_data_directory(
            None,
            None,
            None,
            Some(OsString::from("/home/tester")),
            Some(OsString::from("relative/data")),
        )
        .expect("home data directory resolves");
        assert_eq!(path, Path::new("/home/tester/.local/share/amethys"));
    }

    #[cfg(unix)]
    #[test]
    fn prepared_data_directory_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("amethys-private-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("temporary data directory is created");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("initial permissions are set");

        super::restrict_data_directory_permissions(&path)
            .expect("private permissions are applied");
        let mode = std::fs::metadata(&path)
            .expect("directory metadata is available")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        std::fs::remove_dir(path).expect("temporary directory is removed");
    }

    #[test]
    fn data_directory_override_has_priority() {
        let path = resolve_data_directory(
            Some(OsString::from("/custom/amethys")),
            Some(OsString::from("/unused/appdata")),
            Some(OsString::from("/unused/local")),
            Some(OsString::from("/unused/home")),
            Some(OsString::from("/unused/xdg")),
        )
        .expect("explicit override resolves");
        assert_eq!(path, Path::new("/custom/amethys"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn data_directory_requires_home_when_xdg_is_unusable() {
        let error = resolve_data_directory(None, None, None, None, None)
            .expect_err("missing HOME must not fall back to current directory");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(windows)]
    #[test]
    fn data_directory_prefers_roaming_app_data() {
        let path = resolve_data_directory(
            None,
            Some(OsString::from(r"C:\Users\tester\AppData\Roaming")),
            Some(OsString::from(r"C:\Users\tester\AppData\Local")),
            None,
            None,
        )
        .expect("APPDATA resolves");
        assert_eq!(path, Path::new(r"C:\Users\tester\AppData\Roaming\Amethys"));
    }

    #[cfg(windows)]
    #[test]
    fn data_directory_falls_back_to_local_app_data() {
        let path = resolve_data_directory(
            None,
            None,
            Some(OsString::from(r"C:\Users\tester\AppData\Local")),
            None,
            None,
        )
        .expect("LOCALAPPDATA fallback resolves");
        assert_eq!(path, Path::new(r"C:\Users\tester\AppData\Local\Amethys"));
    }

    #[cfg(windows)]
    #[test]
    fn data_directory_requires_an_app_data_root() {
        let error = resolve_data_directory(None, None, None, None, None)
            .expect_err("missing app data variables must fail");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn data_directory_uses_application_support() {
        let path = resolve_data_directory(
            None,
            None,
            None,
            Some(OsString::from("/Users/tester")),
            None,
        )
        .expect("macOS data directory resolves");
        assert_eq!(
            path,
            Path::new("/Users/tester/Library/Application Support/Amethys")
        );
    }

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
    async fn save_rejects_unsupported_schema_versions_without_touching_file() {
        let path = std::env::temp_dir().join(format!("amethys-{}.json", uuid::Uuid::new_v4()));
        let original = r#"{"schema_version":1,"start_agent_with_session":false,"offline_reminder_days":21}"#;
        tokio::fs::write(&path, original).await.expect("original config is written");

        let unsupported = AppConfig {
            schema_version: 2,
            start_agent_with_session: true,
            offline_reminder_days: 30,
        };
        let error = unsupported.save(&path).await.expect_err("unsupported schema must fail");
        assert_eq!(error.code(), amethys_errors::ErrorCode::ConfigurationWrite);
        assert!(error.message().contains("Unsupported configuration schema version 2"));
        assert_eq!(tokio::fs::read_to_string(&path).await.expect("original remains"), original);

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
