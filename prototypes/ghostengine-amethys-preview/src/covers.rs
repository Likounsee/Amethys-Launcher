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
        let ext = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
            return;
        }
        let decoded = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| decode_texture(&bytes).map_err(|error| format!("{error:?}")));
        match decoded.and_then(|texture| {
            MipmapChain::new(&texture)
                .map(|mips| (texture, mips))
                .map_err(|error| format!("{error:?}"))
        }) {
            Ok((texture, mips)) => {
                let _ = self.cache.insert(texture);
                self.mips.insert(name, mips);
                self.files += 1;
            }
            Err(error) => self.failed.push(format!("{name}: {error}")),
        }
    }

    pub fn get(&self, title: &str) -> Option<&MipmapChain> {
        self.mips.get(&slug(title))
    }

    pub fn get_for_game(&self, game_id: &str, title: &str) -> Option<&MipmapChain> {
        self.mips.get(game_id).or_else(|| self.get(title))
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
