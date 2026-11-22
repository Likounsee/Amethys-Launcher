//! Local media layout for Amethys.
//!
//! This crate deliberately does not download remote artwork. It provides safe,
//! deterministic locations for user-library media and provider catalogue cache.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Icon,
    Logo,
    Cover,
    Banner,
}

impl MediaKind {
    pub const ALL: [Self; 4] = [Self::Icon, Self::Logo, Self::Cover, Self::Banner];

    pub const fn file_stem(self) -> &'static str {
        match self {
            Self::Icon => "icon",
            Self::Logo => "logo",
            Self::Cover => "cover",
            Self::Banner => "banner",
        }
    }
}

/// A provider namespace; it is never used as the Amethys Game ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKey(String);

impl ProviderKey {
    pub fn new(value: impl Into<String>) -> Result<Self, MediaPathError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 48
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(MediaPathError::InvalidProvider);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaPathError {
    #[error("provider key or game ID is invalid")]
    InvalidProvider,
    #[error("media extension must be png, jpg, jpeg or bmp")]
    InvalidExtension,
    #[error("source media file does not exist or is not a regular file")]
    InvalidSource,
    #[error("media directory could not be prepared: {0}")]
    Io(#[from] io::Error),
}

/// Resolves media paths beneath the per-user Amethys data directory.
#[derive(Debug, Clone)]
pub struct MediaStore {
    root: PathBuf,
}

impl MediaStore {
    /// data_root is normally returned by amethys_config::data_directory().
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        Self {
            root: data_root.into().join("media"),
        }
    }

    /// Creates the library and remote-cache roots.
    pub fn prepare(&self) -> Result<(), MediaPathError> {
        fs::create_dir_all(self.library_root())?;
        fs::create_dir_all(self.catalogue_cache_root())?;
        Ok(())
    }

    /// Root directory containing all local Amethys media.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn library_root(&self) -> PathBuf {
        self.root.join("library")
    }

    pub fn catalogue_cache_root(&self) -> PathBuf {
        self.root.join("catalogue-cache")
    }

    /// Path for an image owned by the user's local library entry.
    pub fn library_asset(
        &self,
        game_id: &str,
        kind: MediaKind,
        extension: &str,
    ) -> Result<PathBuf, MediaPathError> {
        validate_game_id(game_id)?;
        Ok(self.library_root().join(game_id).join(format!(
            "{}.{}",
            kind.file_stem(),
            valid_extension(extension)?
        )))
    }

    /// Path for a remote catalogue image cached for a provider item.
    /// External IDs are hex-encoded so arbitrary provider IDs remain one safe component.
    pub fn catalogue_asset(
        &self,
        provider: &ProviderKey,
        external_id: &str,
        kind: MediaKind,
        extension: &str,
    ) -> Result<PathBuf, MediaPathError> {
        if external_id.is_empty() || external_id.len() > 256 {
            return Err(MediaPathError::InvalidProvider);
        }
        let encoded_id = external_id
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(self
            .catalogue_cache_root()
            .join(provider.as_str())
            .join(encoded_id)
            .join(format!(
                "{}.{}",
                kind.file_stem(),
                valid_extension(extension)?
            )))
    }

    /// Imports a user-selected image into the local library media store.
    ///
    /// The source is copied through a temporary file in the destination directory so
    /// an interrupted copy cannot leave a partially written cover at its final path.
    /// The returned path is deterministic and uses the source image extension.
    pub fn import_library_asset(
        &self,
        game_id: &str,
        kind: MediaKind,
        source: &Path,
    ) -> Result<PathBuf, MediaPathError> {
        if !source.is_file() {
            return Err(MediaPathError::InvalidSource);
        }
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .ok_or(MediaPathError::InvalidExtension)?;
        let destination = self.library_asset(game_id, kind, extension)?;
        self.prepare_asset_path(&destination)?;

        let parent = destination
            .parent()
            .ok_or(MediaPathError::InvalidProvider)?;
        let temporary = parent.join(format!(
            ".{}.import-{}-{}.{}",
            kind.file_stem(),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            extension.trim_start_matches('.').to_ascii_lowercase(),
        ));
        fs::copy(source, &temporary)?;
        let result = replace_file(&temporary, &destination);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;

        // A game has exactly one active local asset per media kind. Remove stale
        // extensions only after the new file has been installed successfully.
        let active_extension = extension.trim_start_matches('.').to_ascii_lowercase();
        for stale_extension in ["png", "jpg", "jpeg", "bmp"] {
            if stale_extension != active_extension {
                if let Ok(stale_path) = self.library_asset(game_id, kind, stale_extension) {
                    let _ = fs::remove_file(stale_path);
                }
            }
        }
        Ok(destination)
    }

    /// Creates parent directories for a path returned by this store.
    pub fn prepare_asset_path(&self, path: &Path) -> Result<(), MediaPathError> {
        if !path.starts_with(&self.root)
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(MediaPathError::InvalidProvider);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

// std::fs::rename replaces an existing destination atomically on supported
// filesystems. The temporary file is created beside the destination, so the
// operation stays on the same volume on Windows as well as Unix.
fn replace_file(source: &Path, destination: &Path) -> Result<(), MediaPathError> {
    fs::rename(source, destination)?;
    Ok(())
}

fn validate_game_id(value: &str) -> Result<(), MediaPathError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(MediaPathError::InvalidProvider);
    }
    Ok(())
}

fn valid_extension(extension: &str) -> Result<&str, MediaPathError> {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Ok("png"),
        "jpg" => Ok("jpg"),
        "jpeg" => Ok("jpeg"),
        "bmp" => Ok("bmp"),
        _ => Err(MediaPathError::InvalidExtension),
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaKind, MediaPathError, MediaStore, ProviderKey};
    use std::path::PathBuf;

    #[test]
    fn library_assets_are_independent_of_install_paths_and_provider_ids() {
        let store = MediaStore::new("amethys-data");
        assert_eq!(
            store
                .library_asset("game-3000", MediaKind::Cover, "png")
                .expect("valid path"),
            PathBuf::from("amethys-data/media/library/game-3000/cover.png")
        );
    }

    #[test]
    fn library_game_ids_are_safe_path_components() {
        let store = MediaStore::new("amethys-data");
        assert!(
            store
                .library_asset("../escape", MediaKind::Cover, "png")
                .is_err()
        );
        assert!(
            store
                .library_asset("game/42", MediaKind::Cover, "png")
                .is_err()
        );
        assert!(
            store
                .library_asset("game-42", MediaKind::Cover, "png")
                .is_ok()
        );
    }

    #[test]
    fn catalogue_external_ids_are_encoded_as_one_safe_component() {
        let store = MediaStore::new("amethys-data");
        let provider = ProviderKey::new("steam").expect("valid provider");
        let path = store
            .catalogue_asset(&provider, "../../game/42", MediaKind::Banner, "png")
            .expect("external IDs are encoded");
        assert!(
            path.to_string_lossy()
                .contains("2e2e2f2e2e2f67616d652f3432")
        );
        assert!(!path.to_string_lossy().contains("../../game"));
    }

    #[test]
    fn invalid_provider_and_extension_are_rejected() {
        assert!(matches!(
            ProviderKey::new("../steam"),
            Err(MediaPathError::InvalidProvider)
        ));
        let store = MediaStore::new("amethys-data");
        let provider = ProviderKey::new("steam").expect("valid provider");
        assert!(matches!(
            store.catalogue_asset(&provider, "123", MediaKind::Cover, "exe"),
            Err(MediaPathError::InvalidExtension)
        ));
    }

    #[test]
    fn importing_library_asset_copies_supported_image_to_game_directory() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "amethys-media-import-{}-{unique}",
            std::process::id()
        ));
        let source = root.join("picked-cover.PNG");
        let store = MediaStore::new(root.join("data"));
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, b"image bytes").unwrap();

        let imported = store
            .import_library_asset("3000", MediaKind::Cover, &source)
            .unwrap();
        assert_eq!(imported, root.join("data/media/library/3000/cover.png"));
        assert_eq!(fs::read(&imported).unwrap(), b"image bytes");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn importing_new_cover_format_removes_stale_extensions() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "amethys-media-replace-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let old_source = root.join("old.jpg");
        let new_source = root.join("new.png");
        fs::write(&old_source, b"old").unwrap();
        fs::write(&new_source, b"new").unwrap();
        let store = MediaStore::new(root.join("data"));

        let old_path = store
            .import_library_asset("3000", MediaKind::Cover, &old_source)
            .unwrap();
        assert!(old_path.is_file());
        let new_path = store
            .import_library_asset("3000", MediaKind::Cover, &new_source)
            .unwrap();

        assert_eq!(fs::read(&new_path).unwrap(), b"new");
        assert!(!old_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn importing_library_asset_rejects_unsupported_extensions_and_missing_sources() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "amethys-media-invalid-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let store = MediaStore::new(root.join("data"));
        let unsupported = root.join("cover.exe");
        fs::write(&unsupported, b"not an image").unwrap();

        assert!(matches!(
            store.import_library_asset("3000", MediaKind::Cover, &unsupported),
            Err(MediaPathError::InvalidExtension)
        ));
        assert!(matches!(
            store.import_library_asset("3000", MediaKind::Cover, &root.join("missing.png")),
            Err(MediaPathError::InvalidSource)
        ));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn media_kinds_have_stable_file_names() {
        assert_eq!(
            MediaKind::ALL.map(MediaKind::file_stem),
            ["icon", "logo", "cover", "banner"]
        );
    }
}
