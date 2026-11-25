//! Static master-catalogue client.
//!
//! The provider trait deliberately exposes only a base URL. Transport,
//! validation, caching and the published file format are handled here so a
//! future Amethys-hosted provider can replace GitHub Pages without changing
//! Store/Core consumers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use url::Url;

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_COMPRESSED_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECOMPRESSED_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum RemoteCatalogError {
    #[error("catalogue URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("catalogue network request failed: {0}")]
    Network(String),
    #[error("catalogue response is invalid: {0}")]
    InvalidData(String),
    #[error("catalogue cache I/O failed: {0}")]
    Cache(String),
}

pub type RemoteCatalogResult<T> = Result<T, RemoteCatalogError>;

/// Provider abstraction: consumers do not know which static host serves files.
pub trait CatalogProvider: Send + Sync {
    fn base_url(&self) -> &Url;
}

#[derive(Debug, Clone)]
pub struct StaticCatalogProvider {
    base_url: Url,
}

impl StaticCatalogProvider {
    pub fn new(base_url: &str) -> RemoteCatalogResult<Self> {
        let mut parsed = Url::parse(base_url)
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(RemoteCatalogError::InvalidUrl(
                "provider base URL must be HTTPS and contain no credentials".into(),
            ));
        }
        if !parsed.path().ends_with('/') {
            let path = format!("{}/", parsed.path());
            parsed.set_path(&path);
        }
        Ok(Self { base_url: parsed })
    }
}

impl CatalogProvider for StaticCatalogProvider {
    fn base_url(&self) -> &Url {
        &self.base_url
    }
}

/// Initial deployment adapter. No GitHub-specific behavior exists outside this
/// default URL; the same client works with any HTTPS static file host.
pub type GitHubPagesCatalogProvider = StaticCatalogProvider;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogManifest {
    pub manifest_schema_version: u32,
    pub catalog_schema_version: u32,
    pub catalog_version: String,
    pub generated_at: String,
    pub source: String,
    pub platform: String,
    pub game_count: u64,
    pub store_identity_count: u64,
    #[serde(default)]
    pub source_identity_count: u64,
    pub search_index: CatalogFileDescriptor,
    pub shards: CatalogShardSet,
    pub metadata: CatalogMetadataSet,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogFileDescriptor {
    pub file: String,
    pub compressed_size: Option<u64>,
    pub compressed_sha256: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogShardSet {
    pub scheme: String,
    pub bucket_size: u64,
    pub path_template: String,
    pub game_count: u64,
    pub files: Vec<CatalogShardDescriptor>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogShardDescriptor {
    pub file: String,
    pub bucket: u64,
    pub game_count: u64,
    pub compressed_size: u64,
    pub compressed_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogMetadataSet {
    pub genres: CatalogFileDescriptor,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CatalogSearchEntry {
    pub amethys_id: u64,
    pub name: String,
    pub slug: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Optional artwork URL carried by newer public search indexes.
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CatalogSourceIdentity {
    pub source: String,
    pub external_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CatalogStoreIdentity {
    pub store: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CatalogGameDetail {
    pub amethys_id: u64,
    /// Optional ingestion-source identity; never the canonical Amethys ID.
    #[serde(default)]
    pub igdb_id: Option<u64>,
    pub name: String,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<CatalogSourceIdentity>,
    #[serde(default)]
    pub stores: Vec<CatalogStoreIdentity>,
}

/// Normalizes punctuation and casing so minor title-format differences do not
/// prevent a local game from contributing to the player's taste profile.
fn normalize_game_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub struct RemoteCatalogClient<P> {
    provider: P,
    http: reqwest::Client,
    cache_dir: PathBuf,
}

impl<P: CatalogProvider> RemoteCatalogClient<P> {
    pub fn new(provider: P, cache_dir: impl Into<PathBuf>) -> RemoteCatalogResult<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .user_agent(concat!("Amethys-Launcher/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?;
        Ok(Self {
            provider,
            http,
            cache_dir: cache_dir.into(),
        })
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub async fn fetch_manifest(&self) -> RemoteCatalogResult<CatalogManifest> {
        let url = self
            .provider
            .base_url()
            .join("manifest.json")
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))?;
        let bytes = self.fetch_bytes(&url, MAX_MANIFEST_BYTES, None).await?;
        let manifest: CatalogManifest = serde_json::from_slice(&bytes)
            .map_err(|error| RemoteCatalogError::InvalidData(format!("manifest JSON: {error}")))?;
        validate_manifest(&manifest)?;
        atomic_write(&self.cache_dir.join("manifest.json"), &bytes)?;
        Ok(manifest)
    }

    /// Loads the last validated manifest when the catalogue host is unavailable.
    pub fn load_cached_manifest(&self) -> RemoteCatalogResult<CatalogManifest> {
        let path = self.cache_dir.join("manifest.json");
        let bytes = fs::read(path).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
        let manifest: CatalogManifest = serde_json::from_slice(&bytes).map_err(|error| {
            RemoteCatalogError::InvalidData(format!("cached manifest JSON: {error}"))
        })?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }

    /// Downloads and verifies the compact index, then persists its decoded JSON
    /// representation for fast offline search. Returns the parsed index.
    pub async fn synchronize_index(
        &self,
        manifest: &CatalogManifest,
    ) -> RemoteCatalogResult<Vec<CatalogSearchEntry>> {
        validate_manifest(manifest)?;
        let descriptor = &manifest.search_index;
        let url = self.file_url(&descriptor.file)?;
        let compressed = self
            .fetch_bytes(
                &url,
                MAX_COMPRESSED_FILE_BYTES,
                descriptor.compressed_sha256.as_deref(),
            )
            .await?;
        verify_size(descriptor.compressed_size, compressed.len())?;
        let decoded = decompress_bounded(&compressed)?;
        // Accept legacy three-field rows, genre-aware four-field rows and
        // artwork-aware five-field rows so published catalogues stay readable.
        let rows: Vec<serde_json::Value> = serde_json::from_slice(&decoded).map_err(|error| {
            RemoteCatalogError::InvalidData(format!("search index JSON: {error}"))
        })?;
        if rows.len() as u64 != manifest.game_count {
            return Err(RemoteCatalogError::InvalidData(format!(
                "search index has {} rows; manifest declares {}",
                rows.len(),
                manifest.game_count
            )));
        }
        let mut previous_id = 0;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let fields = row.as_array().ok_or_else(|| {
                RemoteCatalogError::InvalidData("search index rows must be arrays".into())
            })?;
            if !(3..=5).contains(&fields.len()) {
                return Err(RemoteCatalogError::InvalidData(
                    "search index row has an unsupported field count".into(),
                ));
            }
            let id = fields[0].as_u64().ok_or_else(|| {
                RemoteCatalogError::InvalidData("search index ID is invalid".into())
            })?;
            let name = fields[1]
                .as_str()
                .ok_or_else(|| {
                    RemoteCatalogError::InvalidData("search index name is invalid".into())
                })?
                .to_owned();
            let slug = if fields[2].is_null() {
                None
            } else {
                Some(
                    fields[2]
                        .as_str()
                        .ok_or_else(|| {
                            RemoteCatalogError::InvalidData("search index slug is invalid".into())
                        })?
                        .to_owned(),
                )
            };
            let genres = fields
                .get(3)
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let cover_url = fields
                .get(4)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned);
            if id == 0 || id <= previous_id || name.trim().is_empty() || name.len() > 512 {
                return Err(RemoteCatalogError::InvalidData(
                    "search index IDs must be unique, ascending and nonzero; names must be valid"
                        .into(),
                ));
            }
            previous_id = id;
            entries.push(CatalogSearchEntry {
                amethys_id: id,
                name,
                slug,
                genres,
                cover_url,
            });
        }
        let cache_path = self.cache_dir.join("index").join("games-index.json");
        let json = serde_json::to_vec(&entries)
            .map_err(|error| RemoteCatalogError::InvalidData(error.to_string()))?;
        atomic_write(&cache_path, &json)?;
        Ok(entries)
    }

    /// Loads the offline search index if a previous verified sync exists.
    pub fn load_cached_index(&self) -> RemoteCatalogResult<Vec<CatalogSearchEntry>> {
        let path = self.cache_dir.join("index").join("games-index.json");
        let bytes = fs::read(path).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| RemoteCatalogError::InvalidData(format!("cached index JSON: {error}")))
    }

    pub fn search_cached(
        &self,
        entries: &[CatalogSearchEntry],
        query: &str,
        limit: usize,
    ) -> RemoteCatalogResult<Vec<CatalogSearchEntry>> {
        if query.len() > 256 || limit == 0 || limit > MAX_SEARCH_RESULTS {
            return Err(RemoteCatalogError::InvalidData(
                "search query or result limit is outside allowed bounds".into(),
            ));
        }
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            let mut results = entries.to_vec();
            // Do not let stable internal IDs dictate the first page of the Store.
            results.sort_by_cached_key(|entry| entry.name.to_lowercase());
            results.truncate(limit);
            return Ok(results);
        }

        let normalized_needle = normalize_game_title(&needle);
        let mut ranked = entries
            .iter()
            .filter_map(|entry| {
                let name = entry.name.to_lowercase();
                let normalized_name = normalize_game_title(&entry.name);
                let slug = entry.slug.as_deref().unwrap_or_default().to_lowercase();
                let normalized_slug = normalize_game_title(&slug);
                let genre_match = entry.genres.iter().any(|genre| {
                    genre.to_lowercase().contains(&needle)
                        || normalize_game_title(genre).contains(&normalized_needle)
                });

                let rank = if name == needle || normalized_name == normalized_needle {
                    0
                } else if name.starts_with(&needle)
                    || (!normalized_needle.is_empty()
                        && normalized_name.starts_with(&normalized_needle))
                {
                    1
                } else if name.contains(&needle)
                    || (!normalized_needle.is_empty()
                        && normalized_name.contains(&normalized_needle))
                {
                    2
                } else if slug.contains(&needle)
                    || (!normalized_needle.is_empty()
                        && normalized_slug.contains(&normalized_needle))
                {
                    3
                } else if genre_match {
                    4
                } else {
                    return None;
                };
                Some((rank, name, entry.amethys_id, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|(_, _, _, entry)| entry.clone())
            .collect())
    }

    /// Recommends games against the genres and ownership already known locally.
    /// The full cached index is ranked before truncation, so recommendations are
    /// not biased toward the first Amethys IDs in the catalogue.
    pub fn recommend_cached(
        &self,
        entries: &[CatalogSearchEntry],
        owned_genres: &[String],
        owned_ids: &[u64],
        owned_names: &[String],
        limit: usize,
    ) -> RemoteCatalogResult<Vec<CatalogSearchEntry>> {
        if limit == 0
            || limit > 100
            || owned_genres.len() > 512
            || owned_ids.len() > 10_000
            || owned_names.len() > 10_000
            || owned_genres
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 128)
            || owned_names
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err(RemoteCatalogError::InvalidData(
                "recommendation profile or result limit is outside allowed bounds".into(),
            ));
        }

        let mut genre_weights = std::collections::HashMap::<String, usize>::new();
        for genre in owned_genres {
            *genre_weights
                .entry(genre.trim().to_lowercase())
                .or_default() += 1;
        }
        let owned_ids = owned_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let owned_names = owned_names
            .iter()
            .map(|name| normalize_game_title(name))
            .collect::<std::collections::HashSet<_>>();

        // A locally installed game may not have been linked to an Amethys ID yet.
        // Exact title matches let the catalogue itself supply its genres anyway.
        for entry in entries.iter().filter(|entry| {
            owned_ids.contains(&entry.amethys_id)
                || owned_names.contains(&normalize_game_title(&entry.name))
        }) {
            for genre in &entry.genres {
                *genre_weights
                    .entry(genre.trim().to_lowercase())
                    .or_default() += 1;
            }
        }

        let mut ranked = entries
            .iter()
            .filter(|entry| {
                !owned_ids.contains(&entry.amethys_id)
                    && !owned_names.contains(&normalize_game_title(&entry.name))
            })
            .map(|entry| {
                let mut seen = std::collections::HashSet::new();
                let score = entry
                    .genres
                    .iter()
                    .map(|genre| genre.trim().to_lowercase())
                    .filter(|genre| !genre.is_empty() && seen.insert(genre.clone()))
                    .map(|genre| genre_weights.get(&genre).copied().unwrap_or(0))
                    .sum::<usize>();
                (score, entry)
            })
            .collect::<Vec<_>>();
        // Keep the ranking deterministic when multiple games share the same
        // genre score. A zero-score catalogue must not look personalized.
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.name.to_lowercase().cmp(&right.1.name.to_lowercase()))
                .then_with(|| left.1.amethys_id.cmp(&right.1.amethys_id))
        });
        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry.clone())
            .collect())
    }

    /// Downloads and caches a catalogue cover from the current IGDB image host.
    /// Only HTTPS images.igdb.com URLs and bounded JPEG/PNG payloads are accepted.
    pub async fn cache_cover(
        &self,
        amethys_id: u64,
        cover_url: &str,
    ) -> RemoteCatalogResult<Option<PathBuf>> {
        const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;
        if amethys_id == 0 {
            return Err(RemoteCatalogError::InvalidData(
                "Amethys ID cannot be zero".into(),
            ));
        }
        let url = Url::parse(cover_url)
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))?;
        if url.scheme() != "https"
            || url.host_str() != Some("images.igdb.com")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(RemoteCatalogError::InvalidUrl(
                "cover URL must use the trusted HTTPS IGDB image host".into(),
            ));
        }

        let directory = self.cache_dir.join("covers");
        let cached = ["jpg", "png"]
            .iter()
            .map(|extension| directory.join(format!("{amethys_id}.{extension}")))
            .find(|path| path.is_file());
        if let Some(path) = cached {
            let bytes =
                fs::read(&path).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
            if bytes.len() <= MAX_COVER_BYTES && image_extension(&bytes).is_some() {
                return Ok(Some(path));
            }
            fs::remove_file(&path).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
        }

        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?;
        if response.url().scheme() != "https"
            || response.url().host_str() != Some("images.igdb.com")
        {
            return Err(RemoteCatalogError::InvalidUrl(
                "cover redirects must remain on the trusted HTTPS image host".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_COVER_BYTES as u64)
        {
            return Err(RemoteCatalogError::InvalidData(
                "cover exceeds size limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_COVER_BYTES {
                return Err(RemoteCatalogError::InvalidData(
                    "cover exceeds size limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let extension = image_extension(&bytes).ok_or_else(|| {
            RemoteCatalogError::InvalidData("cover is not a supported JPEG or PNG image".into())
        })?;
        let path = directory.join(format!("{amethys_id}.{extension}"));
        atomic_write(&path, &bytes)?;
        Ok(Some(path))
    }

    /// Downloads a single ID-range shard, verifies its compressed checksum and
    /// caches the decoded details. Other shards are never fetched implicitly.
    pub async fn fetch_game(
        &self,
        manifest: &CatalogManifest,
        amethys_id: u64,
    ) -> RemoteCatalogResult<CatalogGameDetail> {
        validate_manifest(manifest)?;
        if amethys_id == 0 {
            return Err(RemoteCatalogError::InvalidData(
                "Amethys ID cannot be zero".into(),
            ));
        }
        let bucket = amethys_id / manifest.shards.bucket_size;
        let descriptor = manifest
            .shards
            .files
            .iter()
            .find(|item| item.bucket == bucket)
            .ok_or_else(|| {
                RemoteCatalogError::InvalidData(format!("no shard for Amethys ID {amethys_id}"))
            })?;
        let cache_path = self.cache_dir.join("shards").join(format!(
            "{:06}-{}.json",
            bucket,
            &descriptor.compressed_sha256[..12]
        ));
        let records: Vec<CatalogGameDetail> = if cache_path.is_file() {
            let bytes = fs::read(&cache_path)
                .map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
            serde_json::from_slice(&bytes).map_err(|error| {
                RemoteCatalogError::InvalidData(format!("cached shard JSON: {error}"))
            })?
        } else {
            let url = self.file_url(&descriptor.file)?;
            let compressed = self
                .fetch_bytes(
                    &url,
                    MAX_COMPRESSED_FILE_BYTES,
                    Some(&descriptor.compressed_sha256),
                )
                .await?;
            verify_size(Some(descriptor.compressed_size), compressed.len())?;
            let decoded = decompress_bounded(&compressed)?;
            let records: Vec<CatalogGameDetail> =
                serde_json::from_slice(&decoded).map_err(|error| {
                    RemoteCatalogError::InvalidData(format!("detail shard JSON: {error}"))
                })?;
            let json = serde_json::to_vec(&records)
                .map_err(|error| RemoteCatalogError::InvalidData(error.to_string()))?;
            atomic_write(&cache_path, &json)?;
            records
        };
        if records.len() as u64 != descriptor.game_count {
            return Err(RemoteCatalogError::InvalidData(
                "detail shard record count mismatch".into(),
            ));
        }
        let mut previous_id = 0;
        for record in &records {
            if record.amethys_id == 0
                || record.amethys_id / manifest.shards.bucket_size != bucket
                || record.amethys_id <= previous_id
                || record.name.trim().is_empty()
            {
                return Err(RemoteCatalogError::InvalidData(
                    "detail shard contains an invalid or unordered game identity".into(),
                ));
            }
            previous_id = record.amethys_id;
        }
        records
            .into_iter()
            .find(|game| game.amethys_id == amethys_id)
            .ok_or_else(|| {
                RemoteCatalogError::InvalidData(format!(
                    "game {amethys_id} is absent from its shard"
                ))
            })
    }

    fn file_url(&self, relative: &str) -> RemoteCatalogResult<Url> {
        validate_relative_path(relative)?;
        self.provider
            .base_url()
            .join(relative)
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))
    }

    async fn fetch_bytes(
        &self,
        url: &Url,
        max_bytes: usize,
        expected_sha256: Option<&str>,
    ) -> RemoteCatalogResult<Vec<u8>> {
        if url.scheme() != "https" {
            return Err(RemoteCatalogError::InvalidUrl(
                "only HTTPS catalogue files are accepted".into(),
            ));
        }
        let mut response = self
            .http
            .get(url.clone())
            .send()
            .await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?;
        if response.url().scheme() != "https" {
            return Err(RemoteCatalogError::InvalidUrl(
                "catalogue redirects must remain HTTPS".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(RemoteCatalogError::InvalidData(
                "catalogue response exceeds size limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
        {
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(RemoteCatalogError::InvalidData(
                    "catalogue response exceeds size limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(&bytes);
            if !constant_time_eq(expected, &actual) {
                return Err(RemoteCatalogError::InvalidData(
                    "catalogue SHA-256 mismatch".into(),
                ));
            }
        }
        Ok(bytes)
    }
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else {
        None
    }
}

fn validate_manifest(manifest: &CatalogManifest) -> RemoteCatalogResult<()> {
    if manifest.manifest_schema_version != 2 || manifest.catalog_schema_version != 1 {
        return Err(RemoteCatalogError::InvalidData(
            "unsupported catalogue manifest/schema version".into(),
        ));
    }
    if manifest.catalog_version.trim().is_empty()
        || manifest.game_count == 0
        || manifest.shards.bucket_size == 0
        || manifest.shards.game_count != manifest.game_count
    {
        return Err(RemoteCatalogError::InvalidData(
            "manifest counters or version are invalid".into(),
        ));
    }
    validate_relative_path(&manifest.search_index.file)?;
    if manifest
        .search_index
        .compressed_sha256
        .as_deref()
        .is_none_or(|hash| !valid_sha256(hash))
    {
        return Err(RemoteCatalogError::InvalidData(
            "search-index checksum is missing or invalid".into(),
        ));
    }
    if manifest.shards.files.len() > 100_000 {
        return Err(RemoteCatalogError::InvalidData(
            "manifest contains too many shards".into(),
        ));
    }
    let mut buckets = std::collections::BTreeSet::new();
    for shard in &manifest.shards.files {
        validate_relative_path(&shard.file)?;
        if !buckets.insert(shard.bucket)
            || shard.game_count == 0
            || !valid_sha256(&shard.compressed_sha256)
        {
            return Err(RemoteCatalogError::InvalidData(
                "manifest contains an invalid shard descriptor".into(),
            ));
        }
    }
    validate_relative_path(&manifest.metadata.genres.file)?;
    Ok(())
}

fn validate_relative_path(value: &str) -> RemoteCatalogResult<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 512
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(RemoteCatalogError::InvalidData(
            "catalogue file path is not a safe relative path".into(),
        ));
    }
    Ok(())
}

fn decompress_bounded(compressed: &[u8]) -> RemoteCatalogResult<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(compressed)
        .map_err(|error| RemoteCatalogError::InvalidData(format!("Zstandard stream: {error}")))?;
    let mut limited = decoder.take(MAX_DECOMPRESSED_FILE_BYTES as u64 + 1);
    let mut output = Vec::new();
    limited.read_to_end(&mut output).map_err(|error| {
        RemoteCatalogError::InvalidData(format!("Zstandard decompression: {error}"))
    })?;
    if output.len() > MAX_DECOMPRESSED_FILE_BYTES {
        return Err(RemoteCatalogError::InvalidData(
            "decompressed catalogue file exceeds size limit".into(),
        ));
    }
    Ok(output)
}

fn verify_size(expected: Option<u64>, actual: usize) -> RemoteCatalogResult<()> {
    if expected.is_some_and(|size| size != actual as u64) {
        return Err(RemoteCatalogError::InvalidData(
            "catalogue compressed-size mismatch".into(),
        ));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> RemoteCatalogResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| RemoteCatalogError::Cache("cache path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::File::create(&temporary)
        .map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
    #[cfg(windows)]
    if path.exists() {
        let backup = path.with_extension(format!("old-{}", std::process::id()));
        fs::rename(path, &backup).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            RemoteCatalogError::Cache(error.to_string())
        })?;
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::rename(&backup, path);
            let _ = fs::remove_file(&temporary);
            return Err(RemoteCatalogError::Cache(error.to_string()));
        }
        let _ = fs::remove_file(backup);
        return Ok(());
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        RemoteCatalogError::Cache(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_requires_https_and_normalizes_base_path() {
        assert!(StaticCatalogProvider::new("http://example.com/catalog").is_err());
        let provider = StaticCatalogProvider::new("https://example.com/catalog").unwrap();
        assert_eq!(provider.base_url().as_str(), "https://example.com/catalog/");
    }

    #[test]
    fn rejects_path_traversal_and_absolute_paths() {
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("/manifest.json").is_err());
        assert!(validate_relative_path("shards/../../secret").is_err());
        assert!(validate_relative_path("shards/000001.json.zst").is_ok());
    }

    #[test]
    fn manifest_schema_matches_published_catalogue_contract() {
        let manifest: CatalogManifest = serde_json::from_str(r#"{
            "manifest_schema_version":2,"catalog_schema_version":1,
            "catalog_version":"2026-11-24T13:00:00Z","generated_at":"2026-11-24T13:00:00Z",
            "source":"IGDB","platform":"windows-pc","game_count":1,"store_identity_count":1,
            "search_index":{"file":"index/games-index.json.zst","records":1,"compressed_size":10,
                "compressed_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "format":"json-array-zstd","record_fields":["amethys_id","name","slug"]},
            "shards":{"scheme":"amethys-id-range-v1","bucket_size":1000,
                "path_template":"shards/{bucket:06d}.json.zst","game_count":1,
                "files":[{"file":"shards/001000.json.zst","records":1,"compressed_size":10,
                    "compressed_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "bucket":1000,"game_count":1}]},
            "metadata":{"genres":{"file":"metadata/genres.json","size":10,
                "sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}}
        }"#).unwrap();
        validate_manifest(&manifest).unwrap();
        assert_eq!(manifest.game_count, 1);
    }

    #[tokio::test]
    async fn cached_detail_shard_rejects_unsorted_or_duplicate_ids() {
        let directory = tempfile::tempdir().unwrap();
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            directory.path(),
        )
        .unwrap();
        let manifest: CatalogManifest = serde_json::from_str(r#"{
            "manifest_schema_version":2,"catalog_schema_version":1,
            "catalog_version":"2026-11-24T13:00:00Z","generated_at":"2026-11-24T13:00:00Z",
            "source":"Amethys","platform":"windows-pc","game_count":2,"store_identity_count":0,
            "search_index":{"file":"index/games-index.json.zst","compressed_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "shards":{"scheme":"amethys-id-range-v1","bucket_size":1000,
                "path_template":"shards/{bucket:06d}.json.zst","game_count":2,
                "files":[{"file":"shards/000001.json.zst","records":2,"compressed_size":10,
                    "compressed_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "bucket":1,"game_count":2}]},
            "metadata":{"genres":{"file":"metadata/genres.json"}}
        }"#).unwrap();
        let cache_path = directory
            .path()
            .join("shards")
            .join("000001-bbbbbbbbbbbb.json");
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(
            &cache_path,
            r#"[
                {"amethys_id":1002,"name":"Second","slug":null,"summary":null,"release_date":null,"cover_url":null},
                {"amethys_id":1001,"name":"First","slug":null,"summary":null,"release_date":null,"cover_url":null}
            ]"#,
        )
        .unwrap();

        let error = client.fetch_game(&manifest, 1001).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid or unordered game identity")
        );
    }

    #[test]
    fn cached_manifest_is_validated_before_offline_use() {
        let directory = tempfile::tempdir().unwrap();
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            directory.path(),
        )
        .unwrap();
        let manifest = r#"{
            "manifest_schema_version":2,"catalog_schema_version":1,
            "catalog_version":"2026-11-24T13:00:00Z","generated_at":"2026-11-24T13:00:00Z",
            "source":"Amethys","platform":"windows-pc","game_count":1,"store_identity_count":0,
            "search_index":{"file":"index/games-index.json.zst","records":1,"compressed_size":10,
                "compressed_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "format":"json-array-zstd","record_fields":["amethys_id","name","slug"]},
            "shards":{"scheme":"amethys-id-range-v1","bucket_size":1000,
                "path_template":"shards/{bucket:06d}.json.zst","game_count":1,
                "files":[{"file":"shards/001000.json.zst","records":1,"compressed_size":10,
                    "compressed_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "bucket":1000,"game_count":1}]},
            "metadata":{"genres":{"file":"metadata/genres.json","size":10,
                "sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}}
        }"#;
        let cache_path = directory.path().join("manifest.json");
        fs::write(&cache_path, manifest).unwrap();

        let cached = client.load_cached_manifest().unwrap();
        assert_eq!(cached.catalog_version, "2026-11-24T13:00:00Z");
        assert_eq!(cached.game_count, 1);

        fs::write(&cache_path, r#"{"catalog_version":"unvalidated"}"#).unwrap();
        assert!(client.load_cached_manifest().is_err());
    }

    #[test]
    fn search_matches_normalized_titles_slugs_and_genres_with_relevance_order() {
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            std::env::temp_dir().join("amethys-catalog-search-ranking-test"),
        )
        .unwrap();
        let entries = vec![
            CatalogSearchEntry {
                amethys_id: 30,
                name: "A Game".into(),
                slug: Some("clair-obscur-expedition-33".into()),
                genres: vec!["RPG".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 20,
                name: "Clair Obscur: Expedition 33".into(),
                slug: Some("expedition-33".into()),
                genres: vec!["Adventure".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 10,
                name: "Another Title".into(),
                slug: None,
                genres: vec!["Action RPG".into()],
                cover_url: None,
            },
        ];

        let title_results = client.search_cached(&entries, "clair obscur", 10).unwrap();
        assert_eq!(
            title_results
                .iter()
                .map(|entry| entry.amethys_id)
                .collect::<Vec<_>>(),
            vec![20, 30]
        );

        let slug_results = client.search_cached(&entries, "expedition 33", 10).unwrap();
        assert_eq!(
            slug_results
                .iter()
                .map(|entry| entry.amethys_id)
                .collect::<Vec<_>>(),
            vec![20, 30]
        );

        let genre_results = client.search_cached(&entries, "action", 10).unwrap();
        assert_eq!(
            genre_results
                .iter()
                .map(|entry| entry.amethys_id)
                .collect::<Vec<_>>(),
            vec![10]
        );
    }

    #[test]
    fn search_index_tuple_decodes_with_optional_slug() {
        let rows: Vec<(u64, String, Option<String>)> =
            serde_json::from_str(r#"[[1001,"Example", "example"],[1002,"No slug",null]]"#).unwrap();
        assert_eq!(rows[1].2, None);
    }

    #[test]
    fn recommendations_prioritize_library_genres_and_exclude_owned_games() {
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            std::env::temp_dir().join("amethys-recommendation-test"),
        )
        .unwrap();
        let entries = vec![
            CatalogSearchEntry {
                amethys_id: 1,
                name: "Owned".into(),
                slug: None,
                genres: vec!["Action".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 2,
                name: "Puzzle".into(),
                slug: None,
                genres: vec!["Puzzle".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 3,
                name: "Action Adventure".into(),
                slug: None,
                genres: vec!["Action".into(), "Adventure".into()],
                cover_url: None,
            },
        ];
        let results = client
            .recommend_cached(
                &entries,
                &["Action".into(), "Adventure".into()],
                &[1],
                &["Owned".into()],
                10,
            )
            .unwrap();
        assert_eq!(
            results
                .iter()
                .map(|game| game.amethys_id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
    }

    #[test]
    fn recommendation_matches_owned_titles_ignoring_punctuation_and_case() {
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            std::env::temp_dir().join("amethys-normalized-recommendation-test"),
        )
        .unwrap();
        let entries = vec![
            CatalogSearchEntry {
                amethys_id: 10,
                name: "Clair-Obscur Expedition 33".into(),
                slug: None,
                genres: vec!["RPG".into(), "Adventure".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 11,
                name: "Another RPG".into(),
                slug: None,
                genres: vec!["RPG".into()],
                cover_url: None,
            },
            CatalogSearchEntry {
                amethys_id: 12,
                name: "Puzzle World".into(),
                slug: None,
                genres: vec!["Puzzle".into()],
                cover_url: None,
            },
        ];
        let results = client
            .recommend_cached(
                &entries,
                &[],
                &[],
                &["clair obscur: expedition 33".into()],
                10,
            )
            .unwrap();
        assert_eq!(
            results
                .iter()
                .map(|game| game.amethys_id)
                .collect::<Vec<_>>(),
            vec![11, 12],
        );
    }

    #[test]
    fn master_catalogue_details_do_not_require_an_igdb_identity() {
        let detail: CatalogGameDetail = serde_json::from_str(
            r#"{
                "amethys_id": 1001,
                "name": "Provider-independent game",
                "slug": "provider-independent-game",
                "summary": null,
                "release_date": null,
                "cover_url": null,
                "genres": ["Adventure"],
                "stores": []
            }"#,
        )
        .unwrap();

        assert_eq!(detail.amethys_id, 1001);
        assert_eq!(detail.igdb_id, None);
        assert!(detail.stores.is_empty());
    }

    #[test]
    fn cover_image_signature_accepts_only_jpeg_and_png() {
        assert_eq!(image_extension(&[0xff, 0xd8, 0xff, 0x00]), Some("jpg"));
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_extension(b"<html>not an image</html>"), None);
        assert_eq!(image_extension(&[]), None);
    }

    #[tokio::test]
    async fn cover_cache_rejects_untrusted_hosts_before_network_access() {
        let directory = tempfile::tempdir().unwrap();
        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            directory.path(),
        )
        .unwrap();

        let error = client
            .cache_cover(1001, "https://attacker.example/cover.jpg")
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteCatalogError::InvalidUrl(_)));

        let error = client
            .cache_cover(1001, "http://images.igdb.com/cover.jpg")
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteCatalogError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn cover_cache_reuses_a_valid_cached_png_without_network_access() {
        let directory = tempfile::tempdir().unwrap();
        let cover_dir = directory.path().join("covers");
        std::fs::create_dir_all(&cover_dir).unwrap();
        let cached = cover_dir.join("1001.png");
        std::fs::write(&cached, b"\x89PNG\r\n\x1a\nvalid-test-image").unwrap();

        let client = RemoteCatalogClient::new(
            StaticCatalogProvider::new("https://example.com/catalog").unwrap(),
            directory.path(),
        )
        .unwrap();

        let result = client
            .cache_cover(
                1001,
                "https://images.igdb.com/igdb/image/upload/t_cover_big/test.jpg",
            )
            .await
            .unwrap();

        assert_eq!(result.as_deref(), Some(cached.as_path()));
    }

    #[test]
    fn rejects_non_sha256_hashes() {
        assert!(!valid_sha256("not-a-hash"));
        assert!(valid_sha256(&"a".repeat(64)));
    }
}
