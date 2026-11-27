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
    games launch <id>                 Démarre un exécutable configuré et vérifié
    games set-executable <id> <chemin> Configure l'exécutable d'un jeu importé
    games add <nom> <plateforme> <dossier> [exécutable]
    games remove <id>                 Retire un jeu de la bibliothèque Amethys
    catalog sync                      Télécharge et vérifie l'index catalogue mondial
    catalog search <terme>            Recherche hors ligne dans le catalogue mondial
    catalog show <amethys-id>         Affiche la fiche détaillée d'un jeu du catalogue

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
        Some("catalog-sync") => Ok(IpcCommand::SyncMasterCatalog),
        _ => Err(
            "Commande inconnue. Utilise « amethysctl --help » pour afficher les commandes disponibles.",
        ),
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
    let mut connection = tokio::time::timeout(std::time::Duration::from_secs(5), connect_to_core())
        .await
        .map_err(|_| {
            "Délai dépassé : impossible de se connecter au cœur Amethys après 5 secondes."
                .to_owned()
        })?
        .map_err(|error| format!("Connexion au cœur Amethys impossible : {error}"))?;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        send_request(&mut connection, IpcRequest::new(command)),
    )
    .await
    .map_err(|_| "Le cœur Amethys n'a pas répondu dans le délai de 15 secondes.".to_owned())?
    .map_err(|error| format!("Échange IPC avec le cœur Amethys impossible : {error}"))?;

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
        [group, action] if group == "games" && action == "list" => Ok(IpcCommand::ListGames),
        [group, action] if group == "catalog" && action == "sync" => {
            Ok(IpcCommand::SyncMasterCatalog)
        }
        [group, action, query] if group == "catalog" && action == "search" => {
            Ok(IpcCommand::SearchMasterCatalog {
                query: query.clone(),
                limit: 50,
            })
        }
        [group, action, id] if group == "catalog" && action == "show" => {
            let amethys_id = id
                .parse::<u64>()
                .map_err(|_| "L'identifiant Amethys doit être un entier positif.")?;
            if amethys_id == 0 {
                return Err("L'identifiant Amethys doit être un entier positif.");
            }
            Ok(IpcCommand::GetMasterCatalogGame { amethys_id })
        }
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
        [group, action, id, executable] if group == "games" && action == "set-executable" => {
            Ok(IpcCommand::SetGameExecutable {
                id: id.clone(),
                executable: executable.clone(),
            })
        }
        _ => Err(
            "Utilisation : amethysctl games add <nom> <plateforme> <dossier> [executable] | games set-executable <id> <chemin> | games remove <id> | games scan | games import <appid> | games import <plateforme> <id> | games import-all | games verify [--json]",
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
        if !saved.platform.eq_ignore_ascii_case(detected.platform) {
            return false;
        }

        match (&saved.platform_id, &detected.platform_id) {
            // A known platform identity is authoritative: a different ID must
            // not be hidden merely because a launcher reports the same path.
            (Some(saved_id), Some(detected_id)) => saved_id == detected_id,
            // Fall back to the installation path only when at least one
            // observation has no stable platform ID.
            _ => same_install_location(&saved.install_path, &detected.install_path),
        }
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

/// Formats the primary and secondary install paths consistently for CLI output.
fn format_install_locations(game: &amethys_ipc::IpcGameEntry) -> String {
    let mut locations = Vec::with_capacity(1 + game.install_locations.len());
    for path in std::iter::once(&game.install_path).chain(game.install_locations.iter()) {
        if !locations
            .iter()
            .any(|known: &String| same_install_location(known, std::path::Path::new(path)))
        {
            locations.push(path.clone());
        }
    }
    if locations.len() <= 1 {
        return game.install_path.clone();
    }
    locations
        .iter()
        .enumerate()
        .map(|(index, path)| {
            format!(
                "{}: {}",
                if index == 0 { "principal" } else { "copie" },
                path
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
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
            "- {}{} — {} — ID interne : {}",
            game.name,
            game.platform_id
                .as_deref()
                .map(|id| format!(" — ID {id}"))
                .unwrap_or_default(),
            format_install_locations(game),
            game.id
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
        .filter(|game| amethys_ipc::game_matches_search_query(game, query))
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
            "- {} ({}){} — {} — ID interne : {}",
            game.name,
            game.platform,
            game.platform_id
                .as_deref()
                .map(|id| format!(" — ID {id}"))
                .unwrap_or_default(),
            format_install_locations(game),
            game.id,
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
        executable: inferred_executable(game),
    };
    let response = request_core(command).await.map_err(|error| {
        format!(
            "Impossible de joindre Amethys. Vérifie que le programme fonctionne. Détail : {error}"
        )
    })?;
    match response.result {
        Ok(IpcResponsePayload::GameAdded { game }) => {
            println!("{} a été ajouté à la bibliothèque Amethys.", game.name);
            saved_games.push(*game);
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
    #[cfg(target_os = "linux")]
    {
        &["Steam", "Heroic"]
    }
    #[cfg(not(any(windows, target_os = "linux")))]
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
    components
        .chain(std::iter::once(first))
        .all(|component| !component.contains(':') && component != "." && component != "..")
}

/// Resolves a configured executable and guarantees it stays inside the registered install.
/// Finds an executable's path relative to any registered installation.
/// Lexical matching is intentionally attempted before canonicalization so a
/// missing primary directory can still identify the executable in a surviving copy.
fn executable_relative_to_known_install(
    game: &amethys_ipc::IpcGameEntry,
    executable: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let resolved = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_path_buf());
    for location in std::iter::once(&game.install_path).chain(game.install_locations.iter()) {
        let known_root = std::path::Path::new(location);
        if let Ok(relative) = executable.strip_prefix(known_root) {
            return Some(relative.to_path_buf());
        }
        if let Ok(canonical_root) = known_root.canonicalize() {
            if let Ok(relative) = resolved.strip_prefix(&canonical_root) {
                return Some(relative.to_path_buf());
            }
        }
    }
    None
}

fn resolve_launch_executable(
    game: &amethys_ipc::IpcGameEntry,
) -> Result<std::path::PathBuf, String> {
    let executable = game
        .executable
        .as_deref()
        .ok_or_else(|| "Aucun exécutable n'est configuré pour ce jeu.".to_owned())?;
    let root = std::path::Path::new(&game.install_path)
        .canonicalize()
        .map_err(|error| format!("Dossier du jeu indisponible : {error}"))?;
    if !root.is_dir() {
        return Err("Le chemin d'installation n'est pas un dossier.".to_owned());
    }

    let configured = std::path::Path::new(executable);
    let candidate = if configured.is_absolute() {
        executable_relative_to_known_install(game, configured)
            .map(|path| root.join(path))
            .unwrap_or_else(|| configured.to_path_buf())
    } else {
        if !is_safe_relative_executable(executable) {
            return Err("Chemin d'exécutable relatif non sûr.".to_owned());
        }
        root.join(configured)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("Exécutable introuvable ou inaccessible : {error}"))?;
    if !resolved.starts_with(&root) {
        return Err(
            "L'exécutable se trouve en dehors du dossier du jeu ; démarrage refusé.".to_owned(),
        );
    }
    if !resolved.is_file() {
        return Err("Le chemin configuré ne désigne pas un fichier exécutable.".to_owned());
    }
    Ok(resolved)
}

fn inspect_game_executable(game: &amethys_ipc::IpcGameEntry) -> Option<PathCheck> {
    let executable = game.executable.as_deref()?;
    let install_root = std::path::Path::new(&game.install_path);
    let canonical_root = match install_root.canonicalize() {
        Ok(path) if path.is_dir() => path,
        Ok(_) => return Some(PathCheck::WrongType),
        Err(_) => return Some(inspect_path(install_root, true)),
    };
    let executable_path = std::path::Path::new(executable);
    let candidate = if executable_path.is_absolute() {
        executable_relative_to_known_install(game, executable_path)
            .map(|path| canonical_root.join(path))
            .unwrap_or_else(|| executable_path.to_path_buf())
    } else {
        if !is_safe_relative_executable(executable) {
            return Some(PathCheck::InvalidPath);
        }
        canonical_root.join(executable_path)
    };
    let canonical_executable = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            // A missing executable outside the game directory is still an
            // invalid configuration, not merely a missing in-game file.
            // An absolute path containing ".." cannot be safely classified
            // from an existing ancestor once an earlier component is missing:
            // the unresolved traversal may escape the installation root.
            // Existing paths are canonicalized above, so valid in-root paths
            // with parent components remain accepted.
            if candidate.is_absolute()
                && candidate
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return Some(PathCheck::InvalidPath);
            }

            // Resolve the nearest existing ancestor so symlinked parents are
            // handled consistently without creating or modifying anything.
            let mut ancestor = candidate.as_path();
            while let Some(parent) = ancestor.parent() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    if !canonical_parent.starts_with(&canonical_root) {
                        return Some(PathCheck::InvalidPath);
                    }
                    return Some(inspect_path(&candidate, false));
                }
                ancestor = parent;
            }
            return Some(PathCheck::Missing);
        }
    };
    if !canonical_executable.starts_with(&canonical_root) {
        return Some(PathCheck::InvalidPath);
    }
    Some(inspect_path(&canonical_executable, false))
}

/// Creates a versioned, read-only report of saved installation directories and
/// configured executables. Directory availability remains separate from executable status.
fn build_verify_report(games: &[amethys_ipc::IpcGameEntry]) -> serde_json::Value {
    let entries = games
        .iter()
        .map(|game| {
            let mut unique_locations = Vec::new();
            for install_path in
                std::iter::once(&game.install_path).chain(game.install_locations.iter())
            {
                if !unique_locations.iter().any(|known: &String| {
                    same_install_location(known, std::path::Path::new(install_path))
                }) {
                    unique_locations.push(install_path.clone());
                }
            }
            let installations = unique_locations
                .iter()
                .enumerate()
                .map(|(index, install_path)| {
                    let mut candidate = game.clone();
                    candidate.install_path = install_path.clone();
                    // Resolve an absolute executable against the original set of
                    // known installations before replacing the primary path. This
                    // lets each secondary copy reuse the same safe relative path.
                    if let Some(executable) = game.executable.as_deref() {
                        let executable_path = std::path::Path::new(executable);
                        if executable_path.is_absolute() {
                            if let Some(relative) =
                                executable_relative_to_known_install(game, executable_path)
                            {
                                candidate.executable =
                                    Some(relative.to_string_lossy().into_owned());
                            }
                        }
                    }
                    let directory_status = inspect_path(std::path::Path::new(install_path), true);
                    let executable_status = inspect_game_executable(&candidate);
                    let executable_label = executable_status
                        .map(PathCheck::as_str)
                        .unwrap_or("not_configured");
                    let healthy = directory_status.is_available()
                        && executable_status.is_none_or(PathCheck::is_available);
                    let launch_ready = directory_status.is_available()
                        && executable_status.is_some_and(PathCheck::is_available);
                    serde_json::json!({
                        "kind": if index == 0 { "primary" } else { "secondary" },
                        "install_path": install_path,
                        "directory_available": directory_status.is_available(),
                        "directory_status": directory_status.as_str(),
                        "executable_status": executable_label,
                        "healthy": healthy,
                        "launch_ready": launch_ready,
                    })
                })
                .collect::<Vec<_>>();
            let primary = &installations[0];
            let installation_count = installations.len();
            let available_installations = installations
                .iter()
                .filter(|entry| entry["directory_available"] == true)
                .count();
            let missing_installations = installation_count - available_installations;
            let directory_available = available_installations > 0;
            let primary_available = primary["directory_available"] == true;
            let state = match (
                primary_available,
                available_installations == 0,
                missing_installations > 0,
            ) {
                (_, true, _) => "all_installations_missing",
                (false, false, _) => "primary_missing_secondary_available",
                (true, false, true) => "primary_available_some_copies_missing",
                (true, false, false) => "all_installations_available",
            };
            let healthy = installations.iter().any(|entry| entry["healthy"] == true);
            let launch_ready = installations
                .iter()
                .any(|entry| entry["launch_ready"] == true);
            serde_json::json!({
                "id": game.id,
                "name": game.name,
                "platform": game.platform,
                "platform_id": game.platform_id,
                "install_path": game.install_path,
                "installations": installations,
                "installation_state": state,
                "installation_count": installation_count,
                "available_installations": available_installations,
                "missing_installations": missing_installations,
                "directory_available": directory_available,
                "primary_directory_available": primary["directory_available"],
                "directory_status": primary["directory_status"],
                "executable": game.executable,
                "executable_status": primary["executable_status"],
                "healthy": healthy,
                "launch_ready": launch_ready,
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
    let launch_ready = entries
        .iter()
        .filter(|entry| entry["launch_ready"] == true)
        .count();
    let issues = entries.len() - healthy;
    let total_installations = entries
        .iter()
        .map(|entry| entry["installation_count"].as_u64().unwrap_or(0) as usize)
        .sum::<usize>();
    let available_installations = entries
        .iter()
        .map(|entry| entry["available_installations"].as_u64().unwrap_or(0) as usize)
        .sum::<usize>();
    json_verify_summary(VerifySummary {
        total: games.len(),
        available,
        healthy,
        launch_ready,
        issues,
        total_installations,
        available_installations,
        games: entries,
    })
}

struct VerifySummary {
    total: usize,
    available: usize,
    healthy: usize,
    launch_ready: usize,
    issues: usize,
    total_installations: usize,
    available_installations: usize,
    games: Vec<serde_json::Value>,
}

fn json_verify_summary(summary: VerifySummary) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 5,
        "read_only": true,
        "total_games": summary.total,
        "available": summary.available,
        "missing": summary.total - summary.available,
        "healthy": summary.healthy,
        "launch_ready": summary.launch_ready,
        "issues": summary.issues,
        "total_installations": summary.total_installations,
        "available_installations": summary.available_installations,
        "missing_installations": summary.total_installations - summary.available_installations,
        "installation_issues": summary.total_installations - summary.available_installations,
        "games": summary.games,
    })
}

async fn launch_saved_game(id: &str) -> Result<(), Box<dyn Error>> {
    if id.trim().is_empty() {
        return Err("Indique l'identifiant du jeu : amethysctl games launch <id>.".into());
    }
    let games = load_saved_games().await?;
    let game = games
        .iter()
        .find(|game| game.id == id)
        .ok_or_else(|| "Aucun jeu ne correspond à cet identifiant.".to_owned())?;
    let executable = resolve_launch_executable(game)
        .map_err(|error| format!("Impossible de démarrer « {} » : {error}", game.name))?;

    let install_root = std::path::Path::new(&game.install_path)
        .canonicalize()
        .map_err(|error| format!("Dossier du jeu indisponible : {error}"))?;
    let child = std::process::Command::new(&executable)
        .current_dir(&install_root)
        .spawn()
        .map_err(|error| format!("Le système n'a pas pu démarrer le jeu : {error}"))?;
    println!(
        "Démarrage demandé : {} (processus {}).",
        game.name,
        child.id()
    );
    Ok(())
}

async fn verify_saved_games(json_output: bool) -> Result<(), Box<dyn Error>> {
    let games = load_saved_games().await?;
    let report = build_verify_report(&games);
    let total = games.len();
    let available = report["available"].as_u64().unwrap_or(0) as usize;
    let healthy = report["healthy"].as_u64().unwrap_or(0) as usize;
    let issues = total - healthy;
    let missing = total - available;
    let missing_installations = report["missing_installations"].as_u64().unwrap_or(0) as usize;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if games.is_empty() {
        println!("Aucun jeu enregistré à vérifier.");
    } else {
        for entry in report["games"].as_array().into_iter().flatten() {
            let name = entry["name"].as_str().unwrap_or("Jeu inconnu");
            let platform = entry["platform"].as_str().unwrap_or("Plateforme inconnue");
            let state = entry["installation_state"].as_str().unwrap_or("unknown");
            println!(
                "[{}] {} ({}) — état : {} — {}/{} installation(s) disponible(s)",
                if entry["launch_ready"] == true {
                    "PRÊT"
                } else if entry["healthy"] == true {
                    "À CONFIGURER"
                } else {
                    "ATTENTION"
                },
                name,
                platform,
                state,
                entry["available_installations"].as_u64().unwrap_or(0),
                entry["installation_count"].as_u64().unwrap_or(0)
            );
            for installation in entry["installations"].as_array().into_iter().flatten() {
                println!(
                    "       {} — dossier : {} — exécutable : {} — {}",
                    installation["kind"].as_str().unwrap_or("copie"),
                    installation["directory_status"]
                        .as_str()
                        .unwrap_or("unknown"),
                    installation["executable_status"]
                        .as_str()
                        .unwrap_or("not_configured"),
                    installation["install_path"].as_str().unwrap_or("")
                );
            }
        }
        println!(
            "Vérification terminée : {healthy}/{total} jeu(x) cohérent(s),              {available}/{total} jeu(x) avec un dossier disponible, {issues} anomalie(s).              {} installation(s) disponible(s) sur {} ; {} copie(s) manquante(s).              Aucun fichier n'a été modifié.",
            report["available_installations"].as_u64().unwrap_or(0),
            report["total_installations"].as_u64().unwrap_or(0),
            missing_installations
        );
    }

    if missing > 0 || issues > 0 || missing_installations > 0 {
        Err(format!(
            "{issues} jeu(x) nécessitent une vérification ; {missing_installations} installation(s) sont manquantes."
        )
        .into())
    } else {
        Ok(())
    }
}

fn inferred_executable(game: &amethys_discovery::DetectedGame) -> Option<String> {
    amethys_discovery::resolve_game_executable(game).map(|path| path.to_string_lossy().into_owned())
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
    if let [group, action, id] = arguments.as_slice() {
        if group == "games" && action == "launch" {
            return launch_saved_game(id).await;
        }
    }
    if let [group, action, id, executable] = arguments.as_slice() {
        if group == "games" && action == "set-executable" {
            let response = request_core(IpcCommand::SetGameExecutable {
                id: id.clone(),
                executable: executable.clone(),
            })
            .await?;
            return match response.result {
                Ok(IpcResponsePayload::GameAdded { game }) => {
                    println!("Exécutable configuré pour {}.", game.name);
                    Ok(())
                }
                Err(error) => Err(format!(
                    "Configuration impossible ({}): {}",
                    error.code, error.message
                )
                .into()),
                Ok(_) => Err("Réponse inattendue du cœur Amethys.".into()),
            };
        }
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
                        "- {} ({}){} — {} — ID interne : {}",
                        game.name,
                        game.platform,
                        game.platform_id
                            .as_deref()
                            .map(|id| format!(" — ID {id}"))
                            .unwrap_or_default(),
                        format_install_locations(&game),
                        game.id
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
        Ok(IpcResponsePayload::CatalogSynchronized {
            catalog_version,
            game_count,
        }) => {
            println!(
                "Catalogue mondial synchronisé : {game_count} jeux (version {catalog_version})."
            );
            Ok(())
        }
        Ok(IpcResponsePayload::CatalogGameDetails { game }) => {
            println!(
                "{} (Amethys ID: {}, IGDB ID: {})",
                game.name,
                game.amethys_id,
                game.igdb_id
                    .map_or_else(|| "non renseigné".to_string(), |id| id.to_string())
            );
            if let Some(date) = game.release_date {
                println!("Sortie : {date}");
            }
            if !game.genres.is_empty() {
                println!("Genres : {}", game.genres.join(", "));
            }
            if let Some(summary) = game.summary {
                println!("\\n{summary}");
            }
            if let Some(cover) = game.cover_url {
                println!("\\nCouverture : {cover}");
            }
            if !game.stores.is_empty() {
                println!("\\nBoutiques :");
                for store in game.stores {
                    let identity = store
                        .external_id
                        .map(|id| format!(" — ID {id}"))
                        .unwrap_or_default();
                    let url = store.url.map(|url| format!(" — {url}")).unwrap_or_default();
                    println!("- {}{}{}", store.name.unwrap_or(store.store), identity, url);
                }
            }
            Ok(())
        }
        Ok(IpcResponsePayload::CatalogSearchResults { games }) => {
            if games.is_empty() {
                println!("Aucun résultat dans le catalogue mondial.");
            } else {
                println!("Résultats du catalogue mondial :");
                for game in games {
                    println!("- {} (Amethys ID: {})", game.name, game.amethys_id);
                }
            }
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
        PathCheck, inspect_path, parse_arguments, parse_command, resolve_launch_executable,
        same_install_location, status_label,
    };
    use amethys_ipc::{IpcCommand, IpcCoreStatus};

    #[test]
    fn matches_imported_games_by_platform_and_platform_identifier() {
        let saved = amethys_ipc::IpcGameEntry {
            id: "local-id".to_owned(),
            name: "Example".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("570".to_owned()),
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: "/games/example".to_owned(),
            install_locations: Vec::new(),
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

        // A matching path is not enough to merge two distinct, known
        // platform identities: both identifiers are authoritative.
        let same_path_different_id = amethys_discovery::DetectedGame {
            platform_id: Some("999".to_owned()),
            name: "Example".to_owned(),
            install_path: std::path::PathBuf::from("/games/example"),
            platform: "Steam",
        };
        assert!(!super::game_is_already_imported(
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
    fn verify_report_checks_primary_and_secondary_installations_independently() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-multi-{}-{nonce}",
            std::process::id()
        ));
        let primary = root.join("primary");
        let secondary = root.join("secondary");
        std::fs::create_dir_all(primary.join("bin")).expect("primary directory");
        std::fs::create_dir_all(secondary.join("bin")).expect("secondary directory");
        let executable = primary.join("bin/game.exe");
        std::fs::write(&executable, b"primary").expect("primary executable");
        std::fs::write(secondary.join("bin/game.exe"), b"secondary").expect("secondary executable");
        let game = amethys_ipc::IpcGameEntry {
            id: "multi".to_owned(),
            name: "Multi".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: primary.to_string_lossy().into_owned(),
            install_locations: vec![secondary.to_string_lossy().into_owned()],
            executable: Some(executable.to_string_lossy().into_owned()),
        };

        let report = super::build_verify_report(&[game]);
        let installations = report["games"][0]["installations"]
            .as_array()
            .expect("installations");
        assert_eq!(installations.len(), 2);
        assert_eq!(installations[0]["kind"], "primary");
        assert_eq!(installations[0]["launch_ready"], true);
        assert_eq!(installations[1]["kind"], "secondary");
        assert_eq!(installations[1]["launch_ready"], true);
        assert_eq!(report["launch_ready"], 1);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn verification_recovers_when_primary_install_is_missing_but_secondary_survives() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-recovery-{}-{nonce}",
            std::process::id()
        ));
        let missing_primary = root.join("old-primary");
        let secondary = root.join("surviving-copy");
        std::fs::create_dir_all(secondary.join("bin")).expect("secondary directory");
        std::fs::write(secondary.join("bin/game.exe"), b"secondary").expect("secondary executable");
        let game = amethys_ipc::IpcGameEntry {
            id: "recoverable".to_owned(),
            name: "Recoverable".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: missing_primary.to_string_lossy().into_owned(),
            install_locations: vec![
                missing_primary.to_string_lossy().into_owned(),
                secondary.to_string_lossy().into_owned(),
            ],
            executable: Some(
                missing_primary
                    .join("bin/game.exe")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };

        let report = super::build_verify_report(&[game]);
        let entry = &report["games"][0];
        assert_eq!(
            entry["installation_state"],
            "primary_missing_secondary_available"
        );
        assert_eq!(entry["available_installations"], 1);
        assert_eq!(entry["missing_installations"], 1);
        assert_eq!(entry["launch_ready"], true);
        assert_eq!(report["available_installations"], 1);
        assert_eq!(report["missing_installations"], 1);
        assert_eq!(report["installation_issues"], 1);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn cli_lists_primary_and_secondary_paths_without_duplicates() {
        let game = amethys_ipc::IpcGameEntry {
            id: "multi".to_owned(),
            name: "Multi".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: "/games/main".to_owned(),
            install_locations: vec!["/games/main".to_owned(), "/games/backup".to_owned()],
            executable: None,
        };
        let formatted = super::format_install_locations(&game);
        assert!(formatted.contains("principal: /games/main"));
        assert!(formatted.contains("copie: /games/backup"));
        assert_eq!(formatted.matches("/games/main").count(), 1);
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
    fn launch_resolution_rejects_paths_outside_the_installation() {
        let root = std::env::temp_dir().join(format!(
            "amethys-launch-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("game");
        std::fs::create_dir_all(install.join("bin")).expect("create installation");
        let executable = install.join("bin/game.exe");
        std::fs::write(&executable, b"game").expect("create executable fixture");
        let make_game = |path: &str| amethys_ipc::IpcGameEntry {
            id: "game-1".to_owned(),
            name: "Example".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: install.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: Some(path.to_owned()),
        };

        assert_eq!(
            resolve_launch_executable(&make_game("bin/game.exe")).expect("valid executable"),
            executable.canonicalize().expect("canonical executable")
        );
        assert!(resolve_launch_executable(&make_game("../outside.exe")).is_err());
        assert!(resolve_launch_executable(&make_game("bin/missing.exe")).is_err());

        let outside = root.join("outside.exe");
        std::fs::write(&outside, b"outside").expect("create outside executable");
        assert!(resolve_launch_executable(&make_game(&outside.to_string_lossy())).is_err());

        #[cfg(unix)]
        {
            let link = install.join("bin/outside-link.exe");
            std::os::unix::fs::symlink(&outside, &link).expect("create external symlink");
            assert!(resolve_launch_executable(&make_game("bin/outside-link.exe")).is_err());
            std::fs::remove_file(link).expect("remove symlink");
        }

        #[cfg(windows)]
        {
            let outside_directory = root.join("outside-directory");
            std::fs::create_dir_all(&outside_directory).expect("create external directory");
            std::fs::write(outside_directory.join("game.exe"), b"outside")
                .expect("create external executable");
            let junction = install.join("bin/outside-junction");
            let output = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "New-Item -ItemType Junction -Path $env:AMETHYS_JUNCTION -Target $env:AMETHYS_TARGET | Out-Null",
                ])
                .env("AMETHYS_JUNCTION", &junction)
                .env("AMETHYS_TARGET", &outside_directory)
                .output()
                .expect("create external directory junction");
            assert!(
                output.status.success(),
                "failed to create external junction: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                resolve_launch_executable(&make_game("bin/outside-junction/game.exe")).is_err(),
                "launch resolution must reject an executable reached through an external junction"
            );
        }

        assert_eq!(
            std::fs::read(&executable).expect("game file remains"),
            b"game"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
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
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: None,
        };
        let report = super::build_verify_report(&[game]);
        assert_eq!(report["schema_version"], 5);
        assert_eq!(report["read_only"], true);
        assert_eq!(report["total_games"], 1);
        assert_eq!(report["available"], 1);
        assert_eq!(report["missing"], 0);
        assert_eq!(report["healthy"], 1);
        assert_eq!(report["launch_ready"], 0);
        assert_eq!(report["issues"], 0);
        assert_eq!(report["games"][0]["directory_available"], true);
        assert_eq!(report["games"][0]["directory_status"], "available");
        assert_eq!(report["games"][0]["executable_status"], "not_configured");
        assert_eq!(report["games"][0]["healthy"], true);
        assert_eq!(report["games"][0]["launch_ready"], false);
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
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: Some(executable.to_owned()),
        };
        let present = super::build_verify_report(&[make_game("bin/game.exe")]);
        assert_eq!(present["games"][0]["executable_status"], "available");
        assert_eq!(present["games"][0]["healthy"], true);
        assert_eq!(present["games"][0]["launch_ready"], true);
        assert_eq!(present["launch_ready"], 1);
        let missing = super::build_verify_report(&[make_game("bin/missing.exe")]);
        assert_eq!(missing["games"][0]["executable_status"], "missing");
        assert_eq!(missing["games"][0]["healthy"], false);

        let outside = root
            .parent()
            .expect("parent")
            .join(format!("amethys-verify-outside-{}.exe", std::process::id()));
        std::fs::write(&outside, b"outside").expect("create outside executable");
        let external = super::build_verify_report(&[make_game(&outside.to_string_lossy())]);
        assert_eq!(external["games"][0]["executable_status"], "invalid_path");
        assert_eq!(external["games"][0]["healthy"], false);
        std::fs::remove_file(&outside).expect("remove outside executable");

        let missing_outside = root.parent().expect("parent").join(format!(
            "amethys-verify-missing-outside-{}.exe",
            std::process::id()
        ));
        let external_missing =
            super::build_verify_report(&[make_game(&missing_outside.to_string_lossy())]);
        assert_eq!(
            external_missing["games"][0]["executable_status"], "invalid_path",
            "a missing absolute path outside the installation is still unsafe"
        );
        assert_eq!(external_missing["games"][0]["healthy"], false);

        let missing_traversal = root
            .join("missing-parent")
            .join("..")
            .join("..")
            .join("outside.exe");
        let traversal_report =
            super::build_verify_report(&[make_game(&missing_traversal.to_string_lossy())]);
        assert_eq!(
            traversal_report["games"][0]["executable_status"], "invalid_path",
            "an unresolved absolute traversal must not be classified as a harmless missing file"
        );
        assert_eq!(traversal_report["games"][0]["healthy"], false);

        #[cfg(unix)]
        {
            let link = root.join("bin/outside-link.exe");
            std::os::unix::fs::symlink(root.parent().unwrap().join("outside-target.exe"), &link)
                .expect("create symlink");
            let outside_target = root.parent().unwrap().join("outside-target.exe");
            std::fs::write(&outside_target, b"outside").expect("create symlink target");
            let symlinked = super::build_verify_report(&[make_game("bin/outside-link.exe")]);
            assert_eq!(symlinked["games"][0]["executable_status"], "invalid_path");
            assert_eq!(symlinked["games"][0]["healthy"], false);
            std::fs::remove_file(link).expect("remove symlink");
            std::fs::remove_file(outside_target).expect("remove symlink target");
        }

        assert_eq!(
            std::fs::read(root.join("bin/game.exe")).expect("fixture remains"),
            b"sentinel"
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn verification_rejects_relative_executable_path_traversal() {
        let root = std::env::temp_dir().join(format!(
            "amethys-verify-traversal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create installation root");
        let game = |executable: &str| amethys_ipc::IpcGameEntry {
            id: "game-path".to_owned(),
            name: "Example".to_owned(),
            platform: "Test".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: Some(executable.to_owned()),
        };

        for unsafe_path in [
            "../outside.exe",
            r"..\\outside.exe",
            "bin/../../outside.exe",
            r"C:outside.exe",
            "bin/game.exe:alternate-stream",
            r"bin\\game.exe:alternate-stream",
        ] {
            assert_eq!(
                super::inspect_game_executable(&game(unsafe_path)),
                Some(super::PathCheck::InvalidPath),
                "must reject {unsafe_path}"
            );
        }
        assert!(super::is_safe_relative_executable("bin/game.exe"));
        std::fs::remove_dir_all(root).expect("remove installation root");
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
        // macOS is intentionally excluded from the 0.1 support matrix until
        // discovery can be validated on physical Mac hardware.
        #[cfg(target_os = "macos")]
        let expected: &[&str] = &[];
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
    fn detected_import_does_not_merge_different_platform_ids_at_the_same_path() {
        let saved = amethys_ipc::IpcGameEntry {
            id: "steam-1".to_owned(),
            name: "First game".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("100".to_owned()),
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: "/games/shared".to_owned(),
            install_locations: Vec::new(),
            executable: None,
        };
        let detected = amethys_discovery::DetectedGame {
            name: "Second game".to_owned(),
            platform: "Steam",
            platform_id: Some("200".to_owned()),
            install_path: std::path::PathBuf::from("/games/shared"),
        };

        assert!(!super::game_is_already_imported(&[saved], &detected));
    }

    #[test]
    fn detected_import_falls_back_to_path_when_platform_id_is_missing() {
        let saved = amethys_ipc::IpcGameEntry {
            id: "steam-1".to_owned(),
            name: "Example".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: "/games/example".to_owned(),
            install_locations: Vec::new(),
            executable: None,
        };
        let detected = amethys_discovery::DetectedGame {
            name: "Example".to_owned(),
            platform: "steam",
            platform_id: Some("100".to_owned()),
            install_path: std::path::PathBuf::from("/games/example"),
        };

        assert!(super::game_is_already_imported(&[saved], &detected));
    }

    #[test]
    fn filters_library_by_platform_case_insensitively() {
        let games = vec![
            amethys_ipc::IpcGameEntry {
                id: "steam-1".to_owned(),
                name: "Example".to_owned(),
                platform: "Steam".to_owned(),
                platform_id: Some("10".to_owned()),
                catalogue_game_id: None,
                catalogue_metadata: None,
                install_path: "A:/Steam/Example".to_owned(),
                install_locations: Vec::new(),
                executable: None,
            },
            amethys_ipc::IpcGameEntry {
                id: "epic-1".to_owned(),
                name: "Example".to_owned(),
                platform: "Epic Games".to_owned(),
                platform_id: Some("10".to_owned()),
                catalogue_game_id: None,
                catalogue_metadata: None,
                install_path: "A:/Epic/Example".to_owned(),
                install_locations: Vec::new(),
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
                catalogue_game_id: Some("amethys-1001".to_owned()),
                catalogue_metadata: Some(amethys_ipc::IpcGameMetadata {
                    genres: vec!["RPG".to_owned(), "Adventure".to_owned()],
                    developer: Some("Sandfall Interactive".to_owned()),
                    publisher: Some("Kepler Interactive".to_owned()),
                    release_date: Some("2025-04-24".to_owned()),
                    description: Some("A journey through a dark fantasy world.".to_owned()),
                }),
                install_path: "A:/SteamLibrary/steamapps/common/Expedition 33".to_owned(),
                install_locations: Vec::new(),
                executable: Some("Expedition33.exe".to_owned()),
            },
            amethys_ipc::IpcGameEntry {
                id: "epic-1".to_owned(),
                name: "Fortnite".to_owned(),
                platform: "Epic Games".to_owned(),
                platform_id: Some("Fortnite".to_owned()),
                catalogue_game_id: None,
                catalogue_metadata: None,
                install_path: "A:/Epic Games/Fortnite".to_owned(),
                install_locations: Vec::new(),
                executable: None,
            },
        ];

        let by_name = super::search_saved_games(&games, "CLAIR obscur");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].id, "steam-1");

        for query in [
            "rpg adventure",
            "sandfall interactive",
            "kepler interactive",
            "2025-04",
            "dark fantasy",
            "amethys-1001",
        ] {
            let results = super::search_saved_games(&games, query);
            assert_eq!(results.len(), 1, "query {query:?}");
            assert_eq!(results[0].id, "steam-1");
        }

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
        let list = vec!["games".to_owned(), "list".to_owned()];
        assert_eq!(parse_arguments(&list), Ok(IpcCommand::ListGames));

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
        let add_without_executable = vec![
            "games".to_owned(),
            "add".to_owned(),
            "Game".to_owned(),
            "Steam".to_owned(),
            "/games/game".to_owned(),
        ];
        assert_eq!(
            parse_arguments(&add_without_executable),
            Ok(IpcCommand::AddGame {
                name: "Game".to_owned(),
                platform: "Steam".to_owned(),
                install_path: "/games/game".to_owned(),
                executable: None,
            })
        );
        assert!(
            parse_arguments(&[
                "games".into(),
                "add".into(),
                "Game".into(),
                "Steam".into(),
                "/games/game".into(),
                "game.exe".into(),
                "unexpected".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_missing_or_unknown_commands() {
        assert!(parse_command(None).is_err());
        assert!(parse_command(Some("launch")).is_err());
        assert!(parse_arguments(&["games".into(), "launch".into()]).is_err());
        assert_eq!(
            parse_arguments(&[
                "games".into(),
                "set-executable".into(),
                "game-id".into(),
                "bin/game".into(),
            ]),
            Ok(IpcCommand::SetGameExecutable {
                id: "game-id".into(),
                executable: "bin/game".into(),
            })
        );
    }

    #[test]
    fn relative_executable_validation_rejects_cross_platform_traversal() {
        assert!(super::is_safe_relative_executable("bin/game.exe"));
        assert!(super::is_safe_relative_executable("game"));
        for invalid in [
            "",
            ".",
            "./game",
            "../outside",
            r"..\outside",
            r"C:\outside\game.exe",
            r"\\server\share\game.exe",
            "bin/../outside",
            "bin:alternate/game",
        ] {
            assert!(
                !super::is_safe_relative_executable(invalid),
                "unsafe executable path should be rejected: {invalid:?}"
            );
        }
    }

    #[test]
    fn launch_path_must_resolve_inside_the_registered_install() {
        let root = std::env::temp_dir().join(format!(
            "amethys-launch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create install");
        let executable = root.join(if cfg!(windows) { "game.exe" } else { "game" });
        std::fs::write(&executable, b"test executable").expect("create executable");
        let game = amethys_ipc::IpcGameEntry {
            id: "game-id".into(),
            name: "Example".into(),
            platform: "Test".into(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: Some(
                executable
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        assert_eq!(
            super::resolve_launch_executable(&game).expect("safe executable resolves"),
            executable.canonicalize().expect("canonical executable")
        );

        let mut traversal = game.clone();
        traversal.executable = Some("../outside".into());
        assert!(super::resolve_launch_executable(&traversal).is_err());

        let outside = root
            .parent()
            .expect("parent")
            .join(format!("amethys-outside-{}", std::process::id()));
        std::fs::write(&outside, b"outside").expect("create outside file");
        let mut absolute_outside = game.clone();
        absolute_outside.executable = Some(outside.to_string_lossy().into_owned());
        assert!(super::resolve_launch_executable(&absolute_outside).is_err());

        std::fs::remove_file(outside).expect("remove outside fixture");
        std::fs::remove_dir_all(root).expect("cleanup install");
    }

    #[cfg(unix)]
    #[test]
    fn launch_path_rejects_symlinks_that_escape_the_registered_install() {
        let root = std::env::temp_dir().join(format!(
            "amethys-launch-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create install");
        let outside = root
            .parent()
            .expect("temporary directory has a parent")
            .join(format!("amethys-launch-target-{}", std::process::id()));
        std::fs::write(&outside, b"outside").expect("create external target");
        let link = root.join("linked-game");
        std::os::unix::fs::symlink(&outside, &link).expect("create escaping symlink");

        let game = amethys_ipc::IpcGameEntry {
            id: "game-id".into(),
            name: "Example".into(),
            platform: "Test".into(),
            platform_id: None,
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: root.to_string_lossy().into_owned(),
            install_locations: Vec::new(),
            executable: Some("linked-game".into()),
        };

        assert!(
            super::resolve_launch_executable(&game).is_err(),
            "an executable symlink resolving outside the install must be rejected"
        );

        std::fs::remove_file(link).expect("remove symlink");
        std::fs::remove_file(outside).expect("remove external target");
        std::fs::remove_dir_all(root).expect("remove install");
    }

    #[test]
    fn explains_core_states_in_french() {
        assert_eq!(status_label(IpcCoreStatus::Running), "en fonctionnement");
        assert_eq!(status_label(IpcCoreStatus::Stopped), "arrêté");
    }
}
