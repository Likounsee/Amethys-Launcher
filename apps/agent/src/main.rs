use amethys_events::EventBus;
use amethys_ipc::{IpcCoreStatus, serve_connection, transport::IpcListener};
use amethys_runtime::{CoreRuntime, RuntimeState};
use amethys_storage::Database;
use std::{
    error::Error,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::task::JoinSet;

const STATE_CREATED: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_STOPPING: u8 = 2;
const STATE_STOPPED: u8 = 3;

fn data_directory() -> PathBuf {
    std::env::var_os("AMETHYS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".amethys")
        })
}

fn ipc_status(state: &AtomicU8) -> IpcCoreStatus {
    match state.load(Ordering::Acquire) {
        STATE_RUNNING => IpcCoreStatus::Running,
        STATE_STOPPING => IpcCoreStatus::Stopping,
        STATE_STOPPED => IpcCoreStatus::Stopped,
        _ => IpcCoreStatus::Created,
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

    let data_dir = data_directory();
    std::fs::create_dir_all(&data_dir)?;
    let _database = Database::open(data_dir.join("amethys.sqlite"))?;

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

    let mut connections = JoinSet::new();
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
                        let state = Arc::clone(&shared_state);
                        connections.spawn(async move {
                            let current_state = ipc_status(&state);
                            if let Err(error) = serve_connection(&mut stream, current_state).await {
                                tracing::debug!(%error, "Local IPC request failed");
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, "Unable to accept a local IPC connection");
                        break;
                    }
                }
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    tracing::warn!(%error, "IPC connection task ended unexpectedly");
                }
            }
        }
    }

    shared_state.store(STATE_STOPPING, Ordering::Release);
    connections.abort_all();
    while connections.join_next().await.is_some() {}

    runtime.shutdown()?;
    store_runtime_state(&shared_state, runtime.state());
    tracing::info!("Amethys Core stopped");
    Ok(())
}
