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
            if install_path.is_dir()
                && !is_steam_runtime_component(&app_id, &name)
                && seen.insert(app_id.clone())
            {
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

fn is_steam_runtime_component(app_id: &str, name: &str) -> bool {
    matches!(app_id, "228980" | "1070560" | "1391110" | "1628350" | "1826330")
        || name.starts_with("Steamworks Common Redistributables")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Proton ")
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
        // Epic manifests may be written with a UTF-8 BOM.
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(contents) else {
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

/// Reads Heroic/Legendary installed-game metadata without starting the launcher.
///
/// Legendary stores one JSON object per installed Epic game in installed.json.
/// Only entries with a non-empty app name/title and an existing install directory
/// are returned; malformed entries and stale paths are ignored.
pub fn scan_heroic_legendary_installed(installed_json: &Path) -> Vec<DetectedGame> {
    let Ok(contents) = fs::read_to_string(installed_json) else { return Vec::new(); };
    let Ok(entries) = serde_json::from_str::<serde_json::Value>(&contents) else { return Vec::new(); };
    let Some(entries) = entries.as_object() else { return Vec::new(); };

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, value) in entries {
        let Some(record) = value.as_object() else { continue; };
        let app_name = record.get("app_name").and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty()).unwrap_or(key);
        let Some(title) = record.get("title").and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty()) else { continue; };
        let Some(install_location) = record.get("install_path").and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty()) else { continue; };
        let install_path = PathBuf::from(install_location);
        if !install_path.is_dir() { continue; }
        if !seen.insert(install_path.to_string_lossy().to_lowercase()) { continue; }
        games.push(DetectedGame {
            platform_id: Some(app_name.to_owned()),
            name: title.to_owned(),
            install_path,
            platform: "Heroic",
        });
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Returns known Heroic and standalone Legendary installed.json locations.
///
/// Heroic stores its configuration below Electron's appData/heroic directory;
/// standalone Legendary uses its own XDG-style config directory.
pub fn heroic_legendary_installed_files() -> Vec<PathBuf> {
    let mut files = BTreeSet::new();

    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        files.insert(PathBuf::from(app_data).join("heroic/legendaryConfig/legendary/installed.json"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
            files.insert(PathBuf::from(config_home).join("heroic/legendaryConfig/legendary/installed.json"));
            files.insert(PathBuf::from(config_home).join("legendary/installed.json"));
        } else if let Some(home) = std::env::var_os("HOME") {
            let config = PathBuf::from(home).join(".config");
            files.insert(config.join("heroic/legendaryConfig/legendary/installed.json"));
            files.insert(config.join("legendary/installed.json"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            files.insert(PathBuf::from(home).join(".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json"));
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        files.insert(PathBuf::from(home).join("Library/Application Support/heroic/legendaryConfig/legendary/installed.json"));
        files.insert(PathBuf::from(home).join(".config/legendary/installed.json"));
    }

    files.into_iter().filter(|path| path.is_file()).collect()
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
    #[allow(unused_mut)]
    let mut databases: BTreeSet<PathBuf> = BTreeSet::new();
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
    let mut search_from = 0;
    let open_start = loop {
        let relative = contents.get(search_from..)?.find(&open_tag)?;
        let candidate = search_from + relative;
        let after_name = candidate + open_tag.len();
        let next = *contents.as_bytes().get(after_name)?;
        if next == b'>' || next.is_ascii_whitespace() {
            break candidate;
        }
        search_from = after_name;
    };
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
        std::env::var_os("LOCALAPPDATA").map(|local_app_data| {
            PathBuf::from(local_app_data)
                .join("Amazon Games/Data/Games/Sql/GameInstallInfo.sqlite")
        })
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
        let install_key_prefixes = [
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\",
            r"HKEY_LOCAL_MACHINE\SOFTWARE\Ubisoft\Launcher\Installs\",
        ];
        if let Some(id) = install_key_prefixes.iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix))
        {
            current_id = Some(id.trim().to_owned());
            continue;
        }
        if let Some((_, value)) = trimmed.split_once("REG_SZ")
            .or_else(|| trimmed.split_once("REG_EXPAND_SZ"))
            .filter(|(name, _)| name.trim() == "InstallDir")
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
    const INSTALLS_KEYS: [&str; 2] = [
        r"HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs",
        r"HKLM\SOFTWARE\Ubisoft\Launcher\Installs",
    ];
    let mut games = Vec::new();
    for key in INSTALLS_KEYS {
        let output = match std::process::Command::new("reg")
            .args(["query", key, "/s"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        games.extend(parse_ubisoft_registry_output(&String::from_utf8_lossy(&output.stdout)));
    }
    let mut seen = BTreeSet::new();
    games.retain(|game| seen.insert(game.install_path.to_string_lossy().to_lowercase()));
    games.sort_by_key(|game| game.name.to_lowercase());
    games
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

/// Reads Battle.net Agent product metadata without starting the launcher or changing files.
///
/// The product database is a protobuf message. Only the documented length-delimited
/// fields needed for product code and install path are decoded; unknown fields are
/// skipped, malformed records are ignored, and every path must already exist.
pub fn scan_battlenet_product_db(database: &Path) -> Vec<DetectedGame> {
    const MAX_DATABASE_SIZE: u64 = 16 * 1024 * 1024;
    let Ok(metadata) = fs::metadata(database) else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_DATABASE_SIZE {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(database) else {
        return Vec::new();
    };

    let mut installs = protobuf_length_delimited_fields(&bytes, 1).unwrap_or_default();
    // Some Agent versions store a single ProductInstall message directly.
    if installs.is_empty() && protobuf_length_delimited_fields(&bytes, 2)
        .is_some_and(|fields| !fields.is_empty())
    {
        installs.push(bytes.as_slice());
    }

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for install in installs {
        let Some(code_bytes) = protobuf_length_delimited_fields(install, 2)
            .and_then(|fields| fields.into_iter().next())
        else {
            continue;
        };
        let Ok(product_code) = std::str::from_utf8(code_bytes) else {
            continue;
        };
        let product_code = product_code.trim();
        if product_code.is_empty() || matches!(product_code, "agent" | "bna") {
            continue;
        }
        let Some(settings) = protobuf_length_delimited_fields(install, 3)
            .and_then(|fields| fields.into_iter().next())
        else {
            continue;
        };
        let Some(path_bytes) = protobuf_length_delimited_fields(settings, 1)
            .and_then(|fields| fields.into_iter().next())
        else {
            continue;
        };
        let Some(cached_state) = protobuf_length_delimited_fields(install, 4)
            .and_then(|fields| fields.into_iter().next())
        else { continue; };
        let Some(base_state) = protobuf_length_delimited_fields(cached_state, 1)
            .and_then(|fields| fields.into_iter().next())
        else { continue; };
        if protobuf_varint_field(base_state, 1) != Some(1) { continue; }
        let Ok(path_text) = std::str::from_utf8(path_bytes) else {
            continue;
        };
        let path_text = path_text.trim_matches(char::from(0)).trim();
        if path_text.is_empty() {
            continue;
        }
        let install_path = PathBuf::from(path_text);
        if !install_path.is_dir() {
            continue;
        }
        let key = install_path.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let name = battlenet_product_name(product_code)
            .map(str::to_owned)
            .or_else(|| install_path.file_name().and_then(|name| name.to_str()).map(str::to_owned));
        let Some(name) = name.filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        games.push(DetectedGame {
            platform_id: Some(product_code.to_owned()),
            name,
            install_path,
            platform: "Battle.net",
        });
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

fn battlenet_product_name(code: &str) -> Option<&'static str> {
    match code {
        "wow" => Some("World of Warcraft"),
        "wow_classic" => Some("World of Warcraft Classic"),
        "fenris" => Some("Diablo IV"),
        "d2r" | "osi" => Some("Diablo II: Resurrected"),
        "d3" | "diablo3" => Some("Diablo III"),
        "anbs" => Some("Diablo Immortal"),
        "pro" | "prometheus" => Some("Overwatch 2"),
        "s2" => Some("StarCraft II"),
        "s1" => Some("StarCraft"),
        "w3" => Some("Warcraft III"),
        "hero" | "heroes" => Some("Heroes of the Storm"),
        "wtcg" | "hs_beta" => Some("Hearthstone"),
        "viper" => Some("Call of Duty: Black Ops 4"),
        "odin" => Some("Call of Duty: Modern Warfare"),
        "lazarus" => Some("Call of Duty: MW2 Campaign Remastered"),
        "zeus" => Some("Call of Duty: Black Ops Cold War"),
        "fore" => Some("Call of Duty: Vanguard"),
        "rtro" => Some("Blizzard Arcade Collection"),
        "wlby" => Some("Crash Bandicoot 4: It's About Time"),
        _ => None,
    }
}

/// Returns the standard Battle.net Agent product database path on Windows.
pub fn battlenet_product_database() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData").map(|program_data| {
            PathBuf::from(program_data).join("Battle.net/Agent/product.db")
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Extracts protobuf length-delimited values for one field number.
///
/// The decoder is intentionally bounded by the source slice and rejects malformed
/// tags, overflowing lengths, unsupported wire types, and truncated values.
fn protobuf_length_delimited_fields(bytes: &[u8], wanted: u32) -> Option<Vec<&[u8]>> {
    let mut offset = 0usize;
    let mut values = Vec::new();
    while offset < bytes.len() {
        let (tag, tag_len) = protobuf_varint(&bytes[offset..])?;
        offset = offset.checked_add(tag_len)?;
        let field = u32::try_from(tag >> 3).ok()?;
        let wire = (tag & 0x07) as u8;
        if field == 0 {
            return None;
        }
        match wire {
            0 => {
                let (_, len) = protobuf_varint(&bytes[offset..])?;
                offset = offset.checked_add(len)?;
            }
            1 => offset = offset.checked_add(8)?,
            2 => {
                let (length, len_len) = protobuf_varint(&bytes[offset..])?;
                offset = offset.checked_add(len_len)?;
                let length = usize::try_from(length).ok()?;
                let end = offset.checked_add(length)?;
                let value = bytes.get(offset..end)?;
                if field == wanted {
                    values.push(value);
                }
                offset = end;
            }
            5 => offset = offset.checked_add(4)?,
            _ => return None,
        }
        if offset > bytes.len() {
            return None;
        }
    }
    Some(values)
}

fn protobuf_varint_field(bytes: &[u8], wanted: u32) -> Option<u64> {
    let mut offset = 0usize;
    let mut found = None;
    while offset < bytes.len() {
        let (tag, tag_len) = protobuf_varint(&bytes[offset..])?;
        offset = offset.checked_add(tag_len)?;
        let field = u32::try_from(tag >> 3).ok()?;
        let wire = (tag & 7) as u8;
        if field == 0 { return None; }
        match wire {
            0 => { let (value, len) = protobuf_varint(&bytes[offset..])?; offset = offset.checked_add(len)?; if field == wanted { found = Some(value); } }
            1 => offset = offset.checked_add(8)?,
            2 => { let (length, len) = protobuf_varint(&bytes[offset..])?; offset = offset.checked_add(len)?; offset = offset.checked_add(usize::try_from(length).ok()?)?; }
            5 => offset = offset.checked_add(4)?,
            _ => return None,
        }
        if offset > bytes.len() { return None; }
    }
    found
}

fn protobuf_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f).checked_shl((index * 7) as u32)?;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

/// Collects read-only detections from supported launchers. Missing launchers
/// simply contribute no results and never prevent other adapters from running.
pub fn discover_all_games() -> Vec<DetectedGame> {
    let mut games = discover_steam_games();
    for root in epic_manifest_roots() {
        games.extend(scan_epic_manifests(&root));
    }
    for installed_json in heroic_legendary_installed_files() {
        games.extend(scan_heroic_legendary_installed(&installed_json));
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
    if let Some(database) = battlenet_product_database() {
        games.extend(scan_battlenet_product_db(&database));
    }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        games.extend(scan_game_folders("GOG", &[home.join("GOG Games")]));
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
    fn reads_epic_manifests_with_utf8_bom() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-bom-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let manifests = root.join("Manifests");
        let install = root.join("Installed Game");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&install).expect("create install");
        let json = format!(
            r#"{{"AppName":"BOMGame","DisplayName":"BOM Game","InstallLocation":{}}}"#,
            serde_json::to_string(&install.to_string_lossy()).expect("encode path")
        );
        std::fs::write(manifests.join("bom.item"), format!("\u{feff}{json}"))
            .expect("write BOM manifest");

        let games = scan_epic_manifests(&manifests);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "BOM Game");
        assert_eq!(games[0].platform_id.as_deref(), Some("BOMGame"));
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
    fn parses_ubisoft_registry_install_locations_from_native_64_bit_key() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-native-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("The Crew");
        std::fs::create_dir_all(&install).expect("create game install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\55\n    InstallDir    REG_SZ    {}\n",
            install.display()
        );
        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("55"));
        assert_eq!(games[0].name, "The Crew");
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

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        while value >= 0x80 {
            bytes.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        bytes.push(value as u8);
        bytes
    }

    fn encode_bytes_field(number: u32, value: &[u8]) -> Vec<u8> {
        let mut bytes = encode_varint((u64::from(number) << 3) | 2);
        bytes.extend(encode_varint(value.len() as u64));
        bytes.extend(value);
        bytes
    }

    #[test]
    fn detects_battlenet_products_from_protobuf_metadata() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("WoW");
        std::fs::create_dir_all(&install).expect("create installed game");
        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8).into_iter().chain(encode_varint(1)).collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"wow");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database_bytes = encode_bytes_field(1, &product);
        let database = root.join("product.db");
        std::fs::write(&database, database_bytes).expect("write protobuf fixture");

        let games = super::scan_battlenet_product_db(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "Battle.net");
        assert_eq!(games[0].platform_id.as_deref(), Some("wow"));
        assert_eq!(games[0].name, "World of Warcraft");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_battlenet_agent_records_missing_paths_and_malformed_data() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let settings = encode_bytes_field(1, root.join("missing").to_string_lossy().as_bytes());
        let mut agent = encode_bytes_field(2, b"agent");
        agent.extend(encode_bytes_field(3, &settings));
        let mut malformed = encode_bytes_field(1, &agent);
        malformed.extend([0x0a, 0xff]);
        let database = root.join("product.db");
        std::fs::write(&database, malformed).expect("write malformed fixture");
        assert!(super::scan_battlenet_product_db(&database).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_battlenet_products_not_marked_as_installed() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-not-installed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("World of Warcraft");
        std::fs::create_dir_all(&install).expect("create leftover game directory");
        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8).into_iter().chain(encode_varint(0)).collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"wow");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database = root.join("product.db");
        std::fs::write(&database, encode_bytes_field(1, &product)).expect("write fixture");

        assert!(super::scan_battlenet_product_db(&database).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_oversized_battlenet_database() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-large-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let database = root.join("product.db");
        let file = std::fs::File::create(&database).expect("create database");
        file.set_len(16 * 1024 * 1024 + 1).expect("extend database");
        assert!(super::scan_battlenet_product_db(&database).is_empty());
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
    fn reads_heroic_legendary_installed_metadata_and_skips_stale_entries() {
        let root = std::env::temp_dir().join(format!(
            "amethys-heroic-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let installed = root.join("Hades");
        std::fs::create_dir_all(&installed).expect("create installed game");
        let database = root.join("installed.json");
        let contents = serde_json::json!({
            "HadesApp": { "app_name": "HadesApp", "title": "Hades", "install_path": installed.to_string_lossy() },
            "RemovedApp": { "app_name": "RemovedApp", "title": "Removed Game", "install_path": root.join("Removed Game").to_string_lossy() },
            "BrokenApp": { "app_name": "BrokenApp", "title": "Broken Game" }
        });
        std::fs::write(&database, contents.to_string()).expect("write Legendary metadata");

        let games = super::scan_heroic_legendary_installed(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "Heroic");
        assert_eq!(games[0].platform_id.as_deref(), Some("HadesApp"));
        assert_eq!(games[0].name, "Hades");
        assert_eq!(games[0].install_path, installed);
        assert!(super::scan_heroic_legendary_installed(&root.join("missing.json")).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn filters_steam_runtime_components_from_game_results() {
        assert!(super::is_steam_runtime_component("228980", "Steamworks Common Redistributables"));
        assert!(super::is_steam_runtime_component("1070560", "Steam Linux Runtime"));
        assert!(super::is_steam_runtime_component("123", "Proton Experimental"));
        assert!(!super::is_steam_runtime_component("570", "Dota 2"));
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
