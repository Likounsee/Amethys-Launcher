//! Native local IPC stream connection helpers.
//!
//! Unix-domain sockets are used on Linux/macOS and named pipes on Windows.
//! Connecting does not itself authenticate the peer or configure server ACLs.

use crate::{AmethysError, ErrorCode};
use tokio::io::{AsyncRead, AsyncWrite};

pub trait IpcStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IpcStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedIpcStream = Box<dyn IpcStream>;

/// Connects to a Unix-domain socket path on Linux or macOS.
#[cfg(unix)]
pub async fn connect(endpoint: &str) -> Result<BoxedIpcStream, AmethysError> {
    tokio::net::UnixStream::connect(endpoint)
        .await
        .map(|stream| Box::new(stream) as BoxedIpcStream)
        .map_err(|error| AmethysError::new(ErrorCode::IpcIo, error.to_string()))
}

/// Connects to a Windows named pipe, for example \\.\pipe\amethys-core.
#[cfg(windows)]
pub async fn connect(endpoint: &str) -> Result<BoxedIpcStream, AmethysError> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint)
        .map(|stream| Box::new(stream) as BoxedIpcStream)
        .map_err(|error| AmethysError::new(ErrorCode::IpcIo, error.to_string()))
}

#[cfg(not(any(unix, windows)))]
pub async fn connect(_endpoint: &str) -> Result<BoxedIpcStream, AmethysError> {
    Err(AmethysError::new(
        ErrorCode::IpcIo,
        "Native IPC transport is not supported on this operating system.",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::connect;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn connects_to_a_local_unix_socket() {
        let path = std::env::temp_dir().join(format!(
            "amethys-ipc-{}.sock",
            uuid::Uuid::new_v4()
        ));
        let listener = UnixListener::bind(&path).expect("bind test socket");
        let endpoint = path.to_string_lossy().into_owned();
        let client = connect(&endpoint).await.expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        drop(client);
        drop(server);
        drop(listener);
        std::fs::remove_file(path).expect("remove test socket");
    }
}
