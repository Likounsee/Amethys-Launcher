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
    /// Lists games saved in the local library.
    ListGames,
    /// Lists games the user owns, regardless of installation state.
    ListOwnedGames,
    AddOwnedGame {
        amethys_id: u64,
        name: String,
        genres: Vec<String>,
    },
    RemoveOwnedGame {
        amethys_id: u64,
    },
    /// Adds a game to the local library.
    AddGame {
        name: String,
        platform: String,
        install_path: String,
        executable: Option<String>,
    },
    /// Adds a detected game while retaining its source-platform identifier.
    AddDetectedGame {
        name: String,
        platform: String,
        platform_id: Option<String>,
        install_path: String,
        executable: Option<String>,
    },
    /// Configures the executable path for a game already in the library.
    SetGameExecutable {
        id: String,
        executable: String,
    },
    /// Refreshes the public master catalogue index from its configured provider.
    SyncMasterCatalog,
    /// Searches the locally cached master catalogue index.
    SearchMasterCatalog {
        query: String,
        limit: usize,
    },
    /// Recommends catalogue games using genres and titles from the local library.
    SearchRecommendedCatalog {
        genres: Vec<String>,
        owned_ids: Vec<u64>,
        owned_names: Vec<String>,
        limit: usize,
    },
    /// Loads one master-catalogue game detail by its stable Amethys ID.
    GetMasterCatalogGame {
        amethys_id: u64,
    },
    /// Removes a game by its identifier.
    RemoveGame {
        id: String,
    },
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
    CoreStatus {
        state: IpcCoreStatus,
    },
    Games {
        games: Vec<IpcGameEntry>,
    },
    OwnedGames {
        games: Vec<IpcOwnedGameEntry>,
    },
    OwnedGameAdded {
        game: IpcOwnedGameEntry,
    },
    OwnedGameRemoved {
        removed: bool,
    },
    GameAdded {
        game: Box<IpcGameEntry>,
    },
    GameRemoved {
        removed: bool,
    },
    CatalogSynchronized {
        catalog_version: String,
        game_count: u64,
    },
    CatalogSearchResults {
        games: Vec<IpcCatalogSearchEntry>,
    },
    CatalogGameDetails {
        game: IpcCatalogGameDetail,
    },
}

/// Canonical metadata shared by all local installations of one Amethys game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IpcGameMetadata {
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub release_date: Option<String>,
    pub description: Option<String>,
}

/// A game summary transferred from the core to a local client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcGameEntry {
    pub id: String,
    pub name: String,
    pub platform: String,
    #[serde(default)]
    pub platform_id: Option<String>,
    /// Stable Amethys catalogue identity shared by linked installations.
    #[serde(default)]
    pub catalogue_game_id: Option<String>,
    /// Catalogue metadata is optional for older databases and incomplete entries.
    #[serde(default)]
    pub catalogue_metadata: Option<IpcGameMetadata>,
    pub install_path: String,
    /// All known install directories; the primary path remains in install_path.
    #[serde(default)]
    pub install_locations: Vec<String>,
    pub executable: Option<String>,
}

/// A game owned by the user, independently from its local installation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcOwnedGameEntry {
    pub amethys_id: u64,
    pub name: String,
    pub genres: Vec<String>,
    pub added_at: String,
}

/// A compact entry from the public master catalogue search index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcCatalogSearchEntry {
    pub amethys_id: u64,
    pub name: String,
    pub slug: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Optional cover URL from the catalogue search index.
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Optional path to a validated local cover cache copy.
    #[serde(default)]
    pub cover_cache_path: Option<String>,
}

/// A provider-independent source identity attached to a master-catalogue game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcCatalogSourceIdentity {
    pub source: String,
    pub external_id: String,
}

/// Full game details from the public master catalogue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcCatalogGameDetail {
    pub amethys_id: u64,
    /// Optional ID from the current metadata ingestion provider.
    #[serde(default)]
    pub igdb_id: Option<u64>,
    pub name: String,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    /// Optional path to a validated local cache copy of the cover image.
    #[serde(default)]
    pub cover_cache_path: Option<String>,
    pub genres: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<IpcCatalogSourceIdentity>,
    pub stores: Vec<IpcCatalogStoreIdentity>,
}

/// External store identity attached to a master-catalogue game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcCatalogStoreIdentity {
    pub store: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
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

/// Converts a validated request into its matching response.
///
/// The caller supplies the current core state so this crate stays independent
/// from the runtime implementation.
#[must_use]
pub fn dispatch_request(request: IpcEnvelope<IpcRequest>, state: IpcCoreStatus) -> IpcResponse {
    dispatch_request_with_games(request, state, &[])
}

/// Dispatches a request with a snapshot of the locally stored game library.
#[must_use]
pub fn dispatch_request_with_games(
    request: IpcEnvelope<IpcRequest>,
    state: IpcCoreStatus,
    games: &[IpcGameEntry],
) -> IpcResponse {
    let payload = match request.payload.command {
        IpcCommand::Ping => IpcResponsePayload::Pong,
        IpcCommand::GetStatus => IpcResponsePayload::CoreStatus { state },
        IpcCommand::ListGames => IpcResponsePayload::Games {
            games: games.to_vec(),
        },
        IpcCommand::AddGame { .. }
        | IpcCommand::AddDetectedGame { .. }
        | IpcCommand::SetGameExecutable { .. }
        | IpcCommand::RemoveGame { .. }
        | IpcCommand::ListOwnedGames
        | IpcCommand::AddOwnedGame { .. }
        | IpcCommand::RemoveOwnedGame { .. }
        | IpcCommand::SyncMasterCatalog
        | IpcCommand::SearchMasterCatalog { .. }
        | IpcCommand::SearchRecommendedCatalog { .. }
        | IpcCommand::GetMasterCatalogGame { .. } => {
            return IpcResponse::failure(
                request.request_id,
                IpcRemoteError::new(
                    "AM-IPC-0008",
                    "Game changes must be handled by the storage-aware core service.",
                ),
            );
        }
    };
    IpcResponse::success(request.request_id, payload)
}

/// Sends one request and verifies that the response belongs to it.
pub async fn send_request<S>(
    stream: &mut S,
    request: IpcRequest,
) -> Result<IpcResponse, AmethysError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let envelope = IpcEnvelope::new(request);
    let request_id = envelope.request_id;
    write_message(stream, &envelope).await?;

    let response: IpcResponse = read_message(stream).await?;
    if response.request_id != request_id {
        return Err(AmethysError::new(
            ErrorCode::IpcResponseMismatch,
            "The IPC response does not match the request that was sent.",
        ));
    }

    Ok(response)
}

/// Handles one request on an already accepted local connection.
///
/// A connection carries one request and one response. The listener owner is
/// responsible for accepting additional clients and calling this function
/// for each connection.
pub async fn serve_connection<S>(stream: &mut S, state: IpcCoreStatus) -> Result<(), AmethysError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    serve_connection_with_games(stream, state, &[]).await
}

/// Handles one request using the current game-library snapshot.
pub async fn serve_connection_with_games<S>(
    stream: &mut S,
    state: IpcCoreStatus,
    games: &[IpcGameEntry],
) -> Result<(), AmethysError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request: IpcEnvelope<IpcRequest> = read_envelope(stream).await?;
    let response = dispatch_request_with_games(request, state, games);
    write_message(stream, &response).await
}

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

/// Matches all query terms against the local installation and its linked catalogue metadata.
///
/// An empty or whitespace-only query matches every game. Callers that want an
/// empty result for an empty query should handle that UI/command policy themselves.
#[must_use]
pub fn game_matches_search_query(game: &IpcGameEntry, query: &str) -> bool {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return true;
    }

    let metadata = game.catalogue_metadata.as_ref();
    let genres = metadata.map_or_else(String::new, |value| value.genres.join(" "));
    let developer = metadata
        .and_then(|value| value.developer.as_deref())
        .unwrap_or_default();
    let publisher = metadata
        .and_then(|value| value.publisher.as_deref())
        .unwrap_or_default();
    let release_date = metadata
        .and_then(|value| value.release_date.as_deref())
        .unwrap_or_default();
    let description = metadata
        .and_then(|value| value.description.as_deref())
        .unwrap_or_default();

    let searchable = format!(
        "{} {} {} {} {} {} {} {} {} {} {}",
        game.name,
        game.platform,
        game.platform_id.as_deref().unwrap_or_default(),
        game.catalogue_game_id.as_deref().unwrap_or_default(),
        game.install_path,
        game.executable.as_deref().unwrap_or_default(),
        genres,
        developer,
        publisher,
        release_date,
        description,
    )
    .to_lowercase();

    terms.iter().all(|term| searchable.contains(term))
}

#[cfg(test)]
mod tests {
    use super::{IpcEnvelope, PROTOCOL_VERSION, read_message, write_message};
    use amethys_errors::ErrorCode;
    use serde::{Deserialize, Serialize};
    use tokio::io::duplex;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct Payload {
        command: String,
    }

    #[test]
    fn dispatch_returns_pong_with_the_same_request_identifier() {
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::Ping));
        let request_id = request.request_id;
        let response = super::dispatch_request(request, super::IpcCoreStatus::Running);

        assert_eq!(response.request_id, request_id);
        assert_eq!(response.result, Ok(super::IpcResponsePayload::Pong));
    }

    #[test]
    fn dispatch_returns_saved_game_library_entries() {
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::ListGames));
        let games = vec![super::IpcGameEntry {
            id: "game-1".to_owned(),
            name: "Example Game".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("570".to_owned()),
            catalogue_game_id: Some("3000".to_owned()),
            catalogue_metadata: None,
            install_path: "/games/example".to_owned(),
            install_locations: Vec::new(),
            executable: Some("game.exe".to_owned()),
        }];
        let response =
            super::dispatch_request_with_games(request, super::IpcCoreStatus::Running, &games);
        assert_eq!(
            response.result,
            Ok(super::IpcResponsePayload::Games { games }),
        );
    }

    #[test]
    fn detected_game_command_round_trips_with_platform_identity() {
        let command = super::IpcCommand::AddDetectedGame {
            name: "Dota 2".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("570".to_owned()),
            install_path: "/games/dota2".to_owned(),
            executable: None,
        };
        let encoded = serde_json::to_vec(&command).expect("command serializes");
        let decoded: super::IpcCommand =
            serde_json::from_slice(&encoded).expect("command deserializes");
        assert_eq!(decoded, command);
    }

    #[test]
    fn game_entry_accepts_older_payloads_without_platform_id() {
        let json = r#"{
            "id":"legacy",
            "name":"Legacy Game",
            "platform":"Steam",
            "install_path":"/games/legacy",
            "executable":null
        }"#;
        let game: super::IpcGameEntry =
            serde_json::from_str(json).expect("legacy game payload deserializes");
        assert_eq!(game.platform_id, None);
        assert_eq!(game.catalogue_game_id, None);
        assert_eq!(game.catalogue_metadata, None);
    }

    #[test]
    fn catalogue_metadata_round_trips_over_ipc() {
        let metadata = super::IpcGameMetadata {
            genres: vec!["Action".to_owned(), "Adventure".to_owned()],
            developer: Some("Studio".to_owned()),
            publisher: Some("Publisher".to_owned()),
            release_date: Some("2026-11-22".to_owned()),
            description: Some("A canonical game description.".to_owned()),
        };
        let encoded = serde_json::to_vec(&metadata).expect("metadata serializes");
        let decoded: super::IpcGameMetadata =
            serde_json::from_slice(&encoded).expect("metadata deserializes");
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn dispatch_returns_the_current_core_status() {
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::GetStatus));
        let response = super::dispatch_request(request, super::IpcCoreStatus::Stopping);

        assert_eq!(
            response.result,
            Ok(super::IpcResponsePayload::CoreStatus {
                state: super::IpcCoreStatus::Stopping,
            }),
        );
    }

    #[tokio::test]
    async fn client_helper_sends_request_and_checks_response_identifier() {
        let (mut client, mut server) = duplex(1024);
        let server_task = tokio::spawn(async move {
            super::serve_connection(&mut server, super::IpcCoreStatus::Running)
                .await
                .expect("serve request");
        });

        let response =
            super::send_request(&mut client, super::IpcRequest::new(super::IpcCommand::Ping))
                .await
                .expect("send request");
        server_task.await.expect("server task");

        assert_eq!(response.result, Ok(super::IpcResponsePayload::Pong));
    }

    #[tokio::test]
    async fn client_helper_rejects_response_for_another_request() {
        let (mut client, mut server) = duplex(1024);
        let server_task = tokio::spawn(async move {
            let _: super::IpcEnvelope<super::IpcRequest> = super::read_envelope(&mut server)
                .await
                .expect("read request");
            let response =
                super::IpcResponse::success(Uuid::new_v4(), super::IpcResponsePayload::Pong);
            super::write_message(&mut server, &response)
                .await
                .expect("write mismatched response");
        });

        let error =
            super::send_request(&mut client, super::IpcRequest::new(super::IpcCommand::Ping))
                .await
                .expect_err("mismatched response must be rejected");
        server_task.await.expect("server task");

        assert_eq!(error.code(), ErrorCode::IpcResponseMismatch);
    }

    #[tokio::test]
    async fn connection_handler_reads_request_and_writes_correlated_response() {
        let (mut client, mut server) = duplex(1024);
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::Ping));
        let request_id = request.request_id;
        let send = tokio::spawn(async move {
            write_message(&mut client, &request)
                .await
                .expect("send request");
            let response: super::IpcResponse =
                read_message(&mut client).await.expect("read response");
            response
        });

        super::serve_connection(&mut server, super::IpcCoreStatus::Running)
            .await
            .expect("serve request");
        let response = send.await.expect("client task");
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.result, Ok(super::IpcResponsePayload::Pong));
    }

    #[test]
    fn ipc_response_keeps_original_request_identifier() {
        let request = IpcEnvelope::new(super::IpcRequest::new(super::IpcCommand::Ping));
        let response =
            super::IpcResponse::success(request.request_id, super::IpcResponsePayload::Pong);

        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.result, Ok(super::IpcResponsePayload::Pong));
    }

    #[test]
    fn catalogue_detail_accepts_missing_provider_identity() {
        let detail: super::IpcCatalogGameDetail = serde_json::from_str(
            r#"{
                "amethys_id": 1001,
                "name": "Provider-independent game",
                "slug": null,
                "summary": null,
                "release_date": null,
                "cover_url": null,
                "genres": [],
                "stores": []
            }"#,
        )
        .expect("catalogue detail without IGDB identity");

        assert_eq!(detail.amethys_id, 1001);
        assert_eq!(detail.igdb_id, None);
        assert!(detail.source_ids.is_empty());
    }

    #[test]
    fn catalogue_detail_preserves_multiple_provider_identities() {
        let detail: super::IpcCatalogGameDetail = serde_json::from_str(
            r#"{
                "amethys_id": 1002,
                "igdb_id": null,
                "name": "Multi-source game",
                "slug": null,
                "summary": null,
                "release_date": null,
                "cover_url": null,
                "genres": [],
                "source_ids": [
                    {"source": "igdb", "external_id": "12345"},
                    {"source": "store:steam", "external_id": "570"},
                    {"source": "store:gog", "external_id": "987654"}
                ],
                "stores": []
            }"#,
        )
        .expect("catalogue detail with multiple source identities");

        assert_eq!(detail.source_ids.len(), 3);
        assert_eq!(detail.source_ids[1].source, "store:steam");
        assert_eq!(detail.source_ids[1].external_id, "570");
    }

    #[test]
    fn library_search_matches_local_and_linked_catalogue_metadata() {
        let game = super::IpcGameEntry {
            id: "local-1".to_owned(),
            name: "Clair Obscur: Expedition 33".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("1903340".to_owned()),
            catalogue_game_id: Some("amethys-1001".to_owned()),
            catalogue_metadata: Some(super::IpcGameMetadata {
                genres: vec!["RPG".to_owned(), "Adventure".to_owned()],
                developer: Some("Sandfall Interactive".to_owned()),
                publisher: Some("Kepler Interactive".to_owned()),
                release_date: Some("2025-04-24".to_owned()),
                description: Some("A journey through a dark fantasy world.".to_owned()),
            }),
            install_path: "C:/Games/Expedition33".to_owned(),
            install_locations: Vec::new(),
            executable: Some("Expedition33.exe".to_owned()),
        };

        for query in [
            "clair obscur",
            "steam 1903340",
            "rpg adventure",
            "sandfall interactive",
            "kepler interactive",
            "2025-04",
            "dark fantasy",
            "amethys-1001",
            "C:/Games expedition",
        ] {
            assert!(
                super::game_matches_search_query(&game, query),
                "query {query:?}"
            );
        }
        assert!(!super::game_matches_search_query(&game, "dark science"));
        assert!(super::game_matches_search_query(&game, "   "));
    }

    #[test]
    fn library_search_supports_legacy_entries_without_catalogue_metadata() {
        let game = super::IpcGameEntry {
            id: "legacy-1".to_owned(),
            name: "Legacy Game".to_owned(),
            platform: "GOG".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: "/games/legacy".to_owned(),
            install_locations: Vec::new(),
            executable: None,
        };
        assert!(super::game_matches_search_query(&game, "legacy gog"));
        assert!(!super::game_matches_search_query(&game, "adventure"));
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
        let error = envelope
            .validate_version()
            .expect_err("version must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcUnsupportedVersion);
    }

    #[tokio::test]
    async fn read_envelope_rejects_unsupported_protocol_version() {
        let (mut writer, mut reader) = duplex(1024);
        let mut outgoing = IpcEnvelope::new(Payload {
            command: "ping".to_owned(),
        });
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
            tokio::io::AsyncWriteExt::write_u32(&mut writer, (super::MAX_FRAME_SIZE as u32) + 1)
                .await
                .expect("write oversized frame header");
        });

        let error = super::read_message::<_, serde_json::Value>(&mut reader)
            .await
            .expect_err("oversized frame must fail");
        send.await.expect("writer task");
        assert_eq!(error.code(), ErrorCode::IpcFrameTooLarge);
    }

    #[tokio::test]
    async fn reader_rejects_empty_frame_before_allocating_payload() {
        let (mut writer, mut reader) = duplex(8);
        use tokio::io::AsyncWriteExt;
        writer.write_u32(0).await.expect("write empty frame header");

        let error = super::read_message::<_, Payload>(&mut reader)
            .await
            .expect_err("empty frame must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcInvalidLength);
    }

    #[tokio::test]
    async fn reader_rejects_oversized_frame_from_header_alone() {
        let (mut writer, mut reader) = duplex(8);
        use tokio::io::AsyncWriteExt;
        writer
            .write_u32((super::MAX_FRAME_SIZE as u32) + 1)
            .await
            .expect("write oversized frame header");

        let error = super::read_message::<_, Payload>(&mut reader)
            .await
            .expect_err("oversized frame must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcFrameTooLarge);
    }

    #[tokio::test]
    async fn writer_rejects_oversized_serialized_payload() {
        let payload = "x".repeat(super::MAX_FRAME_SIZE + 1);
        let (mut writer, _reader) = duplex(8);

        let error = super::write_message(&mut writer, &payload)
            .await
            .expect_err("oversized payload must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcFrameTooLarge);
    }

    #[tokio::test]
    async fn reader_rejects_truncated_frame_header() {
        let (mut writer, mut reader) = duplex(8);
        use tokio::io::AsyncWriteExt;
        writer
            .write_all(&[0, 0])
            .await
            .expect("write partial header");
        writer.shutdown().await.expect("close truncated stream");

        let error = super::read_message::<_, Payload>(&mut reader)
            .await
            .expect_err("truncated header must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcIo);
    }

    #[tokio::test]
    async fn reader_rejects_truncated_frame_payload() {
        let (mut writer, mut reader) = duplex(8);
        use tokio::io::AsyncWriteExt;
        writer.write_u32(4).await.expect("write frame length");
        writer
            .write_all(b"{}")
            .await
            .expect("write partial payload");
        writer.shutdown().await.expect("close truncated stream");

        let error = super::read_message::<_, Payload>(&mut reader)
            .await
            .expect_err("truncated payload must be rejected");
        assert_eq!(error.code(), ErrorCode::IpcIo);
    }
}
