use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// A game found in Steam's local installation metadata.
/// Discovery is read-only: it never changes platform files or the game library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGame {
    pub platform_id: Option<String>,
    pub name: String,
    pub install_path: PathBuf,
    pub platform: &'static str,
}

/// Reads the "path" entries from Steam's libraryfolders.vdf file.
pub fn parse_library_folders(contents: &str, steam_root: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    paths.insert(steam_root.to_path_buf());

    for line in contents.lines() {
        let Some((key, value)) = quoted_pair(line) else {
            continue;
        };
        // Newer Steam files use a nested path field; older files use numeric keys.
        if (key == "path" || key.parse::<u32>().is_ok()) && !value.is_empty() {
            paths.insert(PathBuf::from(unescape_vdf(&value)));
        }
    }

    paths.into_iter().collect()
}

/// Extracts the small set of fields needed from an appmanifest_*.acf file.
pub fn parse_app_manifest(contents: &str) -> Option<(String, String, String)> {
    let app_id = find_vdf_value(contents, "appid")?;
    let name = find_vdf_value(contents, "name")?;
    let install_dir = find_vdf_value(contents, "installdir")?;

    if app_id.is_empty() || name.trim().is_empty() || install_dir.trim().is_empty() {
        return None;
    }

    if install_dir.contains('/') || install_dir.contains('\\')
        || Path::new(&install_dir).components().any(|component| {
            !matches!(component, std::path::Component::Normal(_))
        })
    {
        return None;
    }

    Some((app_id, name, install_dir))
}

/// Scans known Steam library folders without starting Steam or modifying any files.
pub fn scan_steam_libraries(libraries: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();

    for library in libraries {
        let manifests = library.join("steamapps");
        let entries = match fs::read_dir(&manifests) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }

            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let Some((app_id, name, install_dir)) = parse_app_manifest(&contents) else {
                continue;
            };
            let install_path = manifests.join("common").join(install_dir);
            if install_path.is_dir() && seen.insert(app_id.clone()) {
                games.push(DetectedGame {
                    platform_id: Some(app_id),
                    name,
                    install_path,
                    platform: "Steam",
                });
            }
        }
    }

    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Finds common Steam installation roots for the current operating system.
pub fn steam_install_roots() -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();

    #[cfg(windows)]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
            roots.insert(PathBuf::from(program_files).join("Steam"));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            roots.insert(PathBuf::from(program_files).join("Steam"));
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        roots.insert(PathBuf::from(home).join("Library/Application Support/Steam"));
    }

    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.insert(home.join(".steam/steam"));
        roots.insert(home.join(".local/share/Steam"));
    }

    roots.into_iter().filter(|path| path.is_dir()).collect()
}

/// Reads libraryfolders.vdf files and returns their declared library roots.
pub fn discover_steam_libraries() -> Vec<PathBuf> {
    let mut libraries = BTreeSet::new();

    for root in steam_install_roots() {
        libraries.insert(root.clone());
        let metadata = root.join("steamapps/libraryfolders.vdf");
        let Ok(contents) = fs::read_to_string(metadata) else {
            continue;
        };
        libraries.extend(parse_library_folders(&contents, &root));
    }

    libraries.into_iter().collect()
}

/// Performs a read-only scan of Steam games installed in the detected libraries.
pub fn discover_steam_games() -> Vec<DetectedGame> {
    scan_steam_libraries(&discover_steam_libraries())
}


/// Reads Epic Games Launcher manifests. Invalid or incomplete manifests are ignored.
pub fn scan_epic_manifests(manifests_dir: &Path) -> Vec<DetectedGame> {
    let entries = match fs::read_dir(manifests_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("item") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&contents) else {
            continue;
        };
        let Some(install_location) = manifest.get("InstallLocation").and_then(|v| v.as_str()) else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        if !install_path.is_dir() {
            continue;
        }
        let Some(name) = manifest.get("DisplayName").and_then(|v| v.as_str())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| install_path.file_name().and_then(|value| value.to_str()))
        else {
            continue;
        };
        let platform_id = manifest.get("AppName").and_then(|v| v.as_str()).map(str::to_owned);
        let dedup_key = install_path.to_string_lossy().to_lowercase();
        if seen.insert(dedup_key) {
            games.push(DetectedGame {
                platform_id,
                name: name.to_owned(),
                install_path,
                platform: "Epic Games",
            });
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Returns the usual Epic Games manifest folder for the current operating system.
pub fn epic_manifest_roots() -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    #[cfg(windows)]
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.insert(PathBuf::from(program_data).join("Epic/EpicGamesLauncher/Data/Manifests"));
    }
    // Epic's .item launcher manifests are currently supported on Windows.
    // Heroic/Legendary use different metadata formats and are scanned separately.
    roots.into_iter().filter(|path| path.is_dir()).collect()
}

/// Finds game folders inside known libraries for launchers without a stable,
/// documented local manifest format. Only immediate child folders are considered.
pub fn scan_game_folders(platform: &'static str, roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str())
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            let key = path.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                games.push(DetectedGame {
                    platform_id: None,
                    name: name.to_owned(),
                    install_path: path,
                    platform,
                });
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Collects read-only detections from supported launchers. Missing launchers
/// simply contribute no results and never prevent other adapters from running.
pub fn discover_all_games() -> Vec<DetectedGame> {
    let mut games = discover_steam_games();
    for root in epic_manifest_roots() {
        games.extend(scan_epic_manifests(&root));
    }

    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
        if let Some(root) = program_files.as_ref() {
            games.extend(scan_game_folders("EA app", &[root.join("EA Games")]));
            games.extend(scan_game_folders("GOG", &[root.join("GOG Games")]));
            games.extend(scan_game_folders("Xbox", &[root.join("XboxGames")]));
        }
        if let Some(system_drive) = std::env::var_os("SystemDrive") {
            games.extend(scan_game_folders("GOG", &[PathBuf::from(system_drive).join("GOG Games")]));
        }
        if let Some(root) = program_files_x86.as_ref() {
            games.extend(scan_game_folders("Ubisoft Connect", &[root.join("Ubisoft/Ubisoft Game Launcher/games")]));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        games.extend(scan_game_folders("GOG", &[home.join("GOG Games")]));
        games.extend(scan_game_folders("Heroic", &[home.join("Games/Heroic")]));
    }

    games.sort_by_key(|game| (game.platform, game.name.to_lowercase()));
    games.dedup_by(|left, right| {
        left.platform == right.platform
            && left.install_path.to_string_lossy().eq_ignore_ascii_case(&right.install_path.to_string_lossy())
    });
    games
}

fn find_vdf_value(contents: &str, wanted_key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (key, value) = quoted_pair(line)?;
        (key == wanted_key).then(|| unescape_vdf(&value))
    })
}

fn quoted_pair(line: &str) -> Option<(String, String)> {
    let mut quoted = line.match_indices('"');
    let (key_start, _) = quoted.next()?;
    let (key_end, _) = quoted.next()?;
    let (value_start, _) = quoted.next()?;
    let (value_end, _) = quoted.next()?;

    let key = line.get(key_start + 1..key_end)?;
    let value = line.get(value_start + 1..value_end)?;
    Some((key.to_owned(), value.to_owned()))
}

fn unescape_vdf(value: &str) -> String {
    value.replace("\\\\", "\\").replace("\\\"", "\"")
}

#[cfg(test)]
mod tests {
    use super::{
        parse_app_manifest, parse_library_folders, scan_epic_manifests, scan_game_folders,
        scan_steam_libraries,
    };
    use std::path::Path;

    #[test]
    fn reads_steam_library_paths_and_keeps_default_library() {
        let contents = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path" "C:\\Program Files (x86)\\Steam"
                }
                "1"
                {
                    "path" "D:\\SteamLibrary"
                }
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("C:/Steam"));
        assert!(paths.contains(&Path::new("C:/Steam").to_path_buf()));
        assert!(paths.contains(&Path::new(r"D:\SteamLibrary").to_path_buf()));
    }

    #[test]
    fn supports_older_flat_steam_library_entries() {
        let contents = r#"
            "libraryfolders"
            {
                "0" "C:\\Steam"
                "1" "D:\\SteamLibrary"
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("C:/DefaultSteam"));
        assert!(paths.contains(&Path::new(r"C:\Steam").to_path_buf()));
        assert!(paths.contains(&Path::new(r"D:\SteamLibrary").to_path_buf()));
    }

    #[test]
    fn rejects_install_directories_that_escape_the_steam_game_folder() {
        for install_dir in ["../outside", r"..\\outside", "folder/game", r"folder\\game"] {
            let contents = format!(
                r#"
                    "appid" "10"
                    "name" "Unsafe"
                    "installdir" "{install_dir}"
                "#
            );
            assert_eq!(parse_app_manifest(&contents), None, "accepted {install_dir}");
        }
    }

    #[test]
    fn scans_only_manifests_with_existing_game_directories() {
        let root = std::env::temp_dir().join(format!(
            "amethys-steam-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is after Unix epoch")
                .as_nanos()
        ));
        let steamapps = root.join("steamapps");
        let game_dir = steamapps.join("common").join("Dota 2");
        std::fs::create_dir_all(&game_dir).expect("create installed game directory");
        std::fs::write(
            steamapps.join("appmanifest_570.acf"),
            r#"
                "appid" "570"
                "name" "Dota 2"
                "installdir" "Dota 2"
            "#,
        )
        .expect("write installed game manifest");
        std::fs::write(
            steamapps.join("appmanifest_10.acf"),
            r#"
                "appid" "10"
                "name" "Missing Game"
                "installdir" "Missing Game"
            "#,
        )
        .expect("write missing game manifest");

        let games = scan_steam_libraries(std::slice::from_ref(&root));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("570"));
        assert_eq!(games[0].name, "Dota 2");
        assert_eq!(games[0].install_path, game_dir);

        std::fs::remove_dir_all(root).expect("remove temporary Steam library");
    }

    #[test]
    fn reads_epic_games_from_item_manifests_and_ignores_missing_installs() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let manifests = root.join("Manifests");
        let installed = root.join("Installed").join("Example Game");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&installed).expect("create game folder");
        std::fs::write(manifests.join("installed.item"), format!(
            r#"{{"AppName":"ExampleApp","DisplayName":"Example Game","InstallLocation":{}}}"#,
            serde_json::to_string(&installed.to_string_lossy()).expect("encode path")
        )).expect("write manifest");
        std::fs::write(manifests.join("missing.item"),
            r#"{"AppName":"Missing","DisplayName":"Missing Game","InstallLocation":"not-a-real-path"}"#
        ).expect("write stale manifest");
        std::fs::write(manifests.join("broken.item"), "{broken").expect("write invalid manifest");

        let games = scan_epic_manifests(&manifests);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "Epic Games");
        assert_eq!(games[0].platform_id.as_deref(), Some("ExampleApp"));
        assert_eq!(games[0].name, "Example Game");
        assert_eq!(games[0].install_path, installed);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn scans_only_immediate_children_of_a_known_launcher_library() {
        let root = std::env::temp_dir().join(format!(
            "amethys-folder-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is after Unix epoch")
                .as_nanos()
        ));
        let installed = root.join("Installed Game");
        let nested = installed.join("Game Data");
        std::fs::create_dir_all(&nested).expect("create nested game folder");
        std::fs::create_dir_all(root.join("Another Installed Game")).expect("create cache folder");

        let games = scan_game_folders("EA app", std::slice::from_ref(&root));
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| game.name == "Installed Game"));
        assert!(games.iter().any(|game| game.name == "Another Installed Game"));
        assert!(games.iter().all(|game| game.platform == "EA app"));
        assert!(games.iter().all(|game| game.platform_id.is_none()));
        assert!(!games.iter().any(|game| game.name == "Game Data"));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn reads_required_fields_from_app_manifest() {
        let contents = r#"
            "AppState"
            {
                "appid" "570"
                "name" "Dota 2"
                "installdir" "Dota 2"
            }
        "#;
        assert_eq!(
            parse_app_manifest(contents),
            Some(("570".to_owned(), "Dota 2".to_owned(), "Dota 2".to_owned()))
        );
    }

    #[test]
    fn ignores_incomplete_or_empty_manifests() {
        assert_eq!(parse_app_manifest(r#""appid" "10""#), None);
        assert_eq!(
            parse_app_manifest(r#""appid" "10" "name" "" "installdir" "Game""#),
            None
        );
    }
}
