use amethys_errors::{AmethysError, ErrorCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// Commands that a local client may request from the core agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcCommand {
    /// Checks that the core agent is reachable.
    Ping,
    /// Requests the current lifecycle state of the core.
    GetStatus,
}

/// A request sent from a local client to the core agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcRequest {
    pub command: IpcCommand,
}

impl IpcRequest {
    #[must_use]
    pub const fn new(command: IpcCommand) -> Self {
        Self { command }
    }
}

/// Lifecycle states exposed through the local IPC status command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcCoreStatus {
    Created,
    Running,
    Stopping,
    Stopped,
}

/// Successful results returned by the core agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum IpcResponsePayload {
    Pong,
    CoreStatus { state: IpcCoreStatus },
}

/// A safe, serializable error returned by the remote core process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcRemoteError {
    pub code: String,
    pub message: String,
}

impl IpcRemoteError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Response correlated with the identifier of the original request envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcResponse {
    pub request_id: Uuid,
    pub result: Result<IpcResponsePayload, IpcRemoteError>,
}

impl IpcResponse {
    #[must_use]
    pub const fn success(request_id: Uuid, payload: IpcResponsePayload) -> Self {
        Self {
            request_id,
            result: Ok(payload),
        }
    }

    #[must_use]
    pub const fn failure(request_id: Uuid, error: IpcRemoteError) -> Self {
        Self {
            request_id,
            result: Err(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub payload: T,
}

impl<T> IpcEnvelope<T> {
    #[must_use]
    pub fn new(payload: T) -> Self {
        Self { protocol_version: PROTOCOL_VERSION, request_id: Uuid::new_v4(), payload }
    }

    pub fn validate_version(&self) -> Result<(), AmethysError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(AmethysError::new(ErrorCode::IpcUnsupportedVersion, format!(
                "Unsupported protocol version {}; expected {}.",
                self.protocol_version, PROTOCOL_VERSION
            )));
        }
        Ok(())
    }
}

pub async fn write_message<W, T>(writer: &mut W, message: &T) -> Result<(), AmethysError>
where W: AsyncWrite + Unpin, T: Serialize {
    let payload = serde_json::to_vec(message)
        .map_err(|error| AmethysError::new(ErrorCode::IpcSerialization, error.to_string()))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
        return Err(AmethysError::new(ErrorCode::IpcFrameTooLarge, "IPC frame size is outside the accepted range."));
    }
    let length = u32::try_from(payload.len())
        .map_err(|error| AmethysError::new(ErrorCode::IpcFrameTooLarge, error.to_string()))?;
    writer.write_u32(length).await.map_err(io_error)?;
    writer.write_all(&payload).await.map_err(io_error)?;
    writer.flush().await.map_err(io_error)?;
    Ok(())
}

pub async fn read_message<R, T>(reader: &mut R) -> Result<T, AmethysError>
where R: AsyncRead + Unpin, T: DeserializeOwned {
    let length = reader.read_u32().await.map_err(io_error)? as usize;
    if length == 0 {
        return Err(AmethysError::new(ErrorCode::IpcInvalidLength, "An IPC frame cannot be empty."));
    }
    if length > MAX_FRAME_SIZE {
        return Err(AmethysError::new(ErrorCode::IpcFrameTooLarge, "IPC frame exceeds the configured limit."));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await.map_err(io_error)?;
    serde_json::from_slice(&payload)
        .map_err(|error| AmethysError::new(ErrorCode::IpcDeserialization, error.to_string()))
}

/// Reads an envelope and rejects protocol versions this build cannot handle.
pub async fn read_envelope<R, T>(reader: &mut R) -> Result<IpcEnvelope<T>, AmethysError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let envelope: IpcEnvelope<T> = read_message(reader).await?;
    envelope.validate_version()?;
    Ok(envelope)
}

fn io_error(error: std::io::Error) -> AmethysError {
    AmethysError::new(ErrorCode::IpcIo, error.to_string())
}

pub mod transport;

#[cfg(test)]
mod tests {
    use super::{IpcEnvelope, PROTOCOL_VERSION, read_message, write_message};
    use amethys_errors::ErrorCode;
    use serde::{Deserialize, Serialize};
    use tokio::io::duplex;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Payload { command: String }

    #[test]
    fn ipc_response_keeps_original_request_identifier() {
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::Ping));
        let response = super::IpcResponse::success(
            request.request_id,
            super::IpcResponsePayload::Pong,
        );

        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.result, Ok(super::IpcResponsePayload::Pong));
    }

    #[test]
    fn ipc_request_and_response_round_trip_through_json() {
        let request = super::IpcRequest::new(super::IpcCommand::GetStatus);
        let encoded = serde_json::to_vec(&request).expect("serialize request");
        let decoded: super::IpcRequest =
            serde_json::from_slice(&encoded).expect("deserialize request");
        assert_eq!(decoded, request);

        let response = super::IpcResponse::failure(
            Uuid::new_v4(),
            super::IpcRemoteError::new("AM-CORE-0001", "Core is not running."),
        );
        let encoded = serde_json::to_vec(&response).expect("serialize response");
        let decoded: super::IpcResponse =
            serde_json::from_slice(&encoded).expect("deserialize response");
        assert_eq!(decoded, response);
    }


    #[tokio::test]
    async fn framed_message_round_trips() {
        let (mut writer, mut reader) = duplex(1024);
        let outgoing = IpcEnvelope::new(Payload { command: "ping".to_owned() });
        let expected = outgoing.clone();
        let send = tokio::spawn(async move {
            write_message(&mut writer, &outgoing).await.expect("write");
        });
        let received: IpcEnvelope<Payload> = read_message(&mut reader).await.expect("read");
        send.await.expect("writer task");
        assert_eq!(received, expected);
    }

    #[test]
    fn unsupported_protocol_version_has_specific_error_code() {
        let mut envelope = IpcEnvelope::new(Payload { command: "ping".to_owned() });
        envelope.protocol_version = PROTOCOL_VERSION + 1;
        let error = envelope.validate_version().expect_err("version must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcUnsupportedVersion);
    }

    #[tokio::test]
    async fn read_envelope_rejects_unsupported_protocol_version() {
        let (mut writer, mut reader) = duplex(1024);
        let mut outgoing = IpcEnvelope::new(Payload { command: "ping".to_owned() });
        outgoing.protocol_version = PROTOCOL_VERSION + 1;
        let send = tokio::spawn(async move {
            write_message(&mut writer, &outgoing).await.expect("write");
        });

        let error = super::read_envelope::<_, Payload>(&mut reader)
            .await
            .expect_err("unsupported version must fail during read");
        send.await.expect("writer task");
        assert_eq!(error.code(), ErrorCode::IpcUnsupportedVersion);
    }

    #[tokio::test]
    async fn read_message_rejects_empty_frame() {
        let (mut writer, mut reader) = duplex(8);
        let send = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_u32(&mut writer, 0)
                .await
                .expect("write empty frame header");
        });

        let error = super::read_message::<_, serde_json::Value>(&mut reader)
            .await
            .expect_err("empty frame must fail");
        send.await.expect("writer task");
        assert_eq!(error.code(), ErrorCode::IpcInvalidLength);
    }

    #[tokio::test]
    async fn read_message_rejects_frame_larger_than_limit_before_allocating() {
        let (mut writer, mut reader) = duplex(8);
        let send = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_u32(
                &mut writer,
                (super::MAX_FRAME_SIZE as u32) + 1,
            )
            .await
            .expect("write oversized frame header");
        });

        let error = super::read_message::<_, serde_json::Value>(&mut reader)
            .await
            .expect_err("oversized frame must fail");
        send.await.expect("writer task");
        assert_eq!(error.code(), ErrorCode::IpcFrameTooLarge);
    }
}
