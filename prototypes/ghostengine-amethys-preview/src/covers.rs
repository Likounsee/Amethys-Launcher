//! Cover art loaded from generated placeholders and user-owned local files.

use ghostengine::{decode_texture, MipmapChain, TextureCache, TextureCacheStats};
use std::{collections::HashMap, path::Path, time::Instant};

/// "Baldur's Gate 3" -> "baldurs-gate-3"
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().filter(|c| *c != '\'') {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub struct CoverStore {
    cache: TextureCache,
    mips: HashMap<String, MipmapChain>,
    pub decode_ms: f64,
    pub files: usize,
    pub failed: Vec<String>,
}

impl CoverStore {
    pub fn empty() -> Self {
        Self {
            cache: TextureCache::new(256 * 1024 * 1024),
            mips: HashMap::new(),
            decode_ms: 0.0,
            files: 0,
            failed: Vec::new(),
        }
    }

    pub fn load(dir: &Path) -> Self {
        Self::load_with_library(dir, None)
    }

    /// Loads bundled placeholders and optional per-game local cover files.
    /// Local files live at media/library/<game-id>/cover.<extension>.
    pub fn load_with_library(catalogue_dir: &Path, library_dir: Option<&Path>) -> Self {
        let mut store = Self::empty();
        let start = Instant::now();
        store.load_directory(catalogue_dir, false);
        if let Some(library_dir) = library_dir {
            store.load_directory(library_dir, true);
        }
        store.decode_ms = start.elapsed().as_secs_f64() * 1000.0;
        store
    }

    fn load_directory(&mut self, dir: &Path, library_assets: bool) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if library_assets {
                if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    continue;
                }
                let game_id = entry.file_name().to_string_lossy().into_owned();
                let Some(asset) = ["png", "jpg", "jpeg", "bmp"]
                    .iter()
                    .map(|extension| path.join(format!("cover.{extension}")))
                    .find(|candidate| candidate.is_file())
                else {
                    continue;
                };
                self.load_image(&asset, game_id);
            } else {
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
                self.load_image(&path, name);
            }
        }
    }

    fn load_image(&mut self, path: &Path, name: String) {
        let result = self.load_decoded_image(path, &name);
        if let Err(error) = result {
            self.failed.push(format!("{name}: {error}"));
        }
    }

    /// Validates an image before it is copied into the user's media directory.
    pub fn validate_image(&self, path: &Path) -> Result<(), String> {
        let ext = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
        if !matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp") {
            return Err("format non pris en charge (PNG, JPEG ou BMP uniquement)".to_owned());
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let texture = decode_texture(&bytes).map_err(|error| format!("image illisible : {error:?}"))?;
        MipmapChain::new(&texture).map_err(|error| format!("image invalide : {error:?}"))?;
        Ok(())
    }

    /// Loads or replaces a custom cover after it has been imported into local media.
    /// The texture and mipmap chain are refreshed immediately, without restarting the UI.
    pub fn load_library_cover(&mut self, game_id: &str, path: &Path) -> Result<(), String> {
        self.load_decoded_image(path, game_id)
    }

    /// Removes a user cover and restores the bundled/generated placeholder.
    /// Only the four known cover filenames inside this game's media directory are touched.
    pub fn reset_library_cover(&mut self, game_id: &str, library_dir: &Path) -> Result<bool, String> {
        if game_id.is_empty()
            || game_id.len() > 128
            || !game_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("identifiant de jeu invalide".to_owned());
        }

        let game_dir = library_dir.join(game_id);
        let mut removed = false;
        for extension in ["png", "jpg", "jpeg", "bmp"] {
            let path = game_dir.join(format!("cover.{extension}"));
            match std::fs::remove_file(&path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{} : {error}", path.display())),
            }
        }

        if removed {
            self.mips.remove(game_id);
            self.files = self.files.saturating_sub(1);
            if std::fs::read_dir(&game_dir).map(|mut entries| entries.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&game_dir);
            }
        }
        Ok(removed)
    }

    fn load_decoded_image(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let ext = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
            return Err("unsupported image extension".to_owned());
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let texture = decode_texture(&bytes).map_err(|error| format!("{error:?}"))?;
        let mips = MipmapChain::new(&texture).map_err(|error| format!("{error:?}"))?;
        let _ = self.cache.insert(texture);
        if self.mips.insert(name.to_owned(), mips).is_none() {
            self.files += 1;
        }
        Ok(())
    }

    pub fn get(&self, title: &str) -> Option<&MipmapChain> {
        self.mips.get(&slug(title))
    }

    pub fn get_for_game(
        &self,
        catalogue_game_id: &str,
        installation_id: &str,
        title: &str,
    ) -> Option<&MipmapChain> {
        self.mips
            .get(catalogue_game_id)
            .or_else(|| self.mips.get(installation_id))
            .or_else(|| self.get(title))
    }

    pub fn stats(&self) -> TextureCacheStats {
        self.cache.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn title_slug_is_stable() {
        assert_eq!(slug("Baldur's Gate 3"), "baldurs-gate-3");
        assert_eq!(slug("Pokémon Violet"), "pok-mon-violet");
    }
}
