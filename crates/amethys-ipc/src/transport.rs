//! Native local IPC server and client helpers.
//!
//! Unix-domain sockets are used on Linux/macOS and named pipes on Windows.
//! Unix sockets are restricted to the owning user (0600). Windows named pipes
//! use an explicit DACL granting access only to the process user and SYSTEM;
//! remote clients are rejected and client opens use identification-level QoS.
//! Callers must validate the peer's protocol and authorize every command.

use crate::{AmethysError, ErrorCode};
use tokio::io::{AsyncRead, AsyncWrite};

pub trait IpcStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IpcStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedIpcStream = Box<dyn IpcStream>;

#[cfg(unix)]
pub struct IpcListener {
    listener: tokio::net::UnixListener,
    socket_path: std::path::PathBuf,
    socket_device: u64,
    socket_inode: u64,
}

#[cfg(windows)]
pub struct IpcListener {
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    endpoint: String,
    dacl: weida_winpipe::OwnerOnlyDacl,
}

#[cfg(unix)]
impl Drop for IpcListener {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        // Only unlink the socket inode created by this listener. If the path
        // was replaced while the listener was alive, leave the replacement
        // untouched rather than deleting an unrelated file or symlink.
        let Ok(metadata) = std::fs::symlink_metadata(&self.socket_path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.socket_device
            && metadata.ino() == self.socket_inode
        {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(unix)]
async fn remove_stale_socket(path: &std::path::Path) -> Result<(), AmethysError> {
    use std::os::unix::fs::FileTypeExt;

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };

    if !metadata.file_type().is_socket() {
        return Err(io_error(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!(
                "IPC endpoint {} exists and is not a Unix socket",
                path.display()
            ),
        )));
    }

    match tokio::net::UnixStream::connect(path).await {
        Ok(stream) => {
            drop(stream);
            Err(io_error(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "Another Amethys process is already listening on {}",
                    path.display()
                ),
            )))
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            // The pathname may have been replaced while the asynchronous
            // connection probe was in progress. Re-check the socket identity
            // before unlinking so we never delete a replacement file/socket.
            use std::os::unix::fs::MetadataExt;
            let current = match std::fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(io_error(error)),
            };
            if !current.file_type().is_socket()
                || current.dev() != metadata.dev()
                || current.ino() != metadata.ino()
            {
                return Err(io_error(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!(
                        "IPC endpoint {} changed during stale-socket recovery",
                        path.display()
                    ),
                )));
            }
            std::fs::remove_file(path).map_err(io_error)?;
            Ok(())
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
impl IpcListener {
    /// Binds a local socket and restricts its filesystem permissions to 0600.
    pub async fn bind(endpoint: impl AsRef<std::path::Path>) -> Result<Self, AmethysError> {
        use std::os::unix::fs::PermissionsExt;

        let socket_path = endpoint.as_ref().to_path_buf();
        remove_stale_socket(&socket_path).await?;
        let listener = tokio::net::UnixListener::bind(&socket_path).map_err(io_error)?;
        let metadata = std::fs::symlink_metadata(&socket_path).map_err(io_error)?;
        if let Err(error) =
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        {
            // The listener is still local to this function. Remove only the
            // inode we just bound, never a path that may have been replaced.
            use std::os::unix::fs::MetadataExt;
            if let Ok(current) = std::fs::symlink_metadata(&socket_path) {
                if current.dev() == metadata.dev() && current.ino() == metadata.ino() {
                    drop(listener);
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
            return Err(io_error(error));
        }
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            listener,
            socket_path,
            socket_device: metadata.dev(),
            socket_inode: metadata.ino(),
        })
    }

    /// Accepts the next client connection. The same listener can be reused.
    pub async fn accept(&mut self) -> Result<BoxedIpcStream, AmethysError> {
        let (stream, _) = self.listener.accept().await.map_err(io_error)?;
        Ok(Box::new(stream))
    }
}

#[cfg(windows)]
impl IpcListener {
    /// Creates one local named-pipe server instance.
    pub async fn bind(endpoint: impl AsRef<str>) -> Result<Self, AmethysError> {
        let dacl = weida_winpipe::OwnerOnlyDacl::for_current_user().map_err(|error| {
            AmethysError::new(
                ErrorCode::IpcIo,
                format!("Could not create the current-user named-pipe security descriptor: {error}"),
            )
        })?;
        let endpoint = endpoint.as_ref().to_owned();
        let server = weida_winpipe::create_instance(std::ffi::OsStr::new(&endpoint), &dacl, true)
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::IpcIo,
                    format!("Could not create Windows named pipe '{endpoint}': {error}"),
                )
            })?;
        Ok(Self {
            server,
            endpoint,
            dacl,
        })
    }

    /// Accepts one client connection.
    pub async fn accept(&mut self) -> Result<BoxedIpcStream, AmethysError> {
        self.server.connect().await.map_err(io_error)?;
        let next_server =
            weida_winpipe::create_instance(std::ffi::OsStr::new(&self.endpoint), &self.dacl, false)
                .map_err(io_error)?;
        let connected_server = std::mem::replace(&mut self.server, next_server);
        Ok(Box::new(connected_server))
    }
}

/// Connects to a Unix-domain socket path on Linux or macOS.
#[cfg(unix)]
pub async fn connect(
    endpoint: impl AsRef<std::path::Path>,
) -> Result<BoxedIpcStream, AmethysError> {
    tokio::net::UnixStream::connect(endpoint)
        .await
        .map(|stream| Box::new(stream) as BoxedIpcStream)
        .map_err(io_error)
}

/// Connects to a Windows named pipe, for example \\.\pipe\amethys-core.
#[cfg(windows)]
pub async fn connect(endpoint: impl AsRef<str>) -> Result<BoxedIpcStream, AmethysError> {
    // The helper opens the client with identification-level QoS, preventing
    // the server from impersonating this client beyond identity inspection.
    // A named pipe can briefly have no listening instance while the server
    // accepts a client and creates its replacement instance. Windows reports
    // that race as ERROR_PIPE_BUSY; retry it instead of failing the whole IPC
    // request immediately.
    let endpoint = endpoint.as_ref().to_owned();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match weida_winpipe::open_client(std::ffi::OsStr::new(&endpoint)) {
            Ok(stream) => return Ok(Box::new(stream) as BoxedIpcStream),
            Err(error)
                if weida_winpipe::is_pipe_busy(&error)
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => return Err(io_error(error)),
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub struct IpcListener;

#[cfg(not(any(unix, windows)))]
impl IpcListener {
    pub async fn bind(_endpoint: &str) -> Result<Self, AmethysError> {
        Err(unsupported_platform())
    }

    pub async fn accept(&mut self) -> Result<BoxedIpcStream, AmethysError> {
        Err(unsupported_platform())
    }
}

#[cfg(not(any(unix, windows)))]
pub async fn connect(_endpoint: &str) -> Result<BoxedIpcStream, AmethysError> {
    Err(unsupported_platform())
}

fn io_error(error: std::io::Error) -> AmethysError {
    AmethysError::new(ErrorCode::IpcIo, error.to_string())
}

#[cfg(not(any(unix, windows)))]
fn unsupported_platform() -> AmethysError {
    AmethysError::new(
        ErrorCode::IpcIo,
        "Native IPC transport is not supported on this operating system.",
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::{IpcListener, connect};
    use std::os::unix::fs::FileTypeExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn listener_removes_a_stale_socket_before_binding() {
        let path = std::path::PathBuf::from(format!(
            "/tmp/amethys-ipc-stale-{}.sock",
            uuid::Uuid::new_v4()
        ));
        {
            let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
            drop(stale);
        }

        let listener = IpcListener::bind(&path)
            .await
            .expect("replace stale socket");
        assert!(
            std::fs::symlink_metadata(&path)
                .expect("new socket exists")
                .file_type()
                .is_socket()
        );
        drop(listener);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn dropping_listener_does_not_remove_a_replacement_path() {
        let path = std::path::PathBuf::from(format!(
            "/tmp/amethys-ipc-replaced-{}.sock",
            uuid::Uuid::new_v4()
        ));
        let moved_path = path.with_extension("original.sock");
        let listener = IpcListener::bind(&path).await.expect("listener binds");

        std::fs::rename(&path, &moved_path).expect("move original socket path");
        std::fs::write(&path, b"replacement sentinel").expect("create replacement file");
        drop(listener);

        assert_eq!(
            std::fs::read(&path).expect("replacement file remains"),
            b"replacement sentinel"
        );
        std::fs::remove_file(path).expect("remove replacement file");
        std::fs::remove_file(moved_path).expect("remove moved socket");
    }

    #[tokio::test]
    async fn listener_refuses_to_replace_a_regular_file() {
        let path =
            std::env::temp_dir().join(format!("amethys-ipc-file-{}.sock", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"keep this file").expect("create sentinel file");

        let result = IpcListener::bind(&path).await;
        assert!(
            result.is_err(),
            "regular files must not be treated as stale sockets"
        );
        assert_eq!(
            std::fs::read(&path).expect("sentinel file remains"),
            b"keep this file"
        );

        std::fs::remove_file(path).expect("remove sentinel file");
    }

    #[tokio::test]
    async fn listener_does_not_replace_an_active_socket() {
        let path = std::path::PathBuf::from(format!(
            "/tmp/amethys-ipc-active-{}.sock",
            uuid::Uuid::new_v4()
        ));
        let mut first = IpcListener::bind(&path)
            .await
            .expect("first listener binds");
        let error = match IpcListener::bind(&path).await {
            Ok(_) => panic!("second listener must not replace the active socket"),
            Err(error) => error,
        };
        assert_eq!(error.code(), crate::ErrorCode::IpcIo);

        let client_path = path.clone();
        let client_task = tokio::spawn(async move {
            let _client = connect(client_path)
                .await
                .expect("first listener remains reachable");
        });
        let _accepted = first.accept().await.expect("first listener accepts client");
        client_task.await.expect("client task");
    }

    #[tokio::test]
    async fn listener_accepts_multiple_clients_without_rebinding() {
        let path =
            std::path::PathBuf::from(format!("/tmp/amethys-ipc-{}.sock", uuid::Uuid::new_v4()));
        let mut listener = IpcListener::bind(&path).await.expect("bind socket");

        for expected in [b"one".as_slice(), b"two".as_slice()] {
            let client_path = path.clone();
            let client_task = tokio::spawn(async move {
                let mut client = connect(client_path).await.expect("connect");
                client.write_all(expected).await.expect("send");
            });

            let mut server = listener.accept().await.expect("accept client");
            let mut received = vec![0; expected.len()];
            server
                .read_exact(&mut received)
                .await
                .expect("read client data");
            assert_eq!(received, expected);
            client_task.await.expect("client task");
        }
    }

    #[tokio::test]
    async fn protocol_request_round_trips_over_native_socket() {
        use crate::{IpcCommand, IpcCoreStatus, IpcRequest, IpcResponsePayload, send_request};

        let path =
            std::path::PathBuf::from(format!("/tmp/amethys-ipc-{}.sock", uuid::Uuid::new_v4()));
        let mut listener = IpcListener::bind(&path).await.expect("bind socket");

        let server_task = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("accept client");
            crate::serve_connection(&mut stream, IpcCoreStatus::Running)
                .await
                .expect("serve protocol request");
        });

        let mut client = connect(&path).await.expect("connect client");
        let response = send_request(&mut client, IpcRequest::new(IpcCommand::GetStatus))
            .await
            .expect("send protocol request");

        assert_eq!(
            response.result,
            Ok(IpcResponsePayload::CoreStatus {
                state: IpcCoreStatus::Running,
            }),
        );
        server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn client_and_server_exchange_bytes_over_private_socket() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("amethys-ipc-{}.sock", uuid::Uuid::new_v4()));
        let listener = IpcListener::bind(&path).await.expect("bind socket");
        let mode = std::fs::metadata(&path)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let client_path = path.clone();
        let client_task = tokio::spawn(async move {
            let mut client = connect(client_path).await.expect("connect");
            client.write_all(b"ping").await.expect("send");
            let mut response = [0_u8; 4];
            client.read_exact(&mut response).await.expect("receive");
            response
        });

        let mut listener = listener;
        let mut server = listener.accept().await.expect("accept");
        let mut request = [0_u8; 4];
        server.read_exact(&mut request).await.expect("read request");
        assert_eq!(&request, b"ping");
        server.write_all(b"pong").await.expect("write response");
        assert_eq!(&client_task.await.expect("client task"), b"pong");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::{IpcListener, connect};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_endpoint() -> String {
        format!(r"\\.\pipe\amethys-test-{}", uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn named_pipe_accepts_a_client_and_exchanges_bytes() {
        let endpoint = test_endpoint();
        let mut listener = IpcListener::bind(&endpoint).await.expect("bind named pipe");

        let client_endpoint = endpoint.clone();
        let client_task = tokio::spawn(async move {
            let mut client = connect(client_endpoint).await.expect("connect client");
            client.write_all(b"ping").await.expect("send request");
            let mut response = [0_u8; 4];
            client
                .read_exact(&mut response)
                .await
                .expect("read response");
            response
        });

        let mut server = listener.accept().await.expect("accept client");
        let mut request = [0_u8; 4];
        server.read_exact(&mut request).await.expect("read request");
        assert_eq!(&request, b"ping");
        server.write_all(b"pong").await.expect("write response");
        assert_eq!(&client_task.await.expect("client task"), b"pong");
    }

    #[tokio::test]
    async fn named_pipe_peer_identity_matches_the_current_user_and_process() {
        let endpoint = test_endpoint();
        let dacl =
            weida_winpipe::OwnerOnlyDacl::for_current_user().expect("build current-user-only DACL");
        let mut server =
            weida_winpipe::create_instance(std::ffi::OsStr::new(&endpoint), &dacl, true)
                .expect("create protected named pipe");

        let client_endpoint = endpoint.clone();
        let client_task = tokio::spawn(async move {
            let mut client = weida_winpipe::open_client(std::ffi::OsStr::new(&client_endpoint))
                .expect("open protected pipe");
            client
                .write_all(b"identity")
                .await
                .expect("send identity probe");
            weida_winpipe::server_peer(&client).expect("read server identity")
        });

        server.connect().await.expect("accept protected client");
        let mut probe = [0_u8; 8];
        server
            .read_exact(&mut probe)
            .await
            .expect("read identity probe");
        assert_eq!(&probe, b"identity");

        let client_peer =
            weida_winpipe::client_peer(&server).expect("read authenticated client identity");
        let server_peer = client_task.await.expect("client task");

        assert_eq!(client_peer.sid, server_peer.sid);
        assert_eq!(client_peer.pid, std::process::id());
        assert_eq!(server_peer.pid, std::process::id());
    }
    #[tokio::test]
    async fn named_pipe_listener_accepts_multiple_clients() {
        let endpoint = test_endpoint();
        let mut listener = IpcListener::bind(&endpoint).await.expect("bind named pipe");

        for expected in [b"one".as_slice(), b"two".as_slice()] {
            let client_endpoint = endpoint.clone();
            let client_task = tokio::spawn(async move {
                let mut client = connect(client_endpoint).await.expect("connect client");
                client.write_all(expected).await.expect("send request");
            });

            let mut server = listener.accept().await.expect("accept client");
            let mut received = vec![0_u8; expected.len()];
            server
                .read_exact(&mut received)
                .await
                .expect("read request");
            assert_eq!(received, expected);
            client_task.await.expect("client task");
        }
    }
}
