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
    games list --platform <nom>       Filtre la bibliothèque par plateforme
    games search <terme>              Recherche dans la bibliothèque Amethys
    games scan [--verbose|--json]      Détecte les jeux sans les importer
    games import <appid>              Importe un jeu Steam par son App ID
    games import <plateforme> <id>    Importe un jeu détecté par plateforme et ID
    games import-all                  Importe les jeux détectés non encore enregistrés
    games verify [--json]             Vérifie les chemins sans modifier les installations
    games add <nom> <plateforme> <dossier> [exécutable]
    games remove <id>                 Retire un jeu de la bibliothèque Amethys

OPTIONS:
    -h, --help                        Affiche cette aide
    -V, --version                     Affiche la version

ENVIRONNEMENT:
    AMETHYS_DATA_DIR                  Remplace le dossier de données par défaut
    AMETHYS_EA_LIBRARY_ROOTS          Racines EA personnalisées séparées par des points-virgules"
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
    let mut connection =
        tokio::time::timeout(std::time::Duration::from_secs(5), connect_to_core()).await??;

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
        [group, action, name, platform, install_path, executable]
            if group == "games" && action == "add" =>
        {
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
        _ => Err(
            "Utilisation : amethysctl games add <nom> <plateforme> <dossier> [executable] | games remove <id> | games scan | games import <appid> | games import <plateforme> <id> | games import-all | games verify [--json]",
        ),
    }
}

/// Compares saved and detected installation paths after resolving aliases.
/// Windows paths are case-insensitive; other platforms retain native path rules.
fn same_install_location(saved: &str, detected: &std::path::Path) -> bool {
    let saved = std::path::Path::new(saved);
    let saved = saved.canonicalize().unwrap_or_else(|_| saved.to_path_buf());
    let detected = detected
        .canonicalize()
        .unwrap_or_else(|_| detected.to_path_buf());

    #[cfg(windows)]
    {
        saved
            .to_string_lossy()
            .eq_ignore_ascii_case(&detected.to_string_lossy())
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
            && (matches!(
                (&saved.platform_id, &detected.platform_id),
                (Some(saved_id), Some(detected_id)) if saved_id == detected_id
            ) || same_install_location(&saved.install_path, &detected.install_path))
    })
}

async fn load_saved_games() -> Result<Vec<amethys_ipc::IpcGameEntry>, Box<dyn Error>> {
    let response = request_core(IpcCommand::ListGames).await.map_err(|error| {
        format!(
            "Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}"
        )
    })?;

    match response.result {
        Ok(IpcResponsePayload::Games { games }) => Ok(games),
        Err(remote_error) => Err(format!(
            "Impossible de vérifier la bibliothèque ({}): {}",
            remote_error.code, remote_error.message
        )
        .into()),
        Ok(_) => Err("Amethys a envoyé une réponse inattendue en lisant la bibliothèque.".into()),
    }
}

/// Filters the saved library by platform name without changing stored entries.
fn filter_games_by_platform<'a>(
    games: &'a [amethys_ipc::IpcGameEntry],
    platform: &str,
) -> Vec<&'a amethys_ipc::IpcGameEntry> {
    let platform = platform.trim();
    if platform.is_empty() {
        return Vec::new();
    }
    games
        .iter()
        .filter(|game| game.platform.eq_ignore_ascii_case(platform))
        .collect()
}

async fn list_games_for_platform(platform: &str) -> Result<(), Box<dyn Error>> {
    if platform.trim().is_empty() {
        return Err("Indique une plateforme : amethysctl games list --platform <nom>.".into());
    }
    let games = load_saved_games().await?;
    let matches = filter_games_by_platform(&games, platform);
    if matches.is_empty() {
        println!(
            "Aucun jeu enregistré pour la plateforme « {} ».",
            platform.trim()
        );
        return Ok(());
    }
    println!(
        "{} jeu(x) enregistré(s) sur {} :",
        matches.len(),
        platform.trim()
    );
    for game in matches {
        println!(
            "- {}{} — {}",
            game.name,
            game.platform_id
                .as_deref()
                .map(|id| format!(" — ID {id}"))
                .unwrap_or_default(),
            game.install_path
        );
    }
    Ok(())
}

/// Searches saved game metadata without requiring any launcher to be running.
/// Every query word must match somewhere in the combined game metadata.
fn search_saved_games<'a>(
    games: &'a [amethys_ipc::IpcGameEntry],
    query: &str,
) -> Vec<&'a amethys_ipc::IpcGameEntry> {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Vec::new();
    }

    games
        .iter()
        .filter(|game| {
            let searchable = format!(
                "{} {} {} {} {}",
                game.name,
                game.platform,
                game.platform_id.as_deref().unwrap_or_default(),
                game.install_path,
                game.executable.as_deref().unwrap_or_default(),
            )
            .to_lowercase();
            terms.iter().all(|term| searchable.contains(term))
        })
        .collect()
}

async fn search_library(query: &str) -> Result<(), Box<dyn Error>> {
    if query.trim().is_empty() {
        return Err("Indique un terme à rechercher : amethysctl games search <terme>.".into());
    }

    let games = load_saved_games().await?;
    let matches = search_saved_games(&games, query);
    if matches.is_empty() {
        println!("Aucun jeu ne correspond à « {} ».", query.trim());
        return Ok(());
    }

    println!("{} résultat(s) pour « {} » :", matches.len(), query.trim());
    for game in matches {
        println!(
            "- {} ({}){} — {}",
            game.name,
            game.platform,
            game.platform_id
                .as_deref()
                .map(|id| format!(" — ID {id}"))
                .unwrap_or_default(),
            game.install_path,
        );
    }
    Ok(())
}

async fn import_one_detected_game(
    game: &amethys_discovery::DetectedGame,
    saved_games: &mut Vec<amethys_ipc::IpcGameEntry>,
) -> Result<bool, Box<dyn Error>> {
    if game_is_already_imported(saved_games, game) {
        println!(
            "{} est déjà présent dans la bibliothèque Amethys.",
            game.name
        );
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
        format!(
            "Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}"
        )
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
        )
        .into()),
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
        )
        .into())
    }
}

/// Lists the launcher adapters enabled for the current operating system.
fn supported_platforms() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "Steam",
            "Epic Games",
            "Heroic",
            "GOG Galaxy",
            "Amazon Games",
            "EA app",
            "Ubisoft Connect",
            "Battle.net",
            "Riot Games",
            "Xbox app",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        &["Steam", "Epic Games", "Heroic", "GOG Galaxy"]
    }
    #[cfg(target_os = "linux")]
    {
        &["Steam", "Heroic"]
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        &[]
    }
}

/// Builds a versioned, machine-readable report from actual local discovery results.
/// Paths are emitted as strings so the report can be consumed by support tools on any OS.
fn build_scan_report(games: &[amethys_discovery::DetectedGame]) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "total_detected": games.len(),
        "read_only": true,
        "detected_games": games.iter().map(|game| serde_json::json!({
            "name": game.name,
            "platform": game.platform,
            "platform_id": game.platform_id,
            "install_path": game.install_path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    })
}

fn print_scan_json(games: &[amethys_discovery::DetectedGame]) -> Result<(), Box<dyn Error>> {
    let report = build_scan_report(games);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Prints a real discovery result, optionally including zero-result adapters.
fn print_scan_results(games: &[amethys_discovery::DetectedGame], verbose: bool) {
    if games.is_empty() {
        println!("Aucun jeu installé n'a été détecté dans les plateformes prises en charge.");
    } else {
        println!("Jeux détectés (aucun ajout automatique) :");
        for game in games {
            println!(
                "- {} [{}]{} — {}",
                game.name,
                game.platform,
                game.platform_id
                    .as_deref()
                    .map(|id| format!(" — ID {id}"))
                    .unwrap_or_default(),
                game.install_path.display()
            );
        }
        println!("Total : {} jeu(x) détecté(s).", games.len());
    }

    if verbose {
        println!("\nRésultat par plateforme :");
        for platform in supported_platforms() {
            let count = games
                .iter()
                .filter(|game| game.platform.eq_ignore_ascii_case(platform))
                .count();
            println!("- {platform} : {count} jeu(x) détecté(s)");
        }
        println!(
            "Résultats issus des métadonnées locales réellement trouvées ; aucun jeu fictif n'est ajouté."
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathCheck {
    Available,
    Missing,
    WrongType,
    Inaccessible,
    InvalidPath,
    Error,
}

impl PathCheck {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Missing => "missing",
            Self::WrongType => "wrong_type",
            Self::Inaccessible => "inaccessible",
            Self::InvalidPath => "invalid_path",
            Self::Error => "error",
        }
    }

    const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Inspects a path without opening, changing, or creating its contents.
fn inspect_path(path: &std::path::Path, expect_directory: bool) -> PathCheck {
    match std::fs::metadata(path) {
        Ok(metadata) if expect_directory && metadata.is_dir() => PathCheck::Available,
        Ok(metadata) if !expect_directory && metadata.is_file() => PathCheck::Available,
        Ok(_) => PathCheck::WrongType,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PathCheck::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            PathCheck::Inaccessible
        }
        Err(_) => PathCheck::Error,
    }
}

/// Rejects relative executable paths that could escape the registered game directory.
fn is_safe_relative_executable(executable: &str) -> bool {
    let trimmed = executable.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return false;
    }

    // Check both separator styles so Windows metadata is validated consistently
    // even when fixtures are evaluated on a non-Windows CI runner.
    let mut components = trimmed.split(['/', '\\']);
    let Some(first) = components.next() else {
        return false;
    };
    !first.contains(':')
        && !components
            .chain(std::iter::once(first))
            .any(|component| component == "." || component == "..")
}

fn inspect_game_executable(game: &amethys_ipc::IpcGameEntry) -> Option<PathCheck> {
    let executable = game.executable.as_deref()?;
    let executable_path = std::path::Path::new(executable);
    let resolved = if executable_path.is_absolute() {
        executable_path.to_path_buf()
    } else {
        if !is_safe_relative_executable(executable) {
            return Some(PathCheck::InvalidPath);
        }
        std::path::Path::new(&game.install_path).join(executable_path)
    };
    Some(inspect_path(&resolved, false))
}

/// Creates a versioned, read-only report of saved installation directories and
/// configured executables. Directory availability remains separate from executable status.
fn build_verify_report(games: &[amethys_ipc::IpcGameEntry]) -> serde_json::Value {
    let entries = games
        .iter()
        .map(|game| {
            let directory_status = inspect_path(std::path::Path::new(&game.install_path), true);
            let executable_status = inspect_game_executable(game);
            let executable_label = executable_status
                .map(PathCheck::as_str)
                .unwrap_or("not_configured");
            let healthy = directory_status.is_available()
                && executable_status.is_none_or(PathCheck::is_available);
            serde_json::json!({
                "id": game.id,
                "name": game.name,
                "platform": game.platform,
                "platform_id": game.platform_id,
                "install_path": game.install_path,
                "directory_available": directory_status.is_available(),
                "directory_status": directory_status.as_str(),
                "executable": game.executable,
                "executable_status": executable_label,
                "healthy": healthy,
            })
        })
        .collect::<Vec<_>>();
    let available = entries
        .iter()
        .filter(|entry| entry["directory_available"] == true)
        .count();
    let healthy = entries
        .iter()
        .filter(|entry| entry["healthy"] == true)
        .count();
    let issues = entries.len() - healthy;
    json_verify_summary(games.len(), available, healthy, issues, entries)
}

fn json_verify_summary(
    total: usize,
    available: usize,
    healthy: usize,
    issues: usize,
    games: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 2,
        "read_only": true,
        "total_games": total,
        "available": available,
        "missing": total - available,
        "healthy": healthy,
        "issues": issues,
        "games": games,
    })
}

async fn verify_saved_games(json_output: bool) -> Result<(), Box<dyn Error>> {
    let games = load_saved_games().await?;
    let report = build_verify_report(&games);
    let total = games.len();
    let available = report["available"].as_u64().unwrap_or(0) as usize;
    let healthy = report["healthy"].as_u64().unwrap_or(0) as usize;
    let issues = total - healthy;
    let missing = total - available;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if games.is_empty() {
        println!("Aucun jeu enregistré à vérifier.");
    } else {
        for game in &games {
            let directory_status = inspect_path(std::path::Path::new(&game.install_path), true);
            let executable_status = inspect_game_executable(game);
            let healthy = directory_status.is_available()
                && executable_status.is_none_or(PathCheck::is_available);
            let marker = if healthy { "OK" } else { "ATTENTION" };
            println!(
                "[{marker}] {} ({}) — dossier : {}",
                game.name,
                game.platform,
                directory_status.as_str()
            );
            if let Some(status) = executable_status {
                println!("       Exécutable : {}", status.as_str());
            }
            if !directory_status.is_available() {
                println!("       Chemin : {}", game.install_path);
            }
        }
        println!(
            "Vérification terminée : {healthy}/{total} installation(s) cohérente(s), {available}/{total} dossier(s) disponible(s), {issues} anomalie(s). Aucun fichier n'a été modifié."
        );
    }

    if missing > 0 || issues > 0 {
        Err(format!("{issues} installation(s) nécessitent une vérification.").into())
    } else {
        Ok(())
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
    if arguments.is_empty() || arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"]
    {
        print_help();
        return Ok(());
    }
    if arguments.as_slice() == ["--version"] || arguments.as_slice() == ["-V"] {
        println!("Amethys Launcher {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if arguments.len() == 4
        && arguments[0] == "games"
        && arguments[1] == "list"
        && arguments[2] == "--platform"
    {
        return list_games_for_platform(&arguments[3]).await;
    }
    if arguments.len() == 3
        && arguments[0] == "games"
        && arguments[1] == "list"
        && arguments[2] == "--platform"
    {
        return Err("Indique une plateforme : amethysctl games list --platform <nom>.".into());
    }
    if arguments.len() >= 2 && arguments[0] == "games" && arguments[1] == "search" {
        return search_library(&arguments[2..].join(" ")).await;
    }
    if arguments.as_slice() == ["games", "import-all"] {
        return import_detected_games(amethys_discovery::discover_all_games()).await;
    }
    if arguments.as_slice() == ["games", "verify"] {
        return verify_saved_games(false).await;
    }
    if arguments.as_slice() == ["games", "verify", "--json"] {
        return verify_saved_games(true).await;
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
    if arguments.as_slice() == ["games", "scan"]
        || arguments.as_slice() == ["games", "scan", "--verbose"]
        || arguments.as_slice() == ["games", "scan", "--json"]
    {
        let games = amethys_discovery::discover_all_games();
        if arguments
            .last()
            .is_some_and(|argument| argument == "--json")
        {
            return print_scan_json(&games);
        }
        print_scan_results(&games, arguments.len() == 3);
        return Ok(());
    }

    let command = parse_arguments(&arguments)?;

    let response = request_core(command.clone()).await.map_err(|error| {
        format!(
            "Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}"
        )
    })?;

    match response.result {
        Err(remote_error) => Err(format!(
            "Amethys a renvoyé une erreur ({}): {}",
            remote_error.code, remote_error.message
        )
        .into()),
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
                        game.platform_id
                            .as_deref()
                            .map(|id| format!(" — ID {id}"))
                            .unwrap_or_default(),
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
    use super::{
        PathCheck, inspect_path, parse_arguments, parse_command, same_install_location,
        status_label,
    };
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

        assert!(super::game_is_already_imported(
            std::slice::from_ref(&saved),
            &same_game
        ));
        assert!(!super::game_is_already_imported(
            std::slice::from_ref(&saved),
            &other_store
        ));

        // Store metadata can change (for example after a game is moved or
        // re-registered). A matching installation path must still prevent a
        // duplicate even when both platform identifiers are present.
        let same_path_different_id = amethys_discovery::DetectedGame {
            platform_id: Some("999".to_owned()),
            name: "Example".to_owned(),
            install_path: std::path::PathBuf::from("/games/example"),
            platform: "Steam",
        };
        assert!(super::game_is_already_imported(
            std::slice::from_ref(&saved),
            &same_path_different_id
        ));

        let different_path_different_id = amethys_discovery::DetectedGame {
            platform_id: Some("999".to_owned()),
            name: "Example".to_owned(),
            install_path: std::path::PathBuf::from("/another/game"),
            platform: "Steam",
        };
        assert!(!super::game_is_already_imported(
            &[saved],
            &different_path_different_id
        ));
    }

    #[test]
    fn recognizes_installation_path_aliases_without_merging_distinct_paths() {
        let root = std::env::temp_dir().join(format!(
            "amethys-cli-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create installation directory");
        let existing = root.to_string_lossy().into_owned();
        let missing = root.join("missing-game").to_string_lossy().into_owned();

        assert_eq!(
            inspect_path(std::path::Path::new(&existing), true),
            PathCheck::Available
        );
        assert_eq!(
            inspect_path(std::path::Path::new(&missing), true),
            PathCheck::Missing
        );
        assert!(
            root.is_dir(),
            "verification must leave the installation untouched"
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn verification_rejects_a_file_where_an_installation_directory_is_expected() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create test root");
        let file = root.join("not-a-game-folder.exe");
        std::fs::write(&file, b"sentinel").expect("create regular file");

        assert_eq!(inspect_path(&file, true), PathCheck::WrongType);
        assert_eq!(std::fs::read(&file).expect("file remains"), b"sentinel");

        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn verification_json_report_is_versioned_and_read_only() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-json-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create installation");
        let game = amethys_ipc::IpcGameEntry {
            id: "game-1".to_owned(),
            name: "Example".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("123".to_owned()),
            install_path: root.to_string_lossy().into_owned(),
            executable: None,
        };
        let report = super::build_verify_report(&[game]);
        assert_eq!(report["schema_version"], 2);
        assert_eq!(report["read_only"], true);
        assert_eq!(report["total_games"], 1);
        assert_eq!(report["available"], 1);
        assert_eq!(report["missing"], 0);
        assert_eq!(report["healthy"], 1);
        assert_eq!(report["issues"], 0);
        assert_eq!(report["games"][0]["directory_available"], true);
        assert_eq!(report["games"][0]["directory_status"], "available");
        assert_eq!(report["games"][0]["executable_status"], "not_configured");
        assert_eq!(report["games"][0]["healthy"], true);
        std::fs::remove_dir_all(root).expect("remove test installation");
    }

    #[test]
    fn verification_checks_configured_executables_without_modifying_them() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-executable-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("bin")).expect("create install and bin");
        std::fs::write(root.join("bin/game.exe"), b"sentinel").expect("create executable fixture");
        let make_game = |executable: &str| amethys_ipc::IpcGameEntry {
            id: "game-exe".to_owned(),
            name: "Example".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            install_path: root.to_string_lossy().into_owned(),
            executable: Some(executable.to_owned()),
        };
        let present = super::build_verify_report(&[make_game("bin/game.exe")]);
        assert_eq!(present["games"][0]["executable_status"], "available");
        assert_eq!(present["games"][0]["healthy"], true);
        let missing = super::build_verify_report(&[make_game("bin/missing.exe")]);
        assert_eq!(missing["games"][0]["executable_status"], "missing");
        assert_eq!(missing["games"][0]["healthy"], false);
        assert_eq!(
            std::fs::read(root.join("bin/game.exe")).expect("fixture remains"),
            b"sentinel"
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn verification_rejects_relative_executable_path_traversal() {
        let game = |executable: &str| amethys_ipc::IpcGameEntry {
            id: "game-path".to_owned(),
            name: "Example".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            install_path: "C:/Games/Example".to_owned(),
            executable: Some(executable.to_owned()),
        };

        for unsafe_path in [
            "../outside.exe",
            r"..\outside.exe",
            "bin/../../outside.exe",
            r"C:outside.exe",
        ] {
            assert_eq!(
                super::inspect_game_executable(&game(unsafe_path)),
                Some(super::PathCheck::InvalidPath),
                "must reject {unsafe_path}"
            );
        }
        assert!(super::is_safe_relative_executable("bin/game.exe"));
    }

    #[test]
    fn verification_distinguishes_missing_paths_from_non_directories() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture");
        let file = root.join("not-a-directory");
        std::fs::write(&file, b"sentinel").expect("create file");
        let missing = root.join("missing");
        assert_eq!(
            super::inspect_path(&missing, true),
            super::PathCheck::Missing
        );
        assert_eq!(
            super::inspect_path(&file, true),
            super::PathCheck::WrongType
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn scan_json_report_has_a_versioned_read_only_schema() {
        let game = amethys_discovery::DetectedGame {
            platform_id: Some("123".to_owned()),
            name: "Example".to_owned(),
            install_path: std::path::PathBuf::from("/games/example"),
            platform: "Steam",
        };
        let report = super::build_scan_report(&[game]);
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["total_detected"], 1);
        assert_eq!(report["read_only"], true);
        assert_eq!(report["detected_games"][0]["name"], "Example");
        assert_eq!(report["detected_games"][0]["platform"], "Steam");
        assert_eq!(report["detected_games"][0]["platform_id"], "123");
        assert_eq!(
            report["detected_games"][0]["install_path"],
            "/games/example"
        );
    }

    #[test]
    fn scan_summary_includes_platforms_supported_on_current_os() {
        let platforms = super::supported_platforms();

        #[cfg(windows)]
        let expected: &[&str] = &[
            "Steam",
            "Epic Games",
            "Heroic",
            "GOG Galaxy",
            "Amazon Games",
            "EA app",
            "Ubisoft Connect",
            "Battle.net",
            "Riot Games",
            "Xbox app",
        ];
        #[cfg(target_os = "macos")]
        let expected: &[&str] = &["Steam", "Heroic", "GOG Galaxy"];
        #[cfg(target_os = "linux")]
        let expected: &[&str] = &["Steam", "Heroic"];

        for platform in expected {
            assert!(
                platforms
                    .iter()
                    .any(|supported| supported.eq_ignore_ascii_case(platform)),
                "scan summary must include {platform} on this operating system"
            );
        }
        assert_eq!(platforms.len(), expected.len());
    }

    #[test]
    fn filters_library_by_platform_case_insensitively() {
        let games = vec![
            amethys_ipc::IpcGameEntry {
                id: "steam-1".to_owned(),
                name: "Example".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("10".to_owned()),
                install_path: "A:/Steam/Example".to_owned(),
                executable: None,
            },
            amethys_ipc::IpcGameEntry {
                id: "epic-1".to_owned(),
                name: "Example".to_owned(),
                platform: "Epic Games".to_owned(),
                platform_id: Some("10".to_owned()),
                install_path: "A:/Epic/Example".to_owned(),
                executable: None,
            },
        ];
        let steam = super::filter_games_by_platform(&games, "sTEAM");
        assert_eq!(steam.len(), 1);
        assert_eq!(steam[0].id, "steam-1");
        assert!(super::filter_games_by_platform(&games, "GOG").is_empty());
        assert!(super::filter_games_by_platform(&games, "  ").is_empty());
    }

    #[test]
    fn searches_library_metadata_case_insensitively_and_with_multiple_terms() {
        let games = vec![
            amethys_ipc::IpcGameEntry {
                id: "steam-1".to_owned(),
                name: "Clair Obscur: Expedition 33".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("1903340".to_owned()),
                install_path: "A:/SteamLibrary/steamapps/common/Expedition 33".to_owned(),
                executable: Some("Expedition33.exe".to_owned()),
            },
            amethys_ipc::IpcGameEntry {
                id: "epic-1".to_owned(),
                name: "Fortnite".to_owned(),
                platform: "Epic Games".to_owned(),
                platform_id: Some("Fortnite".to_owned()),
                install_path: "A:/Epic Games/Fortnite".to_owned(),
                executable: None,
            },
        ];

        let by_name = super::search_saved_games(&games, "CLAIR obscur");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].id, "steam-1");

        let by_platform_and_id = super::search_saved_games(&games, "steam 1903340");
        assert_eq!(by_platform_and_id.len(), 1);
        assert_eq!(by_platform_and_id[0].id, "steam-1");

        let by_path = super::search_saved_games(&games, "epic games fortnite");
        assert_eq!(by_path.len(), 1);
        assert_eq!(by_path[0].id, "epic-1");

        assert!(super::search_saved_games(&games, "missing title").is_empty());
        assert!(super::search_saved_games(&games, "   ").is_empty());
    }

    #[test]
    fn accepts_supported_commands() {
        assert_eq!(parse_command(Some("ping")), Ok(IpcCommand::Ping));
        assert_eq!(parse_command(Some("status")), Ok(IpcCommand::GetStatus));
        assert_eq!(parse_command(Some("games")), Ok(IpcCommand::ListGames));
    }

    #[test]
    fn parses_game_management_commands() {
        let add = vec![
            "games".to_owned(),
            "add".to_owned(),
            "Game".to_owned(),
            "Steam".to_owned(),
            "/games/game".to_owned(),
            "game.exe".to_owned(),
        ];
        assert_eq!(
            parse_arguments(&add),
            Ok(IpcCommand::AddGame {
                name: "Game".to_owned(),
                platform: "Steam".to_owned(),
                install_path: "/games/game".to_owned(),
                executable: Some("game.exe".to_owned()),
            })
        );
        let remove = vec![
            "games".to_owned(),
            "remove".to_owned(),
            "game-id".to_owned(),
        ];
        assert_eq!(
            parse_arguments(&remove),
            Ok(IpcCommand::RemoveGame {
                id: "game-id".to_owned()
            })
        );
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
