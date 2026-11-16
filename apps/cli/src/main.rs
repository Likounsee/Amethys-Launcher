use amethys_ipc::{IpcCommand, IpcCoreStatus, IpcRequest, IpcResponsePayload, send_request};
use std::error::Error;

#[cfg(any(unix, windows))]
use amethys_ipc::transport::{self, BoxedIpcStream};

fn print_help() {
    println!(
        "Amethys Launcher — contrôle en ligne de commande

USAGE:
    amethysctl <COMMANDE>

COMMANDES:
    ping                              Vérifie que le cœur Amethys répond
    status                            Affiche l'état du cœur
    games                             Liste la bibliothèque Amethys
    games scan                        Détecte les jeux sans les importer
    games import <appid>              Importe un jeu Steam par son App ID
    games import <plateforme> <id>    Importe un jeu détecté par plateforme et ID
    games import-all                  Importe les jeux détectés non encore enregistrés
    games verify                      Vérifie les chemins sans modifier les installations
    games add <nom> <plateforme> <dossier> [exécutable]
    games remove <id>                 Retire un jeu de la bibliothèque Amethys

OPTIONS:
    -h, --help                        Affiche cette aide
    -V, --version                     Affiche la version

ENVIRONNEMENT:
    AMETHYS_DATA_DIR                  Remplace le dossier de données par défaut"
    );
}

fn parse_command(argument: Option<&str>) -> Result<IpcCommand, &'static str> {
    match argument {
        Some("ping") => Ok(IpcCommand::Ping),
        Some("status") => Ok(IpcCommand::GetStatus),
        Some("games") => Ok(IpcCommand::ListGames),
        _ => Err("Commande inconnue. Utilisation : amethysctl <ping|status|games>"),
    }
}

#[cfg(unix)]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Ok(transport::connect(amethys_config::data_directory()?.join("amethys.sock")).await?)
}

#[cfg(windows)]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Ok(transport::connect(r"\\.\pipe\amethys-core").await?)
}

#[cfg(not(any(unix, windows)))]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Err("La communication locale avec Amethys n'est pas disponible sur ce système.".into())
}

/// Sends one local command with bounded connection and response times.
async fn request_core(command: IpcCommand) -> Result<amethys_ipc::IpcResponse, Box<dyn Error>> {
    let mut connection = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        connect_to_core(),
    )
    .await??;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        send_request(&mut connection, IpcRequest::new(command)),
    )
    .await??;

    Ok(response)
}

fn status_label(status: IpcCoreStatus) -> &'static str {
    match status {
        IpcCoreStatus::Created => "créé, mais pas encore démarré",
        IpcCoreStatus::Running => "en fonctionnement",
        IpcCoreStatus::Stopping => "en cours d'arrêt",
        IpcCoreStatus::Stopped => "arrêté",
    }
}

fn parse_arguments(arguments: &[String]) -> Result<IpcCommand, &'static str> {
    match arguments {
        [command] => parse_command(Some(command)),
        [group, action, name, platform, install_path] if group == "games" && action == "add" => {
            Ok(IpcCommand::AddGame {
                name: name.clone(),
                platform: platform.clone(),
                install_path: install_path.clone(),
                executable: None,
            })
        }
        [group, action, name, platform, install_path, executable] if group == "games" && action == "add" => {
            Ok(IpcCommand::AddGame {
                name: name.clone(),
                platform: platform.clone(),
                install_path: install_path.clone(),
                executable: Some(executable.clone()),
            })
        }
        [group, action, id] if group == "games" && action == "remove" => {
            Ok(IpcCommand::RemoveGame { id: id.clone() })
        }
        _ => Err("Utilisation : amethysctl games add <nom> <plateforme> <dossier> [executable] | games remove <id> | games scan | games import <appid> | games import <plateforme> <id> | games import-all | games verify"),
    }
}

/// Compares saved and detected installation paths after resolving aliases.
/// Windows paths are case-insensitive; other platforms retain native path rules.
fn same_install_location(saved: &str, detected: &std::path::Path) -> bool {
    let saved = std::path::Path::new(saved);
    let saved = saved.canonicalize().unwrap_or_else(|_| saved.to_path_buf());
    let detected = detected.canonicalize().unwrap_or_else(|_| detected.to_path_buf());

    #[cfg(windows)]
    {
        saved.to_string_lossy().eq_ignore_ascii_case(&detected.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        saved == detected
    }
}

fn game_is_already_imported(
    saved_games: &[amethys_ipc::IpcGameEntry],
    detected: &amethys_discovery::DetectedGame,
) -> bool {
    saved_games.iter().any(|saved| {
        saved.platform.eq_ignore_ascii_case(detected.platform)
            && match (&saved.platform_id, &detected.platform_id) {
                (Some(saved_id), Some(detected_id)) => saved_id == detected_id,
                _ => same_install_location(&saved.install_path, &detected.install_path),
            }
    })
}

async fn load_saved_games() -> Result<Vec<amethys_ipc::IpcGameEntry>, Box<dyn Error>> {
    let response = request_core(IpcCommand::ListGames).await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;

    match response.result {
        Ok(IpcResponsePayload::Games { games }) => Ok(games),
        Err(remote_error) => Err(format!(
            "Impossible de vérifier la bibliothèque ({}): {}",
            remote_error.code, remote_error.message
        ).into()),
        Ok(_) => Err(
            "Amethys a envoyé une réponse inattendue en lisant la bibliothèque.".into(),
        ),
    }
}

async fn import_one_detected_game(
    game: &amethys_discovery::DetectedGame,
    saved_games: &mut Vec<amethys_ipc::IpcGameEntry>,
) -> Result<bool, Box<dyn Error>> {
    if game_is_already_imported(saved_games, game) {
        println!("{} est déjà présent dans la bibliothèque Amethys.", game.name);
        return Ok(false);
    }

    let command = IpcCommand::AddDetectedGame {
        name: game.name.clone(),
        platform: game.platform.to_owned(),
        platform_id: game.platform_id.clone(),
        install_path: game.install_path.to_string_lossy().into_owned(),
        executable: None,
    };
    let response = request_core(command).await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;
    match response.result {
        Ok(IpcResponsePayload::GameAdded { game }) => {
            println!("{} a été ajouté à la bibliothèque Amethys.", game.name);
            saved_games.push(game);
            Ok(true)
        }
        Err(remote_error) => Err(format!(
            "Amethys n'a pas pu ajouter le jeu ({}): {}",
            remote_error.code, remote_error.message
        ).into()),
        Ok(_) => Err("Amethys a envoyé une réponse inattendue pendant l'ajout du jeu.".into()),
    }
}

async fn import_detected_games(
    detected_games: Vec<amethys_discovery::DetectedGame>,
) -> Result<(), Box<dyn Error>> {
    if detected_games.is_empty() {
        println!("Aucun jeu installé détecté à importer.");
        return Ok(());
    }

    let mut saved_games = load_saved_games().await?;
    let total = detected_games.len();
    let mut added = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();

    for game in &detected_games {
        match import_one_detected_game(game, &mut saved_games).await {
            Ok(true) => added += 1,
            Ok(false) => skipped += 1,
            Err(error) => {
                eprintln!("Échec de l'import de {} : {error}", game.name);
                failures.push(game.name.clone());
            }
        }
    }

    println!(
        "Import terminé : {added} ajouté(s), {skipped} déjà présent(s), {} échec(s), sur {total} jeu(x) détecté(s).",
        failures.len()
    );

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Certains jeux n'ont pas pu être importés : {}.",
            failures.join(", ")
        ).into())
    }
}

fn installation_path_exists(path: &str) -> bool {
    std::path::Path::new(path).is_dir()
}

async fn verify_saved_games() -> Result<(), Box<dyn Error>> {
    let games = load_saved_games().await?;
    if games.is_empty() {
        println!("Aucun jeu enregistré à vérifier.");
        return Ok(());
    }

    let mut missing = 0usize;
    for game in &games {
        if installation_path_exists(&game.install_path) {
            println!("[OK] {} ({}) — {}", game.name, game.platform, game.install_path);
        } else {
            missing += 1;
            println!("[ABSENT] {} ({}) — {}", game.name, game.platform, game.install_path);
        }
    }

    let available = games.len() - missing;
    println!(
        "Vérification terminée : {available}/{} chemin(s) d'installation disponible(s), {missing} absent(s). Aucun fichier n'a été modifié.",
        games.len()
    );

    if missing == 0 {
        Ok(())
    } else {
        Err(format!("{missing} chemin(s) d'installation sont introuvables.").into())
    }
}

async fn import_detected_game(platform: &str, platform_id: &str) -> Result<(), Box<dyn Error>> {
    let game = amethys_discovery::discover_all_games()
        .into_iter()
        .find(|game| {
            game.platform.eq_ignore_ascii_case(platform)
                && game.platform_id.as_deref() == Some(platform_id)
        })
        .ok_or_else(|| format!(
            "Aucun jeu installé de la plateforme « {platform} » ne correspond à l'identifiant {platform_id}. Lance « amethysctl games scan » pour voir les jeux détectés."
        ))?;

    import_detected_games(vec![game]).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() || arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"] {
        print_help();
        return Ok(());
    }
    if arguments.as_slice() == ["--version"] || arguments.as_slice() == ["-V"] {
        println!("Amethys Launcher {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if arguments.as_slice() == ["games", "import-all"] {
        return import_detected_games(amethys_discovery::discover_all_games()).await;
    }
    if arguments.as_slice() == ["games", "verify"] {
        return verify_saved_games().await;
    }
    if let [group, action, app_id] = arguments.as_slice() {
        if group == "games" && action == "import" {
            // Backward-compatible shorthand: a single identifier means Steam.
            return import_detected_game("Steam", app_id).await;
        }
    }
    if let [group, action, platform, platform_id] = arguments.as_slice() {
        if group == "games" && action == "import" {
            return import_detected_game(platform, platform_id).await;
        }
    }
    if arguments.as_slice() == ["games", "scan"] {
        let games = amethys_discovery::discover_all_games();
        if games.is_empty() {
            println!("Aucun jeu installé n'a été détecté dans les plateformes prises en charge.");
        } else {
            println!("Jeux détectés (aucun ajout automatique) :");
            for game in games {
                println!("- {} [{}]{} — {}", game.name, game.platform, game.platform_id.as_deref().map(|id| format!(" — ID {id}")).unwrap_or_default(), game.install_path.display());
            }
        }
        return Ok(());
    }

    let command = parse_arguments(&arguments)?;

    let response = request_core(command.clone()).await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;

    match response.result {
        Err(remote_error) => Err(format!(
            "Amethys a renvoyé une erreur ({}): {}",
            remote_error.code, remote_error.message
        ).into()),
        Ok(IpcResponsePayload::Pong) if command == IpcCommand::Ping => {
            println!("Amethys fonctionne et répond correctement.");
            Ok(())
        }
        Ok(IpcResponsePayload::CoreStatus { state }) if command == IpcCommand::GetStatus => {
            println!("État d'Amethys : {}.", status_label(state));
            Ok(())
        }
        Ok(IpcResponsePayload::Games { games }) if command == IpcCommand::ListGames => {
            if games.is_empty() {
                println!("Aucun jeu enregistré dans la bibliothèque.");
            } else {
                println!("Jeux enregistrés :");
                for game in games {
                    println!(
                    "- {} ({}){} — {}",
                    game.name,
                    game.platform,
                    game.platform_id.as_deref().map(|id| format!(" — ID {id}")).unwrap_or_default(),
                    game.install_path
                );
                }
            }
            Ok(())
        }
        Ok(IpcResponsePayload::GameAdded { game }) => {
            println!("Jeu ajouté : {} (identifiant : {}).", game.name, game.id);
            Ok(())
        }
        Ok(IpcResponsePayload::GameRemoved { removed: true }) => {
            println!("Jeu supprimé de la bibliothèque.");
            Ok(())
        }
        Ok(IpcResponsePayload::GameRemoved { removed: false }) => {
            Err("Aucun jeu ne correspond à cet identifiant.".into())
        }
        Ok(_) => Err("Amethys a envoyé une réponse inattendue.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{installation_path_exists, parse_arguments, parse_command, same_install_location, status_label};
    use amethys_ipc::{IpcCommand, IpcCoreStatus};

    #[test]
    fn matches_imported_games_by_platform_and_platform_identifier() {
        let saved = amethys_ipc::IpcGameEntry {
            id: "local-id".to_owned(),
            name: "Example".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("570".to_owned()),
            install_path: "/games/example".to_owned(),
            executable: None,
        };
        let same_game = amethys_discovery::DetectedGame {
            platform_id: Some("570".to_owned()),
            name: "Renamed Example".to_owned(),
            install_path: std::path::PathBuf::from("/different/path"),
            platform: "steam",
        };
        let other_store = amethys_discovery::DetectedGame {
            platform_id: Some("570".to_owned()),
            name: "Other Store Game".to_owned(),
            install_path: std::path::PathBuf::from("/different/path"),
            platform: "Epic Games",
        };

        assert!(super::game_is_already_imported(std::slice::from_ref(&saved), &same_game));
        assert!(!super::game_is_already_imported(&[saved], &other_store));
    }

    #[test]
    fn recognizes_installation_path_aliases_without_merging_distinct_paths() {
        let root = std::env::temp_dir().join(format!(
            "amethys-cli-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Installed Game");
        let other = root.join("Other Game");
        std::fs::create_dir_all(&install).expect("create install");
        std::fs::create_dir_all(&other).expect("create other install");

        let alias = install.join("..").join("Installed Game");
        assert!(same_install_location(&alias.to_string_lossy(), &install));
        assert!(!same_install_location(&other.to_string_lossy(), &install));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn verifies_installation_paths_without_modifying_them() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create installation directory");
        let existing = root.to_string_lossy().into_owned();
        let missing = root.join("missing-game").to_string_lossy().into_owned();

        assert!(installation_path_exists(&existing));
        assert!(!installation_path_exists(&missing));
        assert!(root.is_dir(), "verification must leave the installation untouched");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn verification_rejects_a_file_where_an_installation_directory_is_expected() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create test root");
        let file = root.join("not-a-game-folder.exe");
        std::fs::write(&file, b"sentinel").expect("create regular file");

        assert!(!installation_path_exists(file.to_str().expect("UTF-8 test path")));
        assert_eq!(std::fs::read(&file).expect("file remains"), b"sentinel");

        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn accepts_supported_commands() {
        assert_eq!(parse_command(Some("ping")), Ok(IpcCommand::Ping));
        assert_eq!(parse_command(Some("status")), Ok(IpcCommand::GetStatus));
        assert_eq!(parse_command(Some("games")), Ok(IpcCommand::ListGames));
    }

    #[test]
    fn parses_game_management_commands() {
        let add = vec!["games".to_owned(), "add".to_owned(), "Game".to_owned(), "Steam".to_owned(), "/games/game".to_owned(), "game.exe".to_owned()];
        assert_eq!(parse_arguments(&add), Ok(IpcCommand::AddGame {
            name: "Game".to_owned(),
            platform: "Steam".to_owned(),
            install_path: "/games/game".to_owned(),
            executable: Some("game.exe".to_owned()),
        }));
        let remove = vec!["games".to_owned(), "remove".to_owned(), "game-id".to_owned()];
        assert_eq!(parse_arguments(&remove), Ok(IpcCommand::RemoveGame { id: "game-id".to_owned() }));
    }

    #[test]
    fn rejects_missing_or_unknown_commands() {
        assert!(parse_command(None).is_err());
        assert!(parse_command(Some("launch")).is_err());
    }

    #[test]
    fn explains_core_states_in_french() {
        assert_eq!(status_label(IpcCoreStatus::Running), "en fonctionnement");
        assert_eq!(status_label(IpcCoreStatus::Stopped), "arrêté");
    }
}
