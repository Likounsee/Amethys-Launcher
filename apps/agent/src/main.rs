use amethys_events::EventBus;
use amethys_ipc::{IpcCommand, IpcCoreStatus, IpcEnvelope, IpcGameEntry, IpcRemoteError, IpcRequest, IpcResponse, IpcResponsePayload, read_envelope, write_message, transport::IpcListener};
use amethys_runtime::{CoreRuntime, RuntimeState};
use amethys_storage::Database;
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

const STATE_CREATED: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_STOPPING: u8 = 2;
const STATE_STOPPED: u8 = 3;

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
    state: IpcCoreStatus,
    database: &Database,
) -> Result<(), Box<dyn Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: IpcEnvelope<IpcRequest> = read_envelope(stream).await?;
    let result = match request.payload.command {
        IpcCommand::Ping => Ok(IpcResponsePayload::Pong),
        IpcCommand::GetStatus => Ok(IpcResponsePayload::CoreStatus { state }),
        IpcCommand::ListGames => database.list_games().map(|entries| {
            IpcResponsePayload::Games {
                games: entries.into_iter().map(|game| IpcGameEntry {
                    id: game.id,
                    name: game.name,
                    platform: game.platform,
                    platform_id: game.platform_id,
                    install_path: game.install_path,
                    executable: game.executable,
                }).collect(),
            }
        }),
        IpcCommand::AddGame { name, platform, install_path, executable } => {
            database.add_game(&name, &platform, &install_path, executable.as_deref()).map(|game| {
                IpcResponsePayload::GameAdded {
                    game: IpcGameEntry {
                        id: game.id,
                        name: game.name,
                        platform: game.platform,
                        platform_id: game.platform_id,
                        install_path: game.install_path,
                        executable: game.executable,
                    },
                }
            })
        },
        IpcCommand::AddDetectedGame { name, platform, platform_id, install_path, executable } => {
            database.add_game_with_platform_id(
                &name,
                &platform,
                platform_id.as_deref(),
                &install_path,
                executable.as_deref(),
            ).map(|game| {
                IpcResponsePayload::GameAdded {
                    game: IpcGameEntry {
                        id: game.id,
                        name: game.name,
                        platform: game.platform,
                        platform_id: game.platform_id,
                        install_path: game.install_path,
                        executable: game.executable,
                    },
                }
            })
        }
        IpcCommand::RemoveGame { id } => database.remove_game(&id).map(|removed| {
            IpcResponsePayload::GameRemoved { removed }
        }),
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
    let database = Database::open(data_dir.join("amethys.sqlite"))?;

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
            accepted = listener.accept() => {
                match accepted {
                    Ok(mut stream) => {
                        let current_state = ipc_status(&shared_state);
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            serve_database_connection(&mut stream, current_state, &database),
                        ).await {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, "Local IPC request failed");
                            }
                            Err(_) => {
                                tracing::warn!("Local IPC request timed out after 15 seconds");
                            }
                        }
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
    runtime.shutdown()?;
    store_runtime_state(&shared_state, runtime.state());
    tracing::info!("Amethys Core stopped");
    Ok(())
}
