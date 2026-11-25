use amethys_events::EventBus;
use amethys_ipc::{
    IpcCatalogGameDetail, IpcCatalogSearchEntry, IpcCatalogSourceIdentity, IpcCatalogStoreIdentity,
    IpcCommand, IpcCoreStatus, IpcEnvelope, IpcGameEntry, IpcOwnedGameEntry, IpcRemoteError,
    IpcRequest, IpcResponse, IpcResponsePayload, read_envelope, transport::IpcListener,
    write_message,
};
use amethys_runtime::{CoreRuntime, RuntimeState};
use amethys_storage::Database;
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};

const STATE_CREATED: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_STOPPING: u8 = 2;
const STATE_STOPPED: u8 = 3;

const DEFAULT_MASTER_CATALOG_URL: &str = "https://likounsee.github.io/Amethys-Catalog/";

async fn sync_master_catalog() -> Result<IpcResponsePayload, String> {
    let data_dir = amethys_config::prepare_data_directory().map_err(|error| error.to_string())?;
    let base_url = std::env::var("AMETHYS_CATALOG_URL")
        .unwrap_or_else(|_| DEFAULT_MASTER_CATALOG_URL.to_owned());
    let provider = amethys_catalog::remote::StaticCatalogProvider::new(&base_url)
        .map_err(|error| error.to_string())?;
    let client =
        amethys_catalog::remote::RemoteCatalogClient::new(provider, data_dir.join("catalog-cache"))
            .map_err(|error| error.to_string())?;
    let manifest = client
        .fetch_manifest()
        .await
        .map_err(|error| error.to_string())?;
    let entries = client
        .synchronize_index(&manifest)
        .await
        .map_err(|error| error.to_string())?;
    Ok(IpcResponsePayload::CatalogSynchronized {
        catalog_version: manifest.catalog_version,
        game_count: entries.len() as u64,
    })
}

async fn search_master_catalog(query: &str, limit: usize) -> Result<IpcResponsePayload, String> {
    let data_dir = amethys_config::prepare_data_directory().map_err(|error| error.to_string())?;
    let base_url = std::env::var("AMETHYS_CATALOG_URL")
        .unwrap_or_else(|_| DEFAULT_MASTER_CATALOG_URL.to_owned());
    let provider = amethys_catalog::remote::StaticCatalogProvider::new(&base_url)
        .map_err(|error| error.to_string())?;
    let client =
        amethys_catalog::remote::RemoteCatalogClient::new(provider, data_dir.join("catalog-cache"))
            .map_err(|error| error.to_string())?;
    let entries = client
        .load_cached_index()
        .map_err(|error| error.to_string())?;
    let results = client
        .search_cached(&entries, query, limit)
        .map_err(|error| error.to_string())?;

    // Cache artwork only for the first visible batch. Remaining results keep
    // their URL and can be loaded when a later page/search makes them visible.
    let mut games = Vec::with_capacity(results.len());
    for (index, entry) in results.into_iter().enumerate() {
        // Keep search responsive: fetch a small first batch with a strict
        // per-cover deadline; missing artwork remains a normal placeholder.
        let cover_cache_path = if index < 8 {
            match entry.cover_url.as_deref() {
                Some(url) => tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    client.cache_cover(entry.amethys_id, url),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .map(|path| path.to_string_lossy().into_owned()),
                None => None,
            }
        } else {
            None
        };
        games.push(IpcCatalogSearchEntry {
            amethys_id: entry.amethys_id,
            name: entry.name,
            slug: entry.slug,
            genres: entry.genres,
            cover_url: entry.cover_url,
            cover_cache_path,
        });
    }
    Ok(IpcResponsePayload::CatalogSearchResults { games })
}

async fn recommend_master_catalog(
    genres: &[String],
    owned_ids: &[u64],
    owned_names: &[String],
    limit: usize,
) -> Result<IpcResponsePayload, String> {
    let data_dir = amethys_config::prepare_data_directory().map_err(|error| error.to_string())?;
    let base_url = std::env::var("AMETHYS_CATALOG_URL")
        .unwrap_or_else(|_| DEFAULT_MASTER_CATALOG_URL.to_owned());
    let provider = amethys_catalog::remote::StaticCatalogProvider::new(&base_url)
        .map_err(|error| error.to_string())?;
    let client =
        amethys_catalog::remote::RemoteCatalogClient::new(provider, data_dir.join("catalog-cache"))
            .map_err(|error| error.to_string())?;
    let entries = client
        .load_cached_index()
        .map_err(|error| error.to_string())?;
    let recommendations = client
        .recommend_cached(&entries, genres, owned_ids, owned_names, limit)
        .map_err(|error| error.to_string())?;
    let mut games = Vec::with_capacity(recommendations.len());
    for (index, entry) in recommendations.into_iter().enumerate() {
        let cover_cache_path = if index < 8 {
            match entry.cover_url.as_deref() {
                Some(url) => tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    client.cache_cover(entry.amethys_id, url),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .map(|path| path.to_string_lossy().into_owned()),
                None => None,
            }
        } else {
            None
        };
        games.push(IpcCatalogSearchEntry {
            amethys_id: entry.amethys_id,
            name: entry.name,
            slug: entry.slug,
            genres: entry.genres,
            cover_url: entry.cover_url,
            cover_cache_path,
        });
    }
    Ok(IpcResponsePayload::CatalogSearchResults { games })
}

async fn get_master_catalog_game(amethys_id: u64) -> Result<IpcResponsePayload, String> {
    let data_dir = amethys_config::prepare_data_directory().map_err(|error| error.to_string())?;
    let base_url = std::env::var("AMETHYS_CATALOG_URL")
        .unwrap_or_else(|_| DEFAULT_MASTER_CATALOG_URL.to_owned());
    let provider = amethys_catalog::remote::StaticCatalogProvider::new(&base_url)
        .map_err(|error| error.to_string())?;
    let client =
        amethys_catalog::remote::RemoteCatalogClient::new(provider, data_dir.join("catalog-cache"))
            .map_err(|error| error.to_string())?;
    let manifest = match client.fetch_manifest().await {
        Ok(manifest) => manifest,
        Err(remote_error) => client.load_cached_manifest().map_err(|cache_error| {
            format!(
                "Catalogue distant indisponible ({remote_error}) et manifeste local inutilisable ({cache_error})."
            )
        })?,
    };
    let game = client
        .fetch_game(&manifest, amethys_id)
        .await
        .map_err(|error| error.to_string())?;
    let cover_cache_path = match game.cover_url.as_deref() {
        Some(url) => client
            .cache_cover(game.amethys_id, url)
            .await
            .ok()
            .flatten()
            .map(|path| path.to_string_lossy().into_owned()),
        None => None,
    };
    Ok(IpcResponsePayload::CatalogGameDetails {
        game: IpcCatalogGameDetail {
            amethys_id: game.amethys_id,
            igdb_id: game.igdb_id,
            name: game.name,
            slug: game.slug,
            summary: game.summary,
            release_date: game.release_date,
            cover_url: game.cover_url,
            cover_cache_path,
            genres: game.genres,
            source_ids: game
                .source_ids
                .into_iter()
                .map(|source_id| IpcCatalogSourceIdentity {
                    source: source_id.source,
                    external_id: source_id.external_id,
                })
                .collect(),
            stores: game
                .stores
                .into_iter()
                .map(|store| IpcCatalogStoreIdentity {
                    store: store.store,
                    external_id: store.external_id,
                    name: store.name,
                    url: store.url,
                })
                .collect(),
        },
    })
}

fn mutation_allowed(state: IpcCoreStatus) -> bool {
    state == IpcCoreStatus::Running
}

fn ipc_status(state: &AtomicU8) -> IpcCoreStatus {
    match state.load(Ordering::Acquire) {
        STATE_RUNNING => IpcCoreStatus::Running,
        STATE_STOPPING => IpcCoreStatus::Stopping,
        STATE_STOPPED => IpcCoreStatus::Stopped,
        _ => IpcCoreStatus::Created,
    }
}

async fn serve_database_connection<S>(
    stream: &mut S,
    shared_state: &AtomicU8,
    database: &Mutex<Database>,
    discovery_notify: &Notify,
) -> Result<(), Box<dyn Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: IpcEnvelope<IpcRequest> = read_envelope(stream).await?;
    // Read the lifecycle state when the request is handled, not when its
    // socket was accepted: a client may connect and then stall before sending.
    let state = ipc_status(shared_state);
    match &request.payload.command {
        IpcCommand::SyncMasterCatalog => {
            let result = if mutation_allowed(state) {
                sync_master_catalog().await
            } else {
                Err("Catalogue synchronization is unavailable while Core is not running.".into())
            };
            let response = match result {
                Ok(payload) => IpcResponse::success(request.request_id, payload),
                Err(message) => IpcResponse::failure(
                    request.request_id,
                    IpcRemoteError::new("AM-CATALOG-0001", message),
                ),
            };
            write_message(stream, &response).await?;
            return Ok(());
        }
        IpcCommand::GetMasterCatalogGame { amethys_id } => {
            let response = match get_master_catalog_game(*amethys_id).await {
                Ok(payload) => IpcResponse::success(request.request_id, payload),
                Err(message) => IpcResponse::failure(
                    request.request_id,
                    IpcRemoteError::new("AM-CATALOG-0003", message),
                ),
            };
            write_message(stream, &response).await?;
            return Ok(());
        }
        IpcCommand::SearchMasterCatalog { query, limit } => {
            let response = match search_master_catalog(query, *limit).await {
                Ok(payload) => IpcResponse::success(request.request_id, payload),
                Err(message) => IpcResponse::failure(
                    request.request_id,
                    IpcRemoteError::new("AM-CATALOG-0002", message),
                ),
            };
            write_message(stream, &response).await?;
            return Ok(());
        }
        IpcCommand::SearchRecommendedCatalog {
            genres,
            owned_ids,
            owned_names,
            limit,
        } => {
            let response = match recommend_master_catalog(genres, owned_ids, owned_names, *limit).await {
                Ok(payload) => IpcResponse::success(request.request_id, payload),
                Err(message) => IpcResponse::failure(
                    request.request_id,
                    IpcRemoteError::new("AM-CATALOG-0004", message),
                ),
            };
            write_message(stream, &response).await?;
            return Ok(());
        }
        _ => {}
    }
    if !mutation_allowed(state)
        && matches!(
            &request.payload.command,
            IpcCommand::AddGame { .. }
                | IpcCommand::AddDetectedGame { .. }
                | IpcCommand::SetGameExecutable { .. }
                | IpcCommand::RemoveGame { .. }
                | IpcCommand::AddOwnedGame { .. }
                | IpcCommand::RemoveOwnedGame { .. }
                | IpcCommand::SyncMasterCatalog
        )
    {
        let response = IpcResponse::failure(
            request.request_id,
            IpcRemoteError::new(
                "AM-CORE-0002",
                "Game-library changes are unavailable while the core is not running.",
            ),
        );
        write_message(stream, &response).await?;
        return Ok(());
    }
    let retry_executable_discovery = matches!(
        &request.payload.command,
        IpcCommand::AddGame { .. } | IpcCommand::AddDetectedGame { .. }
    );
    let result = match request.payload.command {
        IpcCommand::Ping => Ok(IpcResponsePayload::Pong),
        IpcCommand::GetStatus => Ok(IpcResponsePayload::CoreStatus { state }),
        command => {
            let database = database.lock().await;
            match command {
                IpcCommand::ListOwnedGames => {
                    database
                        .list_owned_games()
                        .map(|entries| IpcResponsePayload::OwnedGames {
                            games: entries
                                .into_iter()
                                .map(|game| IpcOwnedGameEntry {
                                    amethys_id: game.amethys_id,
                                    name: game.name,
                                    genres: game.genres,
                                    added_at: game.added_at,
                                })
                                .collect(),
                        })
                }
                IpcCommand::AddOwnedGame {
                    amethys_id,
                    name,
                    genres,
                } => database
                    .add_owned_game(amethys_id, &name, &genres)
                    .map(|game| IpcResponsePayload::OwnedGameAdded {
                        game: IpcOwnedGameEntry {
                            amethys_id: game.amethys_id,
                            name: game.name,
                            genres: game.genres,
                            added_at: game.added_at,
                        },
                    }),
                IpcCommand::RemoveOwnedGame { amethys_id } => database
                    .remove_owned_game(amethys_id)
                    .map(|removed| IpcResponsePayload::OwnedGameRemoved { removed }),
                IpcCommand::ListGames => database.list_games().and_then(|entries| {
                    entries
                        .into_iter()
                        .map(|game| ipc_game_entry(&database, game))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|games| IpcResponsePayload::Games { games })
                }),
                IpcCommand::AddGame {
                    name,
                    platform,
                    install_path,
                    executable,
                } => database
                    .add_game(&name, &platform, &install_path, executable.as_deref())
                    .and_then(|game| {
                        ipc_game_entry(&database, game).map(|game| IpcResponsePayload::GameAdded {
                            game: Box::new(game),
                        })
                    }),
                IpcCommand::AddDetectedGame {
                    name,
                    platform,
                    platform_id,
                    install_path,
                    executable,
                } => {
                    // Import is idempotent by source identity. The database mutex
                    // makes lookup + insert atomic relative to other IPC requests.
                    let game = if let Some(platform_id) = platform_id.as_deref() {
                        database
                            .get_game_by_platform_id(&platform, platform_id)
                            .and_then(|existing| match existing {
                                Some(game) => Ok(game),
                                None => {
                                    // A previously imported entry may predate stable
                                    // platform IDs. Reuse it only when its identity is
                                    // unknown and its platform-scoped path matches.
                                    let legacy = database.list_games()?.into_iter().find(|game| {
                                        game.platform.eq_ignore_ascii_case(&platform)
                                            && game.platform_id.is_none()
                                            && same_install_path(&game.install_path, &install_path)
                                    });
                                    match legacy {
                                        Some(game) => database
                                            .set_game_platform_id(&game.id, &platform, platform_id)?
                                            .ok_or_else(|| {
                                                amethys_errors::AmethysError::new(
                                                    amethys_errors::ErrorCode::StorageQuery,
                                                    "Legacy game disappeared while attaching its platform ID.",
                                                )
                                            }),
                                        None => database.add_game_with_platform_id(
                                            &name,
                                            &platform,
                                            Some(platform_id),
                                            &install_path,
                                            executable.as_deref(),
                                        ),
                                    }
                                }
                            })
                    } else {
                        // Some discovery adapters cannot provide a stable platform ID.
                        // Reuse the existing library entry by installation path so a
                        // repeated scan/import does not create duplicate rows.
                        let existing = database.list_games()?.into_iter().find(|game| {
                            game.platform.eq_ignore_ascii_case(&platform)
                                && same_install_path(&game.install_path, &install_path)
                        });
                        match existing {
                            Some(game) => Ok(game),
                            None => database.add_game_with_platform_id(
                                &name,
                                &platform,
                                None,
                                &install_path,
                                executable.as_deref(),
                            ),
                        }
                    };
                    game.and_then(|game| {
                        attach_detected_provider_identity(
                            &database,
                            &game,
                            &platform,
                            platform_id.as_deref(),
                        )?;
                        ipc_game_entry(&database, game).map(|game| IpcResponsePayload::GameAdded {
                            game: Box::new(game),
                        })
                    })
                }
                IpcCommand::SetGameExecutable { id, executable } => database
                    .set_game_executable(&id, &executable)
                    .and_then(|game| {
                        game.map(|game| ipc_game_entry(&database, game))
                            .transpose()?
                            .map(|game| IpcResponsePayload::GameAdded {
                                game: Box::new(game),
                            })
                            .ok_or_else(|| {
                                amethys_errors::AmethysError::new(
                                    amethys_errors::ErrorCode::StorageQuery,
                                    "Game not found.",
                                )
                            })
                    }),
                IpcCommand::RemoveGame { id } => database
                    .remove_game(&id)
                    .map(|removed| IpcResponsePayload::GameRemoved { removed }),
                IpcCommand::Ping
                | IpcCommand::GetStatus
                | IpcCommand::SyncMasterCatalog
                | IpcCommand::SearchMasterCatalog { .. }
                | IpcCommand::SearchRecommendedCatalog { .. }
                | IpcCommand::GetMasterCatalogGame { .. } => unreachable!("handled above"),
            }
        }
    };
    // Imports can happen after Core has already started. Wake the background
    // resolver immediately instead of waiting for the next process restart.
    if retry_executable_discovery {
        discovery_notify.notify_one();
    }
    let response = match result {
        Ok(payload) => IpcResponse::success(request.request_id, payload),
        Err(error) => IpcResponse::failure(
            request.request_id,
            IpcRemoteError::new(error.code().to_string(), error.message()),
        ),
    };
    write_message(stream, &response).await?;
    Ok(())
}

/// Checks whether a saved executable still points to a real file inside its game installation.
fn configured_executable_is_usable(install_path: &str, executable: &str) -> bool {
    let root = match std::path::Path::new(install_path).canonicalize() {
        Ok(root) if root.is_dir() => root,
        _ => return false,
    };
    let candidate = std::path::Path::new(executable);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        if executable.trim().is_empty()
            || executable.starts_with('/')
            || executable.starts_with('\\')
            || executable
                .split(['/', '\\'])
                .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
        {
            return false;
        }
        root.join(executable)
    };
    candidate
        .canonicalize()
        .is_ok_and(|resolved| resolved.is_file() && resolved.starts_with(&root))
}

/// Retries executable discovery for every library entry with a missing or unusable path.
///
/// The filesystem scan runs on Tokio's blocking pool and never holds the
/// database mutex. Successful results are persisted only if the entry is
/// still present and still has the same missing or stale path when the scan completes.
async fn resolve_missing_game_executables_on_startup(database: Arc<Mutex<Database>>) {
    let missing_games = {
        let database = database.lock().await;
        match database.list_games() {
            Ok(games) => games
                .into_iter()
                .filter(|game| {
                    game.executable.as_deref().is_none_or(|executable| {
                        !configured_executable_is_usable(&game.install_path, executable)
                    })
                })
                .map(|game| {
                    (
                        game.id,
                        game.name,
                        game.platform,
                        game.platform_id,
                        game.install_path,
                        game.executable,
                    )
                })
                .collect::<Vec<_>>(),
            Err(error) => {
                tracing::warn!(%error, "Unable to list games for startup executable discovery");
                return;
            }
        }
    };

    if missing_games.is_empty() {
        tracing::debug!("Executable discovery skipped: every game has a usable executable path");
        return;
    }

    tracing::info!(
        candidates = missing_games.len(),
        "Starting background game executable discovery"
    );

    let scanned = tokio::task::spawn_blocking(move || {
        missing_games
            .into_iter()
            .filter_map(
                |(id, name, platform, platform_id, install_path, previous_executable)| {
                    let Some(executable) = amethys_discovery::resolve_game_executable_for(
                        &platform,
                        platform_id.as_deref(),
                        std::path::Path::new(&install_path),
                        &name,
                    ) else {
                        tracing::debug!(
                            game = %name,
                            install_path = %install_path,
                            "No unambiguous executable candidate found for game"
                        );
                        return None;
                    };
                    Some((
                        id,
                        previous_executable,
                        executable.to_string_lossy().into_owned(),
                    ))
                },
            )
            .collect::<Vec<_>>()
    })
    .await;

    let resolved = match scanned {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(%error, "Background executable discovery task failed");
            return;
        }
    };

    let mut saved = 0usize;
    for (id, previous_executable, executable) in resolved {
        let database = database.lock().await;
        match database.get_game(&id) {
            Ok(Some(game)) if game.executable == previous_executable => {
                match database.set_game_executable(&id, &executable) {
                    Ok(Some(_)) => saved += 1,
                    Ok(None) => {
                        tracing::debug!(game_id = %id, "Game disappeared before executable could be saved")
                    }
                    Err(error) => {
                        tracing::warn!(game_id = %id, %error, "Unable to save discovered game executable")
                    }
                }
            }
            Ok(Some(_)) => {
                // Preserve a path changed manually while discovery was running.
            }
            Ok(None) => {
                // The game was removed while discovery was running.
            }
            Err(error) => {
                tracing::warn!(game_id = %id, %error, "Unable to recheck game before saving executable");
            }
        }
    }

    tracing::info!(
        resolved = saved,
        "Background executable discovery completed"
    );
}

/// Keeps executable discovery alive for the lifetime of Core.
///
/// It runs once immediately, whenever a game is imported, and periodically so
/// that games whose files appear later are retried without restarting Core.
async fn run_game_executable_discovery(
    database: Arc<Mutex<Database>>,
    discovery_notify: Arc<Notify>,
) {
    let mut retry_interval = tokio::time::interval(std::time::Duration::from_secs(300));
    retry_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = retry_interval.tick() => {}
            _ = discovery_notify.notified() => {}
        }
        resolve_missing_game_executables_on_startup(Arc::clone(&database)).await;
    }
}

/// Maps discovery platform names to stable catalogue provider keys.
/// Heroic is intentionally excluded: its app names can refer to several stores.
fn catalogue_provider_key(platform: &str) -> Option<&'static str> {
    match platform.trim().to_ascii_lowercase().as_str() {
        "steam" => Some("steam"),
        "epic games" => Some("epic"),
        "gog" => Some("gog"),
        "amazon games" => Some("amazon-games"),
        "ea app" => Some("ea"),
        "ubisoft connect" => Some("ubisoft"),
        "battle.net" => Some("battlenet"),
        "riot games" => Some("riot"),
        _ => None,
    }
}

/// Attaches discovered provider identities to canonical catalogue records.
/// Matching is based only on exact provider IDs, never on game titles.
fn attach_detected_provider_identity(
    database: &Database,
    game: &amethys_storage::GameEntry,
    platform: &str,
    platform_id: Option<&str>,
) -> Result<(), amethys_errors::AmethysError> {
    let (Some(provider), Some(external_id)) =
        (catalogue_provider_key(platform), platform_id.map(str::trim))
    else {
        return Ok(());
    };
    if external_id.is_empty() {
        return Ok(());
    }

    if let Some(catalogue_game) =
        database.get_catalogue_game_by_external_id(provider, external_id)?
    {
        let current_catalogue_id = database.get_catalogue_game_id(&game.id)?;
        if current_catalogue_id.as_deref() == Some(catalogue_game.id.as_str()) {
            return Ok(());
        }

        // Never overwrite metadata or other provider identities that a user
        // may already have attached to this installation's catalogue record.
        let has_metadata = database.get_game_metadata(&game.id)?.is_some();
        let has_external_ids = !database.list_external_game_ids(&game.id)?.is_empty();
        if !has_metadata && !has_external_ids {
            database.set_game_catalogue_id(&game.id, &catalogue_game.id)?;
        }
    } else {
        database.set_external_game_id(&game.id, provider, external_id)?;
    }
    Ok(())
}

fn ipc_game_entry(
    database: &Database,
    game: amethys_storage::GameEntry,
) -> Result<IpcGameEntry, amethys_errors::AmethysError> {
    let catalogue_game_id = database.get_catalogue_game_id(&game.id)?;
    let catalogue_metadata = catalogue_game_id
        .as_deref()
        .map(|catalogue_id| database.get_catalogue_game_metadata(catalogue_id))
        .transpose()?
        .flatten()
        .map(|metadata| amethys_ipc::IpcGameMetadata {
            genres: metadata.genres,
            developer: metadata.developer,
            publisher: metadata.publisher,
            release_date: metadata.release_date,
            description: metadata.description,
        });
    Ok(IpcGameEntry {
        catalogue_game_id,
        catalogue_metadata,
        id: game.id,
        name: game.name,
        platform: game.platform,
        platform_id: game.platform_id,
        install_path: game.install_path,
        executable: game.executable,
    })
}

fn same_install_path(left: &str, right: &str) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| std::path::PathBuf::from(left));
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| std::path::PathBuf::from(right));

    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn store_runtime_state(shared: &AtomicU8, state: RuntimeState) {
    let value = match state {
        RuntimeState::Created => STATE_CREATED,
        RuntimeState::Running => STATE_RUNNING,
        RuntimeState::Stopping => STATE_STOPPING,
        RuntimeState::Stopped => STATE_STOPPED,
    };
    shared.store(value, Ordering::Release);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    amethys_logging::initialize()?;

    let data_dir = amethys_config::prepare_data_directory()?;
    let database = Arc::new(Mutex::new(Database::open(data_dir.join("amethys.sqlite"))?));
    amethys_catalog::CatalogRepository::initialize(&*database.lock().await)?;
    // Keep slow or stalled local clients from blocking the accept loop or
    // exhausting the agent's task and socket resources.
    const MAX_IPC_CONNECTIONS: usize = 32;
    let connection_limit = Arc::new(Semaphore::new(MAX_IPC_CONNECTIONS));
    let mut connections = JoinSet::new();

    let mut runtime = CoreRuntime::new(EventBus::new(128));
    runtime.start()?;

    let shared_state = Arc::new(AtomicU8::new(STATE_CREATED));
    store_runtime_state(&shared_state, runtime.state());

    #[cfg(unix)]
    let mut listener = IpcListener::bind(data_dir.join("amethys.sock")).await?;

    #[cfg(windows)]
    let mut listener = IpcListener::bind(r"\\.\pipe\amethys-core").await?;

    #[cfg(not(any(unix, windows)))]
    return Err("Amethys Core IPC is not supported on this platform.".into());

    // Start executable discovery before serving requests. The worker scans once
    // immediately, then reacts to imports and retries periodically.
    let discovery_notify = Arc::new(Notify::new());
    tokio::spawn(run_game_executable_discovery(
        Arc::clone(&database),
        Arc::clone(&discovery_notify),
    ));

    tracing::info!("Amethys Core is running and waiting for local requests");

    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal?;
                tracing::info!("Shutdown signal received");
                break;
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    tracing::error!(%error, "IPC connection task failed");
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok(mut stream) => {
                        let permit = match Arc::clone(&connection_limit).try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                tracing::warn!(
                                    limit = MAX_IPC_CONNECTIONS,
                                    "IPC connection limit reached; rejecting client"
                                );
                                continue;
                            }
                        };
                        let shared_state = Arc::clone(&shared_state);
                        let database = Arc::clone(&database);
                        let discovery_notify = Arc::clone(&discovery_notify);
                        connections.spawn(async move {
                            let _permit = permit;
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(15),
                                serve_database_connection(
                                    &mut stream,
                                    &shared_state,
                                    &database,
                                    &discovery_notify,
                                ),
                            ).await {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => {
                                    tracing::debug!(%error, "Local IPC request failed");
                                }
                                Err(_) => {
                                    tracing::warn!("Local IPC request timed out after 15 seconds");
                                }
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, "Unable to accept a local IPC connection");
                        break;
                    }
                }
            }
        }
    }

    shared_state.store(STATE_STOPPING, Ordering::Release);
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            tracing::error!(%error, "IPC connection task failed");
        }
    }
    runtime.shutdown()?;
    store_runtime_state(&shared_state, runtime.state());
    tracing::info!("Amethys Core stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Notify, STATE_STOPPING, mutation_allowed, serve_database_connection};
    use amethys_ipc::{
        IpcCommand, IpcCoreStatus, IpcEnvelope, IpcRequest, IpcResponse, read_message,
        write_message,
    };
    use amethys_storage::Database;
    use std::sync::{Arc, atomic::AtomicU8};
    use tokio::{io::duplex, sync::Mutex};

    #[tokio::test]
    async fn importing_game_after_core_start_wakes_background_executable_discovery() {
        let root = std::env::temp_dir().join(format!(
            "amethys-import-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create install directory");
        std::fs::write(root.join("Sample Game.exe"), b"fixture").expect("create game executable");

        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));
        let notify = Arc::new(Notify::new());
        let worker = tokio::spawn(super::run_game_executable_discovery(
            Arc::clone(&database),
            Arc::clone(&notify),
        ));

        let (mut client, mut server) = duplex(64 * 1024);
        let request = IpcEnvelope::new(IpcRequest::new(IpcCommand::AddGame {
            name: "Sample Game".to_owned(),
            platform: "Test Platform".to_owned(),
            install_path: root.to_string_lossy().into_owned(),
            executable: None,
        }));
        let server_database = Arc::clone(&database);
        let server_state = Arc::clone(&state);
        let server_notify = Arc::clone(&notify);
        let server_task = tokio::spawn(async move {
            serve_database_connection(&mut server, &server_state, &server_database, &server_notify)
                .await
                .expect("serve game import");
        });

        write_message(&mut client, &request)
            .await
            .expect("send game import");
        let response: IpcResponse = read_message(&mut client)
            .await
            .expect("read game import response");
        server_task.await.expect("server task");
        let game_id = match response.result.expect("game import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game.id.clone(),
            other => panic!("unexpected import response: {other:?}"),
        };

        let discovered = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let game = database
                    .lock()
                    .await
                    .get_game(&game_id)
                    .expect("game can be read")
                    .expect("game remains in library");
                if game.executable.is_some() {
                    break game;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background discovery should finish after import");

        assert_eq!(discovered.executable.as_deref(), Some("Sample Game.exe"));
        worker.abort();
        let _ = worker.await;
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[tokio::test]
    async fn startup_discovery_persists_executable_for_games_missing_one() {
        let root = std::env::temp_dir().join(format!(
            "amethys-startup-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create install directory");
        std::fs::write(root.join("Sample Game.exe"), b"fixture").expect("create game executable");

        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let game_id = database
            .lock()
            .await
            .add_game(
                "Sample Game",
                "Test Platform",
                &root.to_string_lossy(),
                None,
            )
            .expect("game without executable is added")
            .id;

        super::resolve_missing_game_executables_on_startup(Arc::clone(&database)).await;

        let game = database
            .lock()
            .await
            .get_game(&game_id)
            .expect("game can be read")
            .expect("game remains in library");
        assert_eq!(game.executable.as_deref(), Some("Sample Game.exe"));

        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[tokio::test]
    async fn startup_discovery_repairs_a_stale_saved_executable_path() {
        let root = std::env::temp_dir().join(format!(
            "amethys-stale-executable-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create install directory");
        std::fs::write(root.join("Sample Game.exe"), b"fixture").expect("create game executable");

        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let game_id = database
            .lock()
            .await
            .add_game(
                "Sample Game",
                "Test Platform",
                &root.to_string_lossy(),
                Some("old-folder/missing.exe"),
            )
            .expect("game with stale executable is added")
            .id;

        super::resolve_missing_game_executables_on_startup(Arc::clone(&database)).await;

        let game = database
            .lock()
            .await
            .get_game(&game_id)
            .expect("game can be read")
            .expect("game remains in library");
        assert_eq!(game.executable.as_deref(), Some("Sample Game.exe"));

        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn detected_provider_ids_link_installations_to_existing_catalogue_games() {
        let database = Database::open(":memory:").expect("database opens");
        let steam = database
            .add_game_with_platform_id("Example", "Steam", Some("570"), "/games/steam", None)
            .expect("Steam installation added");
        database
            .set_external_game_id(&steam.id, "steam", "570")
            .expect("Steam identity saved");
        database
            .set_external_game_id(&steam.id, "epic", "example-epic")
            .expect("Epic identity saved");

        let epic = database
            .add_game_with_platform_id(
                "Example",
                "Epic Games",
                Some("example-epic"),
                "/games/epic",
                None,
            )
            .expect("Epic installation added");
        super::attach_detected_provider_identity(
            &database,
            &epic,
            "Epic Games",
            Some("example-epic"),
        )
        .expect("catalogue identity attached");

        assert_eq!(
            database
                .get_catalogue_game_id(&epic.id)
                .expect("Epic catalogue ID"),
            database
                .get_catalogue_game_id(&steam.id)
                .expect("Steam catalogue ID")
        );
    }

    #[test]
    fn unknown_discovery_sources_are_not_guessed_as_storefronts() {
        assert_eq!(super::catalogue_provider_key("Heroic"), None);
        assert_eq!(super::catalogue_provider_key("Steam"), Some("steam"));
        assert_eq!(super::catalogue_provider_key("Epic Games"), Some("epic"));
    }

    #[test]
    fn library_mutations_are_only_allowed_while_core_is_running() {
        assert!(mutation_allowed(IpcCoreStatus::Running));
        assert!(!mutation_allowed(IpcCoreStatus::Created));
        assert!(!mutation_allowed(IpcCoreStatus::Stopping));
        assert!(!mutation_allowed(IpcCoreStatus::Stopped));
    }

    #[tokio::test]
    async fn stopping_core_rejects_game_library_mutations_without_writing_database() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(STATE_STOPPING));
        let (mut client, mut server) = duplex(4096);
        let request = IpcEnvelope::new(IpcRequest::new(IpcCommand::AddGame {
            name: "Must not be added".to_owned(),
            platform: "Steam".to_owned(),
            install_path: "/games/blocked".to_owned(),
            executable: None,
        }));
        let request_id = request.request_id;

        let server_database = Arc::clone(&database);
        let server_state = Arc::clone(&state);
        let notify = Notify::new();
        let server_task = tokio::spawn(async move {
            serve_database_connection(&mut server, &server_state, &server_database, &notify)
                .await
                .expect("serve rejected request");
        });

        write_message(&mut client, &request)
            .await
            .expect("send request");
        let response: IpcResponse = read_message(&mut client).await.expect("read response");
        server_task.await.expect("server task");

        assert_eq!(response.request_id, request_id);
        let error = response.result.expect_err("mutation must be rejected");
        assert_eq!(error.code, "AM-CORE-0002");
        assert!(
            database
                .lock()
                .await
                .list_games()
                .expect("list games")
                .is_empty(),
            "rejected mutation must not reach SQLite"
        );
    }

    async fn round_trip(
        database: &Arc<Mutex<Database>>,
        state: &Arc<AtomicU8>,
        command: IpcCommand,
    ) -> IpcResponse {
        let (mut client, mut server) = duplex(64 * 1024);
        let request = IpcEnvelope::new(IpcRequest::new(command));
        let server_database = Arc::clone(database);
        let server_state = Arc::clone(state);
        let notify = Notify::new();
        let server_task = tokio::spawn(async move {
            serve_database_connection(&mut server, &server_state, &server_database, &notify)
                .await
                .expect("serve IPC request");
        });

        write_message(&mut client, &request)
            .await
            .expect("write IPC request");
        let response = read_message(&mut client).await.expect("read IPC response");
        server_task.await.expect("IPC server task");
        response
    }

    #[tokio::test]
    async fn running_core_supports_detected_game_add_list_and_remove_over_ipc() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let added = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Dota 2".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("570".to_owned()),
                install_path: "/games/dota2".to_owned(),
                executable: Some("game/bin/win64/dota2.exe".to_owned()),
            },
        )
        .await;
        let added_game = match added.result.expect("detected game is added") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected add response: {other:?}"),
        };
        assert_eq!(added_game.name, "Dota 2");
        assert_eq!(added_game.platform_id.as_deref(), Some("570"));
        assert_eq!(
            added_game.catalogue_game_id.as_deref(),
            None,
            "an unknown storefront identity must not be guessed as an Amethys catalogue ID",
        );

        let listed = round_trip(&database, &state, IpcCommand::ListGames).await;
        let games = match listed.result.expect("game library is listed") {
            amethys_ipc::IpcResponsePayload::Games { games } => games,
            other => panic!("unexpected list response: {other:?}"),
        };
        assert_eq!(games, vec![*added_game.clone()]);

        let removed = round_trip(
            &database,
            &state,
            IpcCommand::RemoveGame {
                id: added_game.id.clone(),
            },
        )
        .await;
        assert_eq!(
            removed.result,
            Ok(amethys_ipc::IpcResponsePayload::GameRemoved { removed: true })
        );

        let listed_after_remove = round_trip(&database, &state, IpcCommand::ListGames).await;
        assert_eq!(
            listed_after_remove.result,
            Ok(amethys_ipc::IpcResponsePayload::Games { games: Vec::new() })
        );
    }

    #[tokio::test]
    async fn importing_the_same_platform_game_twice_is_idempotent_over_ipc() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let first = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Original name".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("570".to_owned()),
                install_path: "/games/original".to_owned(),
                executable: None,
            },
        )
        .await;
        let first_game = match first.result.expect("first import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected first import response: {other:?}"),
        };

        // A second scan may observe renamed or relocated metadata for the same
        // platform identity. It must reuse the existing library entry.
        let second = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Updated name".to_owned(),
                platform: "steam".to_owned(),
                platform_id: Some("570".to_owned()),
                install_path: "/games/moved".to_owned(),
                executable: Some("game.exe".to_owned()),
            },
        )
        .await;
        let second_game = match second.result.expect("repeat import is idempotent") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected repeat import response: {other:?}"),
        };

        assert_eq!(second_game, first_game);
        let games = database
            .lock()
            .await
            .list_games()
            .expect("list imported games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, first_game.id);
        assert_eq!(games[0].name, "Original name");
        assert_eq!(games[0].install_path, "/games/original");
    }

    #[tokio::test]
    async fn detected_import_with_platform_id_reuses_legacy_entry_at_same_path() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let legacy = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Legacy game".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: None,
                install_path: "/games/legacy".to_owned(),
                executable: None,
            },
        )
        .await;
        let legacy_game = match legacy.result.expect("legacy import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected legacy import response: {other:?}"),
        };

        let detected = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Detected game".to_owned(),
                platform: "steam".to_owned(),
                platform_id: Some("12345".to_owned()),
                install_path: "/games/legacy".to_owned(),
                executable: None,
            },
        )
        .await;
        let detected_game = match detected.result.expect("detected import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected detected import response: {other:?}"),
        };

        assert_eq!(detected_game.id, legacy_game.id);
        assert_eq!(detected_game.platform_id.as_deref(), Some("12345"));

        // Once upgraded, the stable source ID must find the same library row
        // even if the launcher later reports a changed installation path.
        let moved = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Detected game".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("12345".to_owned()),
                install_path: "/games/moved".to_owned(),
                executable: None,
            },
        )
        .await;
        let moved_game = match moved.result.expect("reimport by platform ID succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected moved import response: {other:?}"),
        };
        assert_eq!(moved_game.id, legacy_game.id);

        let games = database
            .lock()
            .await
            .list_games()
            .expect("list imported games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("12345"));
        assert_eq!(games[0].install_path, "/games/legacy");
    }

    #[tokio::test]
    async fn detected_import_without_platform_id_reuses_existing_installation_path() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let import = || {
            round_trip(
                &database,
                &state,
                IpcCommand::AddDetectedGame {
                    name: "Legacy launcher game".to_owned(),
                    platform: "Legacy".to_owned(),
                    platform_id: None,
                    install_path: "/games/legacy".to_owned(),
                    executable: None,
                },
            )
        };
        let first = import().await;
        let second = import().await;
        let extract_game = |response: IpcResponse| match response.result.expect("import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
            other => panic!("unexpected import response: {other:?}"),
        };
        let first_game = extract_game(first);
        let second_game = extract_game(second);

        assert_eq!(first_game.id, second_game.id);
        let games = database
            .lock()
            .await
            .list_games()
            .expect("list imported games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].install_path, "/games/legacy");
    }

    #[tokio::test]
    async fn detected_import_does_not_merge_same_path_from_another_platform() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let import = |platform: &str| {
            round_trip(
                &database,
                &state,
                IpcCommand::AddDetectedGame {
                    name: format!("{platform} game"),
                    platform: platform.to_owned(),
                    platform_id: None,
                    install_path: "/games/shared".to_owned(),
                    executable: None,
                },
            )
        };
        let first = import("Platform A").await;
        let second = import("Platform B").await;
        let extract_id = |response: IpcResponse| match response.result.expect("import succeeds") {
            amethys_ipc::IpcResponsePayload::GameAdded { game } => game.id,
            other => panic!("unexpected import response: {other:?}"),
        };

        assert_ne!(extract_id(first), extract_id(second));
        let games = database
            .lock()
            .await
            .list_games()
            .expect("list imported games");
        assert_eq!(games.len(), 2);
    }

    #[tokio::test]
    async fn invalid_game_metadata_is_rejected_without_partial_database_write() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let response = round_trip(
            &database,
            &state,
            IpcCommand::AddGame {
                name: "   ".to_owned(),
                platform: "Steam".to_owned(),
                install_path: "/games/invalid".to_owned(),
                executable: None,
            },
        )
        .await;

        assert!(
            response.result.is_err(),
            "invalid metadata must be rejected"
        );
        assert!(
            database
                .lock()
                .await
                .list_games()
                .expect("list games")
                .is_empty(),
            "invalid metadata must not create a database row"
        );
    }

    #[tokio::test]
    async fn concurrent_detected_game_imports_with_same_platform_identity_are_idempotent() {
        let database = Arc::new(Mutex::new(
            Database::open(":memory:").expect("database opens"),
        ));
        let state = Arc::new(AtomicU8::new(super::STATE_RUNNING));

        let first = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Original name".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("570".to_owned()),
                install_path: "/games/original".to_owned(),
                executable: None,
            },
        );
        let second = round_trip(
            &database,
            &state,
            IpcCommand::AddDetectedGame {
                name: "Second observation".to_owned(),
                platform: "steam".to_owned(),
                platform_id: Some("570".to_owned()),
                install_path: "/games/second-observation".to_owned(),
                executable: None,
            },
        );

        let (first, second) = tokio::join!(first, second);
        let extract_game =
            |response: IpcResponse| match response.result.expect("concurrent import succeeds") {
                amethys_ipc::IpcResponsePayload::GameAdded { game } => game,
                other => panic!("unexpected import response: {other:?}"),
            };
        let first_game = extract_game(first);
        let second_game = extract_game(second);

        assert_eq!(first_game.id, second_game.id);
        let games = database
            .lock()
            .await
            .list_games()
            .expect("list imported games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, first_game.id);
    }
}
