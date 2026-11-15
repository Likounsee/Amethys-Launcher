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
    #[allow(unused_mut)]
    let mut roots: BTreeSet<PathBuf> = BTreeSet::new();
    #[cfg(windows)]
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.insert(PathBuf::from(program_data).join("Epic/EpicGamesLauncher/Data/Manifests"));
    }
    // Epic's .item launcher manifests are currently supported on Windows.
    // Heroic/Legendary use different metadata formats and are scanned separately.
    roots.into_iter().filter(|path| path.is_dir()).collect()
}

/// Reads locally installed GOG Galaxy games from its read-only SQLite database.
pub fn scan_gog_galaxy(database: &Path) -> Vec<DetectedGame> {
    use rusqlite::{Connection, OpenFlags};

    let connection = match Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => connection,
        Err(_) => return Vec::new(),
    };
    let mut statement = match connection.prepare(
        "SELECT ibp.productId, ibp.installationPath, gp.value
         FROM InstalledBaseProducts AS ibp
         JOIN GamePieces AS gp ON gp.releaseKey = 'gog_' || ibp.productId
         JOIN GamePieceTypes AS gpt ON gp.gamePieceTypeId = gpt.id
         WHERE gpt.type = 'originalTitle'"
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows.flatten() {
        let (platform_id, install_location, title_json) = row;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&title_json) else {
            continue;
        };
        let Some(name) = value.get("title").and_then(|title| title.as_str())
            .filter(|title| !title.trim().is_empty())
        else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        if install_path.is_dir() && seen.insert(platform_id.clone()) {
            games.push(DetectedGame {
                platform_id: Some(platform_id),
                name: name.to_owned(),
                install_path,
                platform: "GOG",
            });
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Returns known GOG Galaxy database locations for the current operating system.
pub fn gog_galaxy_databases() -> Vec<PathBuf> {
    let mut databases = BTreeSet::new();
    #[cfg(windows)]
    if let Some(program_data) = std::env::var_os("ProgramData") {
        databases.insert(PathBuf::from(program_data).join("GOG.com/Galaxy/storage/galaxy-2.0.db"));
    }
    #[cfg(target_os = "macos")]
    databases.insert(PathBuf::from("/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db"));
    databases.into_iter().filter(|path| path.is_file()).collect()
}

/// Reads installed Amazon Games entries from its local SQLite database.
///
/// The database is opened read-only. Rows without a valid title, product ID,
/// installed directory, or existing directory are ignored.
pub fn scan_amazon_games(database: &Path) -> Vec<DetectedGame> {
    use rusqlite::{Connection, OpenFlags};

    let connection = match Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => connection,
        Err(_) => return Vec::new(),
    };
    let mut statement = match connection.prepare(
        "SELECT Id, ProductTitle, InstallDirectory FROM DbSet"
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows.flatten() {
        let (platform_id, name, install_location) = row;
        if platform_id.trim().is_empty() || name.trim().is_empty() || install_location.trim().is_empty() {
            continue;
        }
        let install_path = PathBuf::from(install_location);
        if install_path.is_dir() && seen.insert(platform_id.clone()) {
            games.push(DetectedGame {
                platform_id: Some(platform_id),
                name,
                install_path,
                platform: "Amazon Games",
            });
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Reads EA app installations using the installer metadata shipped with each game.
pub fn scan_ea_games(roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();

    for root in roots {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let install_path = entry.path();
            if !install_path.is_dir() {
                continue;
            }
            let metadata = install_path.join("__Installer/installerdata.xml");
            let Ok(contents) = fs::read_to_string(&metadata) else {
                continue;
            };
            let name = xml_element_text(&contents, "gameTitle")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    install_path.file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_owned()
                });
            if name.trim().is_empty() {
                continue;
            }
            let platform_id = xml_element_text(&contents, "contentID")
                .filter(|value| !value.trim().is_empty());
            let key = install_path.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                games.push(DetectedGame {
                    platform_id,
                    name,
                    install_path,
                    platform: "EA app",
                });
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

fn xml_element_text(contents: &str, element: &str) -> Option<String> {
    let open_tag = format!("<{element}");
    let open_start = contents.find(&open_tag)?;
    let open_end = contents[open_start..].find('>')? + open_start + 1;
    let close_tag = format!("</{element}>");
    let close_start = contents[open_end..].find(&close_tag)? + open_end;
    let raw = contents[open_end..close_start].trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'"))
}

/// Returns the standard Amazon Games install metadata database on Windows.
pub fn amazon_games_database() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return std::env::var_os("LOCALAPPDATA").map(|local_app_data| {
            PathBuf::from(local_app_data)
                .join("Amazon Games/Data/Games/Sql/GameInstallInfo.sqlite")
        });
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Parses Ubisoft Connect's registry output into installed game records.
pub fn parse_ubisoft_registry_output(contents: &str) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut current_id: Option<String> = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(id) = trimmed.strip_prefix(
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\"
        ) {
            current_id = Some(id.trim().to_owned());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("InstallDir    REG_SZ    ")
            .or_else(|| trimmed.strip_prefix("InstallDir    REG_EXPAND_SZ    "))
        {
            let install_path = PathBuf::from(value.trim());
            if !install_path.is_dir() {
                continue;
            }
            let Some(name) = install_path.file_name().and_then(|value| value.to_str())
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            if let Some(platform_id) = current_id.as_ref().filter(|id| !id.is_empty()) {
                games.push(DetectedGame {
                    platform_id: Some(platform_id.clone()),
                    name: name.to_owned(),
                    install_path,
                    platform: "Ubisoft Connect",
                });
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Reads Ubisoft Connect's machine-wide install registry entries on Windows.
#[cfg(windows)]
pub fn discover_ubisoft_games() -> Vec<DetectedGame> {
    const INSTALLS_KEY: &str =
        r"HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    let output = match std::process::Command::new("reg")
        .args(["query", INSTALLS_KEY, "/s"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    parse_ubisoft_registry_output(&String::from_utf8_lossy(&output.stdout))
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
    for database in gog_galaxy_databases() {
        games.extend(scan_gog_galaxy(&database));
    }
    if let Some(database) = amazon_games_database() {
        games.extend(scan_amazon_games(&database));
    }

    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
        if let Some(root) = program_files.as_ref() {
            games.extend(scan_ea_games(&[root.join("EA Games")]));
            games.extend(scan_game_folders("GOG", &[root.join("GOG Games")]));
            games.extend(scan_game_folders("Xbox", &[root.join("XboxGames")]));
        }
        if let Some(system_drive) = std::env::var_os("SystemDrive") {
            games.extend(scan_game_folders("GOG", &[PathBuf::from(system_drive).join("GOG Games")]));
        }
        if let Some(root) = program_files_x86.as_ref() {
            games.extend(scan_game_folders("Ubisoft Connect", &[root.join("Ubisoft/Ubisoft Game Launcher/games")]));
        }
        games.extend(discover_ubisoft_games());
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
    fn reads_installed_gog_games_from_galaxy_database() {
        use rusqlite::Connection;

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let install = root.join("Heroes 3");
        std::fs::create_dir_all(&install).expect("create installed game");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection.execute_batch(
            "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);"
        ).expect("create GOG metadata tables");
        connection.execute("INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
            rusqlite::params!["1234567890", install.to_string_lossy()])
            .expect("insert installed GOG product");
        connection.execute("INSERT INTO GamePieceTypes VALUES (1, 'originalTitle')", [])
            .expect("insert title type");
        connection.execute("INSERT INTO GamePieces VALUES (?1, 1, ?2)",
            rusqlite::params!["gog_1234567890", r#"{"title":"Heroes of Might and Magic 3"}"#])
            .expect("insert GOG title");
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "GOG");
        assert_eq!(games[0].platform_id.as_deref(), Some("1234567890"));
        assert_eq!(games[0].name, "Heroes of Might and Magic 3");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_unknown_or_missing_gog_galaxy_databases() {
        use rusqlite::Connection;

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let database = root.join("unknown.db");
        Connection::open(&database).expect("create empty database")
            .execute_batch("CREATE TABLE unrelated (value TEXT);")
            .expect("create unrelated table");
        assert!(super::scan_gog_galaxy(&database).is_empty());
        assert!(super::scan_gog_galaxy(&root.join("missing.db")).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn detects_ea_games_from_installer_metadata_and_reads_title_and_content_id() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("EA Games").join("Dead Space");
        let metadata_dir = install.join("__Installer");
        std::fs::create_dir_all(&metadata_dir).expect("create EA metadata folder");
        std::fs::write(
            metadata_dir.join("installerdata.xml"),
            r#"<?xml version="1.0"?><DiPManifest><gameTitle locale="en_US">Dead Space</gameTitle><contentIDs><contentID>DR:12345</contentID></contentIDs></DiPManifest>"#,
        ).expect("write EA installer metadata");

        let games = super::scan_ea_games(&[root.join("EA Games")]);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "EA app");
        assert_eq!(games[0].platform_id.as_deref(), Some("DR:12345"));
        assert_eq!(games[0].name, "Dead Space");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_ea_folders_without_installer_metadata_and_falls_back_to_folder_name() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Mass Effect");
        std::fs::create_dir_all(install.join("__Installer")).expect("create metadata folder");
        std::fs::write(install.join("__Installer/installerdata.xml"), "<DiPManifest/>")
            .expect("write incomplete metadata");
        std::fs::create_dir_all(root.join("Unrelated Folder")).expect("create unrelated folder");

        let games = super::scan_ea_games(std::slice::from_ref(&root));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Mass Effect");
        assert_eq!(games[0].platform_id, None);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn reads_only_installed_amazon_games_from_read_only_database() {
        use rusqlite::Connection;

        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let installed = root.join("Blue Fire");
        let not_installed = root.join("Not Installed");
        std::fs::create_dir_all(&installed).expect("create installed game");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create fixture database");
        connection.execute_batch(
            "CREATE TABLE DbSet (
                Id TEXT,
                InstallDirectory TEXT,
                ProductTitle TEXT
            );"
        ).expect("create Amazon metadata table");
        connection.execute(
            "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["amzn1.blue-fire", installed.to_string_lossy(), "Blue Fire"],
        ).expect("insert installed game");
        connection.execute(
            "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["amzn1.other", not_installed.to_string_lossy(), "Not Installed"],
        ).expect("insert uninstalled game");
        drop(connection);

        let games = super::scan_amazon_games(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "Amazon Games");
        assert_eq!(games[0].platform_id.as_deref(), Some("amzn1.blue-fire"));
        assert_eq!(games[0].name, "Blue Fire");
        assert_eq!(games[0].install_path, installed);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_unknown_or_missing_amazon_databases() {
        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let unknown = root.join("unknown.sqlite");
        rusqlite::Connection::open(&unknown)
            .expect("create empty database")
            .execute_batch("CREATE TABLE unrelated (value TEXT);")
            .expect("create unrelated table");
        assert!(super::scan_amazon_games(&unknown).is_empty());
        assert!(super::scan_amazon_games(&root.join("missing.sqlite")).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_ubisoft_registry_install_locations() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Assassins Creed");
        std::fs::create_dir_all(&install).expect("create game install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\n",
            install.display()
        );
        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "Ubisoft Connect");
        assert_eq!(games[0].platform_id.as_deref(), Some("44"));
        assert_eq!(games[0].name, "Assassins Creed");
        assert_eq!(games[0].install_path, install);
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
