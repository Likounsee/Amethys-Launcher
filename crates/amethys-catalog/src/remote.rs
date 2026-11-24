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
        if parsed.scheme() != "https" || parsed.host_str().is_none()
            || !parsed.username().is_empty() || parsed.password().is_some()
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
    pub igdb_id: u64,
    pub name: String,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub stores: Vec<CatalogStoreIdentity>,
}

/// A current PC game deal returned by CheapShark's public deals API.
#[derive(Debug, Clone, PartialEq)]
pub struct CheapSharkDeal {
    pub deal_id: String,
    pub title: String,
    pub store_name: String,
    pub sale_price_usd: f64,
    pub normal_price_usd: f64,
    pub savings_percent: f64,
    pub deal_rating: Option<f64>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheapSharkDealResponse {
    #[serde(rename = "dealID")]
    deal_id: String,
    title: String,
    #[serde(rename = "storeID")]
    store_id: String,
    #[serde(rename = "salePrice")]
    sale_price: String,
    #[serde(rename = "normalPrice")]
    normal_price: String,
    savings: String,
    #[serde(rename = "isOnSale")]
    is_on_sale: String,
    #[serde(rename = "dealRating")]
    deal_rating: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheapSharkStoreResponse {
    #[serde(rename = "storeID")]
    store_id: String,
    #[serde(rename = "storeName")]
    store_name: String,
}

/// Fetches a single user-requested page of live deals; it deliberately avoids
/// bulk crawling so the client respects CheapShark's public API rate limits.
pub async fn fetch_cheapshark_deals(limit: usize) -> RemoteCatalogResult<Vec<CheapSharkDeal>> {
    let limit = limit.clamp(1, 60);
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Amethys-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| RemoteCatalogError::Network(error.to_string()))?;

    let stores_response = http
        .get("https://www.cheapshark.com/api/1.0/stores")
        .send()
        .await
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark stores: {error}")))?
        .error_for_status()
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark stores: {error}")))?;
    let stores_bytes = stores_response
        .bytes()
        .await
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark stores: {error}")))?;
    let stores: Vec<CheapSharkStoreResponse> =
        serde_json::from_slice(&stores_bytes).map_err(|error| {
            RemoteCatalogError::InvalidData(format!("CheapShark stores JSON: {error}"))
        })?;
    let store_names = stores
        .into_iter()
        .map(|store| (store.store_id, store.store_name))
        .collect::<std::collections::HashMap<_, _>>();

    let deals_response = http
        .get("https://www.cheapshark.com/api/1.0/deals")
        .query(&[
            ("onSale", "1".to_owned()),
            ("pageNumber", "0".to_owned()),
            ("pageSize", limit.to_string()),
            ("sortBy", "DealRating".to_owned()),
            ("desc", "1".to_owned()),
        ])
        .send()
        .await
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark deals: {error}")))?
        .error_for_status()
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark deals: {error}")))?;
    let deals_bytes = deals_response
        .bytes()
        .await
        .map_err(|error| RemoteCatalogError::Network(format!("CheapShark deals: {error}")))?;
    let deals: Vec<CheapSharkDealResponse> =
        serde_json::from_slice(&deals_bytes).map_err(|error| {
            RemoteCatalogError::InvalidData(format!("CheapShark deals JSON: {error}"))
        })?;

    let mut result = Vec::with_capacity(deals.len().min(limit));
    for deal in deals {
        let (Ok(sale_price), Ok(normal_price), Ok(savings_percent)) = (
            deal.sale_price.parse::<f64>(),
            deal.normal_price.parse::<f64>(),
            deal.savings.parse::<f64>(),
        ) else {
            continue;
        };
        if deal.is_on_sale != "1"
            || !sale_price.is_finite()
            || !normal_price.is_finite()
            || !savings_percent.is_finite()
            || sale_price < 0.0
            || normal_price <= sale_price
            || savings_percent <= 0.0
        {
            continue;
        }
        let mut url = Url::parse("https://www.cheapshark.com/redirect")
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))?;
        // CheapShark returns dealID URL-encoded already. Decode before
        // appending it as a query pair to avoid double-encoding percent signs.
        let encoded_id = format!("dealID={}", deal.deal_id);
        let decoded_id = url::form_urlencoded::parse(encoded_id.as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        url.query_pairs_mut().append_pair("dealID", &decoded_id);
        result.push(CheapSharkDeal {
            deal_id: deal.deal_id,
            title: deal.title,
            store_name: store_names
                .get(&deal.store_id)
                .cloned()
                .unwrap_or_else(|| format!("Boutique {}", deal.store_id)),
            sale_price_usd: sale_price,
            normal_price_usd: normal_price,
            savings_percent: savings_percent.clamp(0.0, 100.0),
            deal_rating: deal
                .deal_rating
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite()),
            url: url.to_string(),
        });
        if result.len() >= limit {
            break;
        }
    }
    Ok(result)
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
        Ok(Self { provider, http, cache_dir: cache_dir.into() })
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub async fn fetch_manifest(&self) -> RemoteCatalogResult<CatalogManifest> {
        let url = self.provider.base_url().join("manifest.json")
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))?;
        let bytes = self.fetch_bytes(&url, MAX_MANIFEST_BYTES, None).await?;
        let manifest: CatalogManifest = serde_json::from_slice(&bytes)
            .map_err(|error| RemoteCatalogError::InvalidData(format!("manifest JSON: {error}")))?;
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
        let compressed = self.fetch_bytes(
            &url,
            MAX_COMPRESSED_FILE_BYTES,
            descriptor.compressed_sha256.as_deref(),
        ).await?;
        verify_size(descriptor.compressed_size, compressed.len())?;
        let decoded = decompress_bounded(&compressed)?;
        // Accept both the legacy three-field rows and the genre-aware
        // four-field format so existing published catalogues remain readable.
        let rows: Vec<serde_json::Value> = serde_json::from_slice(&decoded)
            .map_err(|error| RemoteCatalogError::InvalidData(format!("search index JSON: {error}")))?;
        if rows.len() as u64 != manifest.game_count {
            return Err(RemoteCatalogError::InvalidData(format!(
                "search index has {} rows; manifest declares {}", rows.len(), manifest.game_count
            )));
        }
        let mut previous_id = 0;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let fields = row.as_array().ok_or_else(|| RemoteCatalogError::InvalidData(
                "search index rows must be arrays".into()
            ))?;
            if !(3..=4).contains(&fields.len()) {
                return Err(RemoteCatalogError::InvalidData("search index row has an unsupported field count".into()));
            }
            let id = fields[0].as_u64().ok_or_else(|| RemoteCatalogError::InvalidData("search index ID is invalid".into()))?;
            let name = fields[1].as_str().ok_or_else(|| RemoteCatalogError::InvalidData("search index name is invalid".into()))?.to_owned();
            let slug = if fields[2].is_null() { None } else {
                Some(fields[2].as_str().ok_or_else(|| RemoteCatalogError::InvalidData("search index slug is invalid".into()))?.to_owned())
            };
            let genres = fields.get(3).and_then(serde_json::Value::as_array)
                .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            if id == 0 || id <= previous_id || name.trim().is_empty() || name.len() > 512 {
                return Err(RemoteCatalogError::InvalidData(
                    "search index IDs must be unique, ascending and nonzero; names must be valid".into()
                ));
            }
            previous_id = id;
            entries.push(CatalogSearchEntry { amethys_id: id, name, slug, genres });
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
                "search query or result limit is outside allowed bounds".into()
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
        Ok(entries.iter()
            .filter(|entry| entry.name.to_lowercase().contains(&needle))
            .take(limit)
            .cloned()
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
        if limit == 0 || limit > 100 || owned_genres.len() > 512
            || owned_ids.len() > 10_000 || owned_names.len() > 10_000
            || owned_genres.iter().any(|value| value.trim().is_empty() || value.len() > 128)
            || owned_names.iter().any(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err(RemoteCatalogError::InvalidData(
                "recommendation profile or result limit is outside allowed bounds".into()
            ));
        }

        let mut genre_weights = std::collections::HashMap::<String, usize>::new();
        for genre in owned_genres {
            *genre_weights.entry(genre.trim().to_lowercase()).or_default() += 1;
        }
        let owned_ids = owned_ids.iter().copied().collect::<std::collections::HashSet<_>>();
        let owned_names = owned_names.iter().map(|name| normalize_game_title(name))
            .collect::<std::collections::HashSet<_>>();

        // A locally installed game may not have been linked to an Amethys ID yet.
        // Exact title matches let the catalogue itself supply its genres anyway.
        for entry in entries.iter().filter(|entry| owned_ids.contains(&entry.amethys_id)
            || owned_names.contains(&normalize_game_title(&entry.name)))
        {
            for genre in &entry.genres {
                *genre_weights.entry(genre.trim().to_lowercase()).or_default() += 1;
            }
        }

        let mut ranked = entries.iter()
            .filter(|entry| !owned_ids.contains(&entry.amethys_id)
                && !owned_names.contains(&entry.name.trim().to_lowercase()))
            .map(|entry| {
                let mut seen = std::collections::HashSet::new();
                let score = entry.genres.iter()
                    .map(|genre| genre.trim().to_lowercase())
                    .filter(|genre| !genre.is_empty() && seen.insert(genre.clone()))
                    .map(|genre| genre_weights.get(&genre).copied().unwrap_or(0))
                    .sum::<usize>();
                (score, entry)
            })
            .collect::<Vec<_>>();
        if genre_weights.is_empty() {
            ranked.sort_by_cached_key(|(_, entry)| entry.name.to_lowercase());
        } else {
            ranked.sort_by(|left, right| right.0.cmp(&left.0));
        }
        Ok(ranked.into_iter().take(limit).map(|(_, entry)| entry.clone()).collect())
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
            return Err(RemoteCatalogError::InvalidData("Amethys ID cannot be zero".into()));
        }
        let bucket = amethys_id / manifest.shards.bucket_size;
        let descriptor = manifest.shards.files.iter().find(|item| item.bucket == bucket)
            .ok_or_else(|| RemoteCatalogError::InvalidData(format!("no shard for Amethys ID {amethys_id}")))?;
        let cache_path = self.cache_dir.join("shards").join(format!("{:06}-{}.json", bucket, &descriptor.compressed_sha256[..12]));
        let records: Vec<CatalogGameDetail> = if cache_path.is_file() {
            let bytes = fs::read(&cache_path).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
            serde_json::from_slice(&bytes)
                .map_err(|error| RemoteCatalogError::InvalidData(format!("cached shard JSON: {error}")))?
        } else {
            let url = self.file_url(&descriptor.file)?;
            let compressed = self.fetch_bytes(
                &url, MAX_COMPRESSED_FILE_BYTES, Some(&descriptor.compressed_sha256)
            ).await?;
            verify_size(Some(descriptor.compressed_size), compressed.len())?;
            let decoded = decompress_bounded(&compressed)?;
            let records: Vec<CatalogGameDetail> = serde_json::from_slice(&decoded)
                .map_err(|error| RemoteCatalogError::InvalidData(format!("detail shard JSON: {error}")))?;
            if records.len() as u64 != descriptor.game_count {
                return Err(RemoteCatalogError::InvalidData("detail shard record count mismatch".into()));
            }
            for record in &records {
                if record.amethys_id / manifest.shards.bucket_size != bucket
                    || record.amethys_id == 0 || record.name.trim().is_empty()
                {
                    return Err(RemoteCatalogError::InvalidData("detail shard contains an invalid game identity".into()));
                }
            }
            let json = serde_json::to_vec(&records)
                .map_err(|error| RemoteCatalogError::InvalidData(error.to_string()))?;
            atomic_write(&cache_path, &json)?;
            records
        };
        records.into_iter().find(|game| game.amethys_id == amethys_id)
            .ok_or_else(|| RemoteCatalogError::InvalidData(format!("game {amethys_id} is absent from its shard")))
    }

    fn file_url(&self, relative: &str) -> RemoteCatalogResult<Url> {
        validate_relative_path(relative)?;
        self.provider.base_url().join(relative)
            .map_err(|error| RemoteCatalogError::InvalidUrl(error.to_string()))
    }

    async fn fetch_bytes(
        &self,
        url: &Url,
        max_bytes: usize,
        expected_sha256: Option<&str>,
    ) -> RemoteCatalogResult<Vec<u8>> {
        if url.scheme() != "https" {
            return Err(RemoteCatalogError::InvalidUrl("only HTTPS catalogue files are accepted".into()));
        }
        let mut response = self.http.get(url.clone()).send().await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?;
        if response.url().scheme() != "https" {
            return Err(RemoteCatalogError::InvalidUrl("catalogue redirects must remain HTTPS".into()));
        }
        if response.content_length().is_some_and(|length| length > max_bytes as u64) {
            return Err(RemoteCatalogError::InvalidData("catalogue response exceeds size limit".into()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await
            .map_err(|error| RemoteCatalogError::Network(error.to_string()))?
        {
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(RemoteCatalogError::InvalidData("catalogue response exceeds size limit".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(&bytes);
            if !constant_time_eq(expected, &actual) {
                return Err(RemoteCatalogError::InvalidData("catalogue SHA-256 mismatch".into()));
            }
        }
        Ok(bytes)
    }
}

fn validate_manifest(manifest: &CatalogManifest) -> RemoteCatalogResult<()> {
    if manifest.manifest_schema_version != 2 || manifest.catalog_schema_version != 1 {
        return Err(RemoteCatalogError::InvalidData("unsupported catalogue manifest/schema version".into()));
    }
    if manifest.catalog_version.trim().is_empty() || manifest.game_count == 0
        || manifest.shards.bucket_size == 0 || manifest.shards.game_count != manifest.game_count
    {
        return Err(RemoteCatalogError::InvalidData("manifest counters or version are invalid".into()));
    }
    validate_relative_path(&manifest.search_index.file)?;
    if manifest.search_index.compressed_sha256.as_deref().is_none_or(|hash| !valid_sha256(hash)) {
        return Err(RemoteCatalogError::InvalidData("search-index checksum is missing or invalid".into()));
    }
    if manifest.shards.files.len() > 100_000 {
        return Err(RemoteCatalogError::InvalidData("manifest contains too many shards".into()));
    }
    let mut buckets = std::collections::BTreeSet::new();
    for shard in &manifest.shards.files {
        validate_relative_path(&shard.file)?;
        if !buckets.insert(shard.bucket) || shard.game_count == 0 || !valid_sha256(&shard.compressed_sha256) {
            return Err(RemoteCatalogError::InvalidData("manifest contains an invalid shard descriptor".into()));
        }
    }
    validate_relative_path(&manifest.metadata.genres.file)?;
    Ok(())
}

fn validate_relative_path(value: &str) -> RemoteCatalogResult<()> {
    let path = Path::new(value);
    if value.is_empty() || value.len() > 512 || path.is_absolute()
        || path.components().any(|component| !matches!(component, Component::Normal(_)))
        || value.contains('\\') || value.contains(':')
    {
        return Err(RemoteCatalogError::InvalidData("catalogue file path is not a safe relative path".into()));
    }
    Ok(())
}

fn decompress_bounded(compressed: &[u8]) -> RemoteCatalogResult<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(compressed)
        .map_err(|error| RemoteCatalogError::InvalidData(format!("Zstandard stream: {error}")))?;
    let mut limited = decoder.take(MAX_DECOMPRESSED_FILE_BYTES as u64 + 1);
    let mut output = Vec::new();
    limited.read_to_end(&mut output)
        .map_err(|error| RemoteCatalogError::InvalidData(format!("Zstandard decompression: {error}")))?;
    if output.len() > MAX_DECOMPRESSED_FILE_BYTES {
        return Err(RemoteCatalogError::InvalidData("decompressed catalogue file exceeds size limit".into()));
    }
    Ok(output)
}

fn verify_size(expected: Option<u64>, actual: usize) -> RemoteCatalogResult<()> {
    if expected.is_some_and(|size| size != actual as u64) {
        return Err(RemoteCatalogError::InvalidData("catalogue compressed-size mismatch".into()));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.bytes().zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> RemoteCatalogResult<()> {
    let parent = path.parent().ok_or_else(|| RemoteCatalogError::Cache("cache path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::File::create(&temporary).map_err(|error| RemoteCatalogError::Cache(error.to_string()))?;
    file.write_all(bytes).and_then(|()| file.sync_all())
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
            },
            CatalogSearchEntry {
                amethys_id: 2,
                name: "Puzzle".into(),
                slug: None,
                genres: vec!["Puzzle".into()],
            },
            CatalogSearchEntry {
                amethys_id: 3,
                name: "Action Adventure".into(),
                slug: None,
                genres: vec!["Action".into(), "Adventure".into()],
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
            },
            CatalogSearchEntry {
                amethys_id: 11,
                name: "Another RPG".into(),
                slug: None,
                genres: vec!["RPG".into()],
            },
            CatalogSearchEntry {
                amethys_id: 12,
                name: "Puzzle World".into(),
                slug: None,
                genres: vec!["Puzzle".into()],
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
    fn rejects_non_sha256_hashes() {
        assert!(!valid_sha256("not-a-hash"));
        assert!(valid_sha256(&"a".repeat(64)));
    }
}
