use amethys_ipc::{IpcCommand, IpcCoreStatus, IpcRequest, IpcResponsePayload, send_request};
use std::error::Error;

#[cfg(any(unix, windows))]
use amethys_ipc::transport::{self, BoxedIpcStream};
#[cfg(unix)]
use std::path::PathBuf;

fn parse_command(argument: Option<&str>) -> Result<IpcCommand, &'static str> {
    match argument {
        Some("ping") => Ok(IpcCommand::Ping),
        Some("status") => Ok(IpcCommand::GetStatus),
        Some("games") => Ok(IpcCommand::ListGames),
        _ => Err("Commande inconnue. Utilisation : amethysctl <ping|status|games>"),
    }
}

#[cfg(unix)]
fn data_directory() -> PathBuf {
    std::env::var_os("AMETHYS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".amethys")
        })
}

#[cfg(unix)]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Ok(transport::connect(data_directory().join("amethys.sock")).await?)
}

#[cfg(windows)]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Ok(transport::connect(r"\\.\pipe\amethys-core").await?)
}

#[cfg(not(any(unix, windows)))]
async fn connect_to_core() -> Result<BoxedIpcStream, Box<dyn Error>> {
    Err("La communication locale avec Amethys n'est pas disponible sur ce système.".into())
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
        _ => Err("Utilisation : amethysctl games add <nom> <plateforme> <dossier> [executable] | games remove <id> | games scan | games import <appid>"),
    }
}

async fn import_detected_steam_game(app_id: &str) -> Result<(), Box<dyn Error>> {
    let game = amethys_discovery::discover_steam_games()
        .into_iter()
        .find(|game| game.app_id == app_id)
        .ok_or_else(|| format!(
            "Aucun jeu Steam installé ne correspond à l'AppID {app_id}. Lance « amethysctl games scan » pour voir les jeux détectés."
        ))?;

    let mut library_connection = connect_to_core().await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;
    let library_response = send_request(
        &mut library_connection,
        IpcRequest::new(IpcCommand::ListGames),
    ).await?;

    let saved_games = match library_response.result {
        Ok(IpcResponsePayload::Games { games }) => games,
        Err(remote_error) => return Err(format!(
            "Impossible de vérifier la bibliothèque ({}): {}",
            remote_error.code, remote_error.message
        ).into()),
        Ok(_) => return Err("Amethys a envoyé une réponse inattendue en lisant la bibliothèque.".into()),
    };

    let install_path = game.install_path.to_string_lossy().into_owned();
    if saved_games.iter().any(|saved| {
        saved.platform.eq_ignore_ascii_case(game.platform)
            && saved.install_path == install_path
    }) {
        println!("{} est déjà présent dans la bibliothèque Amethys.", game.name);
        return Ok(());
    }

    let command = IpcCommand::AddGame {
        name: game.name.clone(),
        platform: game.platform.to_owned(),
        install_path,
        executable: None,
    };
    let mut connection = connect_to_core().await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;
    let response = send_request(&mut connection, IpcRequest::new(command)).await?;
    match response.result {
        Ok(IpcResponsePayload::GameAdded { game }) => {
            println!("{} a été ajouté à la bibliothèque Amethys.", game.name);
            Ok(())
        }
        Err(remote_error) => Err(format!(
            "Amethys n'a pas pu ajouter le jeu ({}): {}",
            remote_error.code, remote_error.message
        ).into()),
        Ok(_) => Err("Amethys a envoyé une réponse inattendue pendant l'ajout du jeu.".into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if let [group, action, app_id] = arguments.as_slice() {
        if group == "games" && action == "import" {
            return import_detected_steam_game(app_id).await;
        }
    }
    if arguments.as_slice() == ["games", "scan"] {
        let games = amethys_discovery::discover_steam_games();
        if games.is_empty() {
            println!("Aucun jeu Steam installé n'a été détecté.");
        } else {
            println!("Jeux Steam détectés (aucun ajout automatique) :");
            for game in games {
                println!("- {} (AppID {}) — {}", game.name, game.app_id, game.install_path.display());
            }
        }
        return Ok(());
    }

    let command = parse_arguments(&arguments)?;

    let mut connection = connect_to_core().await.map_err(|error| {
        format!("Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}")
    })?;
    let response = send_request(&mut connection, IpcRequest::new(command.clone())).await?;

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
                    println!("- {} ({}) — {}", game.name, game.platform, game.install_path);
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
    use super::{parse_arguments, parse_command, status_label};
    use amethys_ipc::{IpcCommand, IpcCoreStatus};

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
