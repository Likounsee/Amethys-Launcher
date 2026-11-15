use amethys_errors::{AmethysError, ErrorCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

/// Current IPC protocol version.
pub const PROTOCOL_VERSION: u16 = 1;
/// Maximum accepted serialized message size (8 MiB).
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// Versioned request/response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub payload: T,
}

impl<T> IpcEnvelope<T> {
    #[must_use]
    pub fn new(payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            payload,
        }
    }

    pub fn validate_version(&self) -> Result<(), AmethysError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(AmethysError::new(
                ErrorCode::IpcUnsupportedVersion,
                format!(
                    "Unsupported protocol version {}; expected {}.",
                    self.protocol_version, PROTOCOL_VERSION
                ),
            ));
        }
        Ok(())
    }
}

/// Writes one JSON message prefixed by its 32-bit big-endian byte length.
pub async fn write_message<W, T>(writer: &mut W, message: &T) -> Result<(), AmethysError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(message)
        .map_err(|error| AmethysError::new(ErrorCode::IpcSerialization, error.to_string()))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
        return Err(AmethysError::new(
            ErrorCode::IpcFrameTooLarge,
            "IPC frame size is outside the accepted range.",
        ));
    }
    let length = u32::try_from(payload.len())
        .map_err(|error| AmethysError::new(ErrorCode::IpcFrameTooLarge, error.to_string()))?;
    writer.write_u32(length).await.map_err(io_error)?;
    writer.write_all(&payload).await.map_err(io_error)?;
    writer.flush().await.map_err(io_error)?;
    Ok(())
}

/// Reads and deserializes one length-prefixed JSON message.
pub async fn read_message<R, T>(reader: &mut R) -> Result<T, AmethysError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let length = reader.read_u32().await.map_err(io_error)? as usize;
    if length == 0 {
        return Err(AmethysError::new(
            ErrorCode::IpcInvalidLength,
            "An IPC frame cannot be empty.",
        ));
    }
    if length > MAX_FRAME_SIZE {
        return Err(AmethysError::new(
            ErrorCode::IpcFrameTooLarge,
            "IPC frame exceeds the configured limit.",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await.map_err(io_error)?;
    serde_json::from_slice(&payload)
        .map_err(|error| AmethysError::new(ErrorCode::IpcDeserialization, error.to_string()))
}

fn io_error(error: std::io::Error) -> AmethysError {
    AmethysError::new(ErrorCode::IpcIo, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{IpcEnvelope, PROTOCOL_VERSION, read_message, write_message};
    use amethys_errors::ErrorCode;
    use serde::{Deserialize, Serialize};
    use tokio::io::duplex;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Payload {
        command: String,
    }

    #[tokio::test]
    async fn framed_message_round_trips() {
        let (mut writer, mut reader) = duplex(1024);
        let outgoing = IpcEnvelope::new(Payload {
            command: "ping".to_owned(),
        });
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
        let mut envelope = IpcEnvelope::new(Payload {
            command: "ping".to_owned(),
        });
        envelope.protocol_version = PROTOCOL_VERSION + 1;

        let error = envelope.validate_version().expect_err("version must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcUnsupportedVersion);
    }
}
