//! Thin client for the existing Amethys Core IPC service.
//! The UI never opens SQLite directly: the agent remains the owner of the database.

use amethys_ipc::{
    IpcCommand, IpcGameEntry, IpcRequest, IpcResponsePayload, send_request,
    transport,
};

/// Reads the real local library through the same IPC protocol used by Amethys clients.
pub fn list_games() -> Result<Vec<IpcGameEntry>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Impossible de démarrer le client IPC : {error}"))?;

    runtime.block_on(async {
        #[cfg(windows)]
        let mut stream = transport::connect(r"\\.\pipe\amethys-core")
            .await
            .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            transport::connect(directory.join("amethys.sock"))
                .await
                .map_err(|error| error.to_string())?
        };

        let response = send_request(
            &mut stream,
            IpcRequest::new(IpcCommand::ListGames),
        )
        .await
        .map_err(|error| error.to_string())?;

        match response.result {
            Ok(IpcResponsePayload::Games { games }) => Ok(games),
            Ok(other) => Err(format!("Réponse IPC inattendue : {other:?}")),
            Err(error) => Err(format!("{} : {}", error.code, error.message)),
        }
    })
}
