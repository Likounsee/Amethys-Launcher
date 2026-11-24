//! Thin client for the existing Amethys Core IPC service.
//! The UI never opens SQLite directly: the agent remains the owner of the database.

use amethys_ipc::{
    IpcCatalogGameDetail, IpcCatalogSearchEntry, IpcCommand, IpcGameEntry, IpcRequest,
    IpcResponsePayload, send_request,
    transport,
};

/// Reads the real local library through the same IPC protocol used by Amethys clients.
pub fn list_games() -> Result<Vec<IpcGameEntry>, String> {
    // Missing executable discovery belongs to Amethys Core startup, where it
    // runs in the background instead of delaying the desktop library view.
    list_games_with_timeouts(
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(15),
    )
}

fn list_games_with_timeouts(
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
) -> Result<Vec<IpcGameEntry>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Impossible de démarrer le client IPC : {error}"))?;

    runtime.block_on(async {
        #[cfg(windows)]
        let mut stream = tokio::time::timeout(
            connect_timeout,
            transport::connect(r"\\.\pipe\amethys-core"),
        )
        .await
        .map_err(|_| format!("Délai dépassé lors de la connexion à Amethys Core ({:.1} s).", connect_timeout.as_secs_f32()))?
        .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            tokio::time::timeout(
                connect_timeout,
                transport::connect(directory.join("amethys.sock")),
            )
            .await
            .map_err(|_| format!("Délai dépassé lors de la connexion à Amethys Core ({:.1} s).", connect_timeout.as_secs_f32()))?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            request_timeout,
            send_request(&mut stream, IpcRequest::new(IpcCommand::ListGames)),
        )
        .await
        .map_err(|_| format!("Amethys Core n'a pas répondu dans les {:.1} secondes.", request_timeout.as_secs_f32()))?
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
        root.join(trimmed)
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

/// Returns the reason a game cannot currently be launched, using the exact
/// same validation path as launch_game. None means the executable is ready.
pub fn launch_block_reason(game: &IpcGameEntry) -> Option<String> {
    resolve_launch_executable(game).err()
}

/// Whether the configured executable currently passes the same checks as launch.
pub fn can_launch(game: &IpcGameEntry) -> bool {
    launch_block_reason(game).is_none()
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


/// Imports one discovered installation through the Core-owned database.
pub fn add_detected_game(
    detected: &amethys_discovery::DetectedGame,
) -> Result<IpcGameEntry, String> {
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
        .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core.".to_owned())?
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
            .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core.".to_owned())?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            send_request(
                &mut stream,
                IpcRequest::new(IpcCommand::AddDetectedGame {
                    name: detected.name.clone(),
                    platform: detected.platform.to_owned(),
                    platform_id: detected.platform_id.clone(),
                    install_path: detected.install_path.to_string_lossy().into_owned(),
                    executable: amethys_discovery::resolve_game_executable(detected)
                        .map(|path| path.to_string_lossy().into_owned()),
                }),
            ),
        )
        .await
        .map_err(|_| "Amethys Core n'a pas répondu dans les 15 secondes.".to_owned())?
        .map_err(|error| error.to_string())?;

        match response.result {
            Ok(IpcResponsePayload::GameAdded { game }) => Ok(*game),
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
            Ok(IpcResponsePayload::GameAdded { game }) => Ok(*game),
            Ok(other) => Err(format!("Réponse IPC inattendue : {other:?}")),
            Err(error) => Err(format!("{} : {}", error.code, error.message)),
        }
    })
}

/// Finds the agent beside a packaged launcher or in an ancestor Cargo workspace target directory.
fn find_agent_executable(launcher_executable: &std::path::Path) -> Option<std::path::PathBuf> {
    let directory = launcher_executable.parent()?;
    #[cfg(windows)]
    let agent_name = "amethys-agent.exe";
    #[cfg(not(windows))]
    let agent_name = "amethys-agent";

    // Packaged builds keep the agent beside the launcher. Development builds
    // of this nested preview crate have their own target directory, while the
    // Core agent is built in the repository workspace's target directory.
    let profile = directory.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| *name == "debug" || *name == "release")
        .unwrap_or("debug");

    let beside_launcher = directory.join(agent_name);
    if beside_launcher.is_file() {
        return Some(beside_launcher);
    }

    directory.ancestors()
        .map(|ancestor| ancestor.join("target").join(profile).join(agent_name))
        .find(|candidate| candidate.is_file())
}

/// Connects to the running agent or starts the bundled/development agent automatically.
pub fn list_games_or_start_agent() -> Result<Vec<IpcGameEntry>, String> {
    match list_games() {
        Ok(games) => Ok(games),
        Err(initial_error) => {
            let executable = std::env::current_exe()
                .map_err(|error| format!("Impossible de localiser le launcher : {error}"))?;
            let directory = executable.parent()
                .ok_or_else(|| "Impossible de localiser le dossier du launcher.".to_owned())?;
            let Some(agent_path) = find_agent_executable(&executable) else {
                return Err(format!(
                    "Amethys Core est inaccessible ({initial_error}) et amethys-agent est introuvable. Compilez-le depuis la racine du dépôt avec « cargo build --release -p amethys-agent », puis relancez le launcher. Dossier du launcher : {}",
                    directory.display()
                ));
            };

            let mut command = std::process::Command::new(&agent_path);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            command.spawn()
                .map_err(|error| format!("Impossible de démarrer Amethys Core depuis {} : {error}", agent_path.display()))?;

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if let Ok(games) = list_games_with_timeouts(
                    std::time::Duration::from_secs(1),
                    std::time::Duration::from_secs(1),
                ) {
                    return Ok(games);
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
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
            catalogue_game_id: Some("3000".to_owned()),
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            executable: Some("bin/game.exe".to_owned()),
        };
        (root, game)
    }

    #[test]
    fn finds_agent_in_parent_workspace_target_directory() {
        let root = std::env::temp_dir().join(format!("amethys-agent-search-{}", std::process::id()));
        let launcher_dir = root.join("prototypes/preview/target/release");
        let agent_dir = root.join("target/release");
        std::fs::create_dir_all(&launcher_dir).unwrap();
        std::fs::create_dir_all(&agent_dir).unwrap();
        let launcher = launcher_dir.join(if cfg!(windows) { "amethys-launcher.exe" } else { "amethys-launcher" });
        let agent = agent_dir.join(if cfg!(windows) { "amethys-agent.exe" } else { "amethys-agent" });
        std::fs::write(&launcher, b"launcher").unwrap();
        std::fs::write(&agent, b"agent").unwrap();

        assert_eq!(find_agent_executable(&launcher), Some(agent.clone()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prefers_agent_beside_launcher() {
        let root = std::env::temp_dir().join(format!("amethys-agent-local-{}", std::process::id()));
        let directory = root.join("release");
        std::fs::create_dir_all(&directory).unwrap();
        let launcher = directory.join(if cfg!(windows) { "amethys-launcher.exe" } else { "amethys-launcher" });
        let agent = directory.join(if cfg!(windows) { "amethys-agent.exe" } else { "amethys-agent" });
        std::fs::write(&launcher, b"launcher").unwrap();
        std::fs::write(&agent, b"agent").unwrap();

        assert_eq!(find_agent_executable(&launcher), Some(agent.clone()));
        std::fs::remove_dir_all(root).unwrap();
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
    fn launch_block_reason_explains_missing_executable_configuration() {
        let (root, mut game) = fixture();
        game.executable = None;
        assert_eq!(
            launch_block_reason(&game).as_deref(),
            Some("Aucun exécutable n'est configuré pour ce jeu.")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn launch_block_reason_explains_rejected_relative_path() {
        let (root, mut game) = fixture();
        game.executable = Some("../outside.exe".to_owned());
        assert_eq!(
            launch_block_reason(&game).as_deref(),
            Some("Chemin d'exécutable relatif non sûr.")
        );
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

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_that_escapes_install_root() {
        let (root, mut game) = fixture();
        let outside = root.parent().unwrap().join(format!("amethys-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("escape.exe"), b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("bin/linked")).unwrap();
        game.executable = Some("bin/linked/escape.exe".to_owned());
        assert!(resolve_launch_executable(&game).is_err());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn rejects_missing_executable() {
        let (root, mut game) = fixture();
        game.executable = Some("bin/missing.exe".to_owned());
        assert!(resolve_launch_executable(&game).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}


/// Sends a Core command over the same local IPC endpoint used by the library.
fn send_core_command(
    command: IpcCommand,
    request_timeout: std::time::Duration,
) -> Result<IpcResponsePayload, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Impossible de démarrer le client IPC : {error}"))?;

    runtime.block_on(async {
        #[cfg(windows)]
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport::connect(r"\\.\pipe\amethys-core"),
        )
        .await
        .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (10 s).".to_owned())?
        .map_err(|error| error.to_string())?;

        #[cfg(unix)]
        let mut stream = {
            let directory = amethys_config::data_directory()
                .map_err(|error| format!("Dossier de données Amethys indisponible : {error}"))?;
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                transport::connect(directory.join("amethys.sock")),
            )
            .await
            .map_err(|_| "Délai dépassé lors de la connexion à Amethys Core (10 s).".to_owned())?
            .map_err(|error| error.to_string())?
        };

        let response = tokio::time::timeout(
            request_timeout,
            send_request(&mut stream, IpcRequest::new(command)),
        )
        .await
        .map_err(|_| {
            format!(
                "Amethys Core n'a pas répondu dans les {:.0} secondes.",
                request_timeout.as_secs_f32()
            )
        })?
        .map_err(|error| error.to_string())?;

        response
            .result
            .map_err(|error| format!("{} : {}", error.code, error.message))
    })
}

/// Downloads and verifies the public master-catalogue index through Core.
pub fn sync_master_catalog() -> Result<(String, u64), String> {
    match send_core_command(
        IpcCommand::SyncMasterCatalog,
        std::time::Duration::from_secs(180),
    )? {
        IpcResponsePayload::CatalogSynchronized {
            catalog_version,
            game_count,
        } => Ok((catalog_version, game_count)),
        other => Err(format!("Réponse IPC inattendue : {other:?}")),
    }
}

/// Searches the locally cached public catalogue without opening the database.
pub fn search_master_catalog(
    query: &str,
    limit: usize,
) -> Result<Vec<IpcCatalogSearchEntry>, String> {
    match send_core_command(
        IpcCommand::SearchMasterCatalog {
            query: query.to_owned(),
            limit: limit.clamp(1, 100),
        },
        std::time::Duration::from_secs(30),
    )? {
        IpcResponsePayload::CatalogSearchResults { games } => Ok(games),
        other => Err(format!("Réponse IPC inattendue : {other:?}")),
    }
}

/// Loads a single catalogue detail and its store identities through Core.
pub fn get_master_catalog_game(
    amethys_id: u64,
) -> Result<IpcCatalogGameDetail, String> {
    match send_core_command(
        IpcCommand::GetMasterCatalogGame { amethys_id },
        std::time::Duration::from_secs(60),
    )? {
        IpcResponsePayload::CatalogGameDetails { game } => Ok(game),
        other => Err(format!("Réponse IPC inattendue : {other:?}")),
    }
}
