use amethys_events::EventBus;
use amethys_ipc::{
    IpcCommand, IpcCoreStatus, IpcEnvelope, IpcGameEntry, IpcRemoteError, IpcRequest, IpcResponse,
    IpcResponsePayload, read_envelope, transport::IpcListener, write_message,
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
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

const STATE_CREATED: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_STOPPING: u8 = 2;
const STATE_STOPPED: u8 = 3;

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
) -> Result<(), Box<dyn Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: IpcEnvelope<IpcRequest> = read_envelope(stream).await?;
    // Read the lifecycle state when the request is handled, not when its
    // socket was accepted: a client may connect and then stall before sending.
    let state = ipc_status(shared_state);
    if !mutation_allowed(state)
        && matches!(
            &request.payload.command,
            IpcCommand::AddGame { .. }
                | IpcCommand::AddDetectedGame { .. }
                | IpcCommand::SetGameExecutable { .. }
                | IpcCommand::RemoveGame { .. }
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
    let result = match request.payload.command {
        IpcCommand::Ping => Ok(IpcResponsePayload::Pong),
        IpcCommand::GetStatus => Ok(IpcResponsePayload::CoreStatus { state }),
        command => {
            let database = database.lock().await;
            match command {
                IpcCommand::ListGames => {
                    database
                        .list_games()
                        .map(|entries| IpcResponsePayload::Games {
                            games: entries
                                .into_iter()
                                .map(|game| IpcGameEntry {
                                    id: game.id,
                                    name: game.name,
                                    platform: game.platform,
                                    platform_id: game.platform_id,
                                    install_path: game.install_path,
                                    executable: game.executable,
                                })
                                .collect(),
                        })
                }
                IpcCommand::AddGame {
                    name,
                    platform,
                    install_path,
                    executable,
                } => database
                    .add_game(&name, &platform, &install_path, executable.as_deref())
                    .map(|game| IpcResponsePayload::GameAdded {
                        game: IpcGameEntry {
                            id: game.id,
                            name: game.name,
                            platform: game.platform,
                            platform_id: game.platform_id,
                            install_path: game.install_path,
                            executable: game.executable,
                        },
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
                                None => database.add_game_with_platform_id(
                                    &name,
                                    &platform,
                                    Some(platform_id),
                                    &install_path,
                                    executable.as_deref(),
                                ),
                            })
                    } else {
                        database.add_game_with_platform_id(
                            &name,
                            &platform,
                            None,
                            &install_path,
                            executable.as_deref(),
                        )
                    };
                    game.map(|game| IpcResponsePayload::GameAdded {
                        game: IpcGameEntry {
                            id: game.id,
                            name: game.name,
                            platform: game.platform,
                            platform_id: game.platform_id,
                            install_path: game.install_path,
                            executable: game.executable,
                        },
                    })
                }
                IpcCommand::SetGameExecutable { id, executable } => database
                    .set_game_executable(&id, &executable)
                    .and_then(|game| {
                        game.map(|game| IpcResponsePayload::GameAdded {
                            game: IpcGameEntry {
                                id: game.id,
                                name: game.name,
                                platform: game.platform,
                                platform_id: game.platform_id,
                                install_path: game.install_path,
                                executable: game.executable,
                            },
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
                IpcCommand::Ping | IpcCommand::GetStatus => unreachable!("handled above"),
            }
        }
    };
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
                        connections.spawn(async move {
                            let _permit = permit;
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(15),
                                serve_database_connection(&mut stream, &shared_state, &database),
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
    use super::{STATE_STOPPING, mutation_allowed, serve_database_connection};
    use amethys_ipc::{
        IpcCommand, IpcCoreStatus, IpcEnvelope, IpcRequest, IpcResponse, read_message,
        write_message,
    };
    use amethys_storage::Database;
    use std::sync::{Arc, atomic::AtomicU8};
    use tokio::{io::duplex, sync::Mutex};

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
        let server_task = tokio::spawn(async move {
            serve_database_connection(&mut server, &server_state, &server_database)
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
        let server_task = tokio::spawn(async move {
            serve_database_connection(&mut server, &server_state, &server_database)
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

        let listed = round_trip(&database, &state, IpcCommand::ListGames).await;
        let games = match listed.result.expect("game library is listed") {
            amethys_ipc::IpcResponsePayload::Games { games } => games,
            other => panic!("unexpected list response: {other:?}"),
        };
        assert_eq!(games, vec![added_game.clone()]);

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
}
