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
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport::connect(r"\\.\pipe\amethys-core"),
        )
        .await
        .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
        .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                transport::connect(directory.join("amethys.sock")),
            )
            .await
            .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            send_request(&mut stream, IpcRequest::new(IpcCommand::ListGames)),
        )
        .await
        .map_err(|_| "Amethys Core n'a pas répondu dans les 15 secondes.".to_owned())?
        .map_err(|error| error.to_string())?;

        match response.result {
            Ok(IpcResponsePayload::Games { games }) => Ok(games),
            Ok(other) => Err(format!("Réponse IPC inattendue : {other:?}")),
            Err(error) => Err(format!("{} : {}", error.code, error.message)),
        }
    })
}


/// Resolves a configured executable and guarantees it remains inside the registered game directory.
fn resolve_launch_executable(game: &IpcGameEntry) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let executable = game.executable.as_deref()
        .ok_or_else(|| "Aucun exécutable n'est configuré pour ce jeu.".to_owned())?;
    let root = std::path::Path::new(&game.install_path)
        .canonicalize()
        .map_err(|error| format!("Dossier du jeu indisponible : {error}"))?;
    if !root.is_dir() {
        return Err("Le chemin d'installation n'est pas un dossier.".to_owned());
    }

    let candidate = std::path::Path::new(executable);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        let trimmed = executable.trim();
        let mut components = trimmed.split(['/', '\\']);
        let Some(first) = components.next() else {
            return Err("Chemin d'exécutable relatif non sûr.".to_owned());
        };
        if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\')
            || !components.chain(std::iter::once(first))
                .all(|component| !component.contains(':') && component != "." && component != "..")
        {
            return Err("Chemin d'exécutable relatif non sûr.".to_owned());
        }
        root.join(candidate)
    };
    let resolved = candidate.canonicalize()
        .map_err(|error| format!("Exécutable introuvable ou inaccessible : {error}"))?;
    if !resolved.starts_with(&root) {
        return Err("L'exécutable se trouve en dehors du dossier du jeu ; démarrage refusé.".to_owned());
    }
    if !resolved.is_file() {
        return Err("Le chemin configuré ne désigne pas un fichier exécutable.".to_owned());
    }
    Ok((root, resolved))
}

/// Whether the configured executable currently passes the same checks as launch.
pub fn can_launch(game: &IpcGameEntry) -> bool {
    resolve_launch_executable(game).is_ok()
}

/// Starts a configured game after validating its executable against the registered installation.
pub fn launch_game(game: &IpcGameEntry) -> Result<u32, String> {
    let (root, executable) = resolve_launch_executable(game)?;
    std::process::Command::new(&executable)
        .current_dir(&root)
        .spawn()
        .map(|child| child.id())
        .map_err(|error| format!("Le système n'a pas pu démarrer le jeu : {error}"))
}


/// Removes one saved library entry through the Core-owned storage service.
pub fn remove_game(id: &str) -> Result<bool, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Impossible de démarrer le client IPC : {error}"))?;

    runtime.block_on(async {
        #[cfg(windows)]
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport::connect(r"\\.\pipe\amethys-core"),
        )
        .await
        .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
        .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                transport::connect(directory.join("amethys.sock")),
            )
            .await
            .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            send_request(
                &mut stream,
                IpcRequest::new(IpcCommand::RemoveGame { id: id.to_owned() }),
            ),
        )
        .await
        .map_err(|_| "Amethys Core n'a pas répondu dans les 15 secondes.".to_owned())?
        .map_err(|error| error.to_string())?;

        match response.result {
            Ok(IpcResponsePayload::GameRemoved { removed }) => Ok(removed),
            Ok(other) => Err(format!("Réponse IPC inattendue : {other:?}")),
            Err(error) => Err(format!("{} : {}", error.code, error.message)),
        }
    })
}


/// Saves the executable path for an existing library entry through Core IPC.
pub fn set_game_executable(id: &str, executable: &str) -> Result<IpcGameEntry, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Impossible de démarrer le client IPC : {error}"))?;

    runtime.block_on(async {
        #[cfg(windows)]
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            transport::connect(r"\\.\pipe\amethys-core"),
        )
        .await
        .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
        .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                transport::connect(directory.join("amethys.sock")),
            )
            .await
            .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (5 s).".to_owned())?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            send_request(
                &mut stream,
                IpcRequest::new(IpcCommand::SetGameExecutable {
                    id: id.to_owned(),
                    executable: executable.to_owned(),
                }),
            ),
        )
        .await
        .map_err(|_| "Amethys Core n'a pas répondu dans les 15 secondes.".to_owned())?
        .map_err(|error| error.to_string())?;

        match response.result {
            Ok(IpcResponsePayload::GameAdded { game }) => Ok(game),
            Ok(other) => Err(format!("Réponse IPC inattendue : {other:?}")),
            Err(error) => Err(format!("{} : {}", error.code, error.message)),
        }
    })
}

/// Connects to the running agent or starts the bundled agent beside the desktop executable.
pub fn list_games_or_start_agent() -> Result<Vec<IpcGameEntry>, String> {
    match list_games() {
        Ok(games) => return Ok(games),
        Err(initial_error) => {
            let executable = std::env::current_exe()
                .map_err(|error| format!("Impossible de localiser le launcher : {error}"))?;
            let directory = executable.parent()
                .ok_or_else(|| "Impossible de localiser le dossier du launcher.".to_owned())?;
            #[cfg(windows)]
            let agent_path = directory.join("amethys-agent.exe");
            #[cfg(not(windows))]
            let agent_path = directory.join("amethys-agent");

            if !agent_path.is_file() {
                return Err(format!(
                    "Amethys Core est inaccessible ({initial_error}) et amethys-agent est absent de {}. Placez l'agent à côté du launcher ou démarrez-le séparément.",
                    directory.display()
                ));
            }

            let mut command = std::process::Command::new(&agent_path);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            command.spawn()
                .map_err(|error| format!("Impossible de démarrer Amethys Core depuis {} : {error}", agent_path.display()))?;

            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Ok(games) = list_games() {
                    return Ok(games);
                }
            }
            Err(format!(
                "Amethys Core n'a pas répondu après son démarrage. Première erreur IPC : {initial_error}"
            ))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> (std::path::PathBuf, IpcGameEntry) {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("amethys-launch-check-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/game.exe"), b"test executable").unwrap();
        let game = IpcGameEntry {
            id: "test-game".to_owned(),
            name: "Test Game".to_owned(),
            platform: "Tests".to_owned(),
            platform_id: None,
            install_path: root.to_string_lossy().into_owned(),
            executable: Some("bin/game.exe".to_owned()),
        };
        (root, game)
    }

    #[test]
    fn resolves_executable_inside_install_root() {
        let (root, game) = fixture();
        let resolved = resolve_launch_executable(&game).unwrap();
        assert_eq!(resolved.0, root.canonicalize().unwrap());
        assert!(resolved.1.ends_with("bin/game.exe"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        let (root, mut game) = fixture();
        game.executable = Some("../outside.exe".to_owned());
        assert!(resolve_launch_executable(&game).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_absolute_executable_outside_install_root() {
        let (root, mut game) = fixture();
        let outside = root.parent().unwrap().join("outside-amethys-test.exe");
        std::fs::write(&outside, b"outside").unwrap();
        game.executable = Some(outside.to_string_lossy().into_owned());
        assert!(resolve_launch_executable(&game).is_err());
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_executable() {
        let (root, mut game) = fixture();
        game.executable = Some("bin/missing.exe".to_owned());
        assert!(resolve_launch_executable(&game).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
