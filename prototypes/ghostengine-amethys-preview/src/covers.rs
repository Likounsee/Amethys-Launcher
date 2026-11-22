//! Cover art loaded from real image files with GhostEngine's decoder.
//!
//! `assets/covers/<slug>.png|jpg|bmp` is decoded by `ghostengine::decode_texture`
//! (feature `image-decoding`), stored in the engine's `TextureCache` and turned into
//! a `MipmapChain` so downscaled covers are sampled with the engine's mipmapping.
//! Drop a real cover next to the generated placeholders (same file name) to replace it.

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
        Self { cache: TextureCache::new(256 * 1024 * 1024), mips: HashMap::new(), decode_ms: 0.0, files: 0, failed: Vec::new() }
    }

    /// Decodes every supported image in `dir`; missing folders simply give an empty store.
    pub fn load(dir: &Path) -> Self {
        Self::load_with_library(dir, None)
    }

    /// Loads catalogue artwork and optional per-game local cover files.
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
                let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_owned();
                self.load_image(&path, name);
            }
        }
    }

    fn load_image(&mut self, path: &Path, name: String) {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
                continue;
            }
            let decoded = std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| decode_texture(&bytes).map_err(|e| format!("{e:?}")));
            match decoded.and_then(|t| MipmapChain::new(&t).map(|m| (t, m)).map_err(|e| format!("{e:?}"))) {
                Ok((texture, mips)) => {
                    let _ = store.cache.insert(texture);
                    store.mips.insert(name, mips);
                    store.files += 1;
                }
                Err(e) => store.failed.push(format!("{name}: {e}")),
            }
        }
        store.decode_ms = start.elapsed().as_secs_f64() * 1000.0;
        store
    }

    pub fn get(&self, title: &str) -> Option<&MipmapChain> {
        self.mips.get(&slug(title))
    }

    pub fn stats(&self) -> TextureCacheStats {
        self.cache.stats()
    }
}
