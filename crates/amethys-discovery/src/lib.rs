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
    let mut depth = 0usize;

    for line in contents.lines() {
        if let Some((key, value)) = quoted_pair(line) {
            // Legacy entries are numeric key/value pairs directly inside the
            // libraryfolders object. Modern entries store their path one level
            // deeper; numeric keys inside their nested "apps" map are app IDs,
            // not library paths.
            let is_legacy_library = depth == 1 && key.parse::<u32>().is_ok();
            let is_modern_library_path = depth == 2 && key == "path";
            if (is_legacy_library || is_modern_library_path) && !value.is_empty() {
                paths.insert(PathBuf::from(unescape_vdf(&value)));
            }
        }

        depth = vdf_update_depth(depth, line);
    }

    paths.into_iter().collect()
}

/// Counts VDF braces outside quoted strings so braces in paths do not affect nesting.
fn vdf_update_depth(depth: usize, line: &str) -> usize {
    let mut depth = depth;
    let mut in_quotes = false;
    let mut escaped = false;

    for character in line.chars() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_quotes = false;
            }
        } else {
            match character {
                '"' => in_quotes = true,
                '{' => depth = depth.saturating_add(1),
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    depth
}

/// Extracts the small set of fields needed from an appmanifest_*.acf file.
pub fn parse_app_manifest(contents: &str) -> Option<(String, String, String)> {
    // Launcher metadata can contain accidental padding around values. Trim it
    // before validating IDs or constructing paths so valid installs are not
    // silently missed because of harmless whitespace.
    let app_id = find_vdf_value(contents, "appid")?.trim().to_owned();
    let name = find_vdf_value(contents, "name")?.trim().to_owned();
    let install_dir = find_vdf_value(contents, "installdir")?.trim().to_owned();

    if app_id.is_empty() || name.is_empty() || install_dir.is_empty() {
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
            // Steam's manifest filename and embedded AppID must agree. A stale,
            // copied or malformed manifest must not attribute another game's
            // installation directory to the wrong platform identifier.
            let filename_app_id = filename
                .strip_prefix("appmanifest_")
                .and_then(|value| value.strip_suffix(".acf"));
            // Steam AppIDs are decimal identifiers. Reject arbitrary filenames
            // that merely happen to repeat a non-numeric value in the payload.
            if filename_app_id.and_then(|value| value.parse::<u32>().ok()).is_none()
                || filename_app_id != Some(app_id.as_str())
            {
                continue;
            }
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
    // Filesystem directory iteration order is unspecified. Sort manifests so
    // duplicate metadata always produces the same result across scans.
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut seen_app_ids = BTreeSet::new();

    for entry in entries {
        let path = entry.path();
        if !path.extension().and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
        {
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
        let Some(install_location) = manifest.get("InstallLocation").and_then(|v| v.as_str())
            .map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        if !install_path.is_dir() {
            continue;
        }
        let Some(name) = manifest.get("DisplayName").and_then(|v| v.as_str())
            .map(str::trim).filter(|value| !value.is_empty())
            .or_else(|| install_path.file_name().and_then(|value| value.to_str()))
        else {
            continue;
        };
        let platform_id = manifest.get("AppName").and_then(|v| v.as_str())
            .map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
        if platform_id.as_ref().is_some_and(|id| seen_app_ids.contains(id)) {
            continue;
        }
        if insert_unique_install_path(&mut seen_paths, &install_path) {
            if let Some(id) = platform_id.as_ref() {
                seen_app_ids.insert(id.clone());
            }
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
    let mut seen_paths = BTreeSet::new();
    let mut seen_app_ids = BTreeSet::new();
    // serde_json map ordering depends on enabled features; sort explicitly so
    // duplicate app IDs always resolve to the same record on every build.
    let mut entries: Vec<_> = entries.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (key, value) in entries {
        let Some(record) = value.as_object() else { continue; };
        let app_name = record.get("app_name").and_then(|value| value.as_str())
            .map(str::trim).filter(|value| !value.is_empty()).unwrap_or(key.trim());
        if app_name.is_empty() || seen_app_ids.contains(app_name) {
            continue;
        }
        let Some(title) = record.get("title").and_then(|value| value.as_str())
            .map(str::trim).filter(|value| !value.is_empty()) else { continue; };
        let Some(install_location) = record.get("install_path").and_then(|value| value.as_str())
            .map(str::trim).filter(|value| !value.is_empty()) else { continue; };
        let install_path = PathBuf::from(install_location);
        if !install_path.is_dir() { continue; }
        if !insert_unique_install_path(&mut seen_paths, &install_path) { continue; }
        seen_app_ids.insert(app_name.to_owned());
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
            let config_home = PathBuf::from(config_home);
            files.insert(config_home.join("heroic/legendaryConfig/legendary/installed.json"));
            files.insert(config_home.join("legendary/installed.json"));
        } else if let Some(home) = std::env::var_os("HOME") {
            let config = PathBuf::from(home).join(".config");
            files.insert(config.join("heroic/legendaryConfig/legendary/installed.json"));
            files.insert(config.join("legendary/installed.json"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            files.insert(home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json"));
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        files.insert(home.join("Library/Application Support/heroic/legendaryConfig/legendary/installed.json"));
        files.insert(home.join(".config/legendary/installed.json"));
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
    let mut seen_products = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    for row in rows.flatten() {
        let (platform_id, install_location, title_json) = row;
        let platform_id = platform_id.trim();
        let install_location = install_location.trim();
        if platform_id.is_empty() || install_location.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&title_json) else {
            continue;
        };
        let Some(name) = value
            .get("title")
            .and_then(|title| title.as_str())
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        let Ok(path_key) = fs::canonicalize(&install_path) else {
            continue;
        };
        let product_key = platform_id.to_owned();
        if install_path.is_dir()
            && !seen_products.contains(&product_key)
            && !seen_paths.contains(&path_key)
        {
            // Commit both deduplication keys only after every acceptance
            // condition passes; a path collision must not poison a product ID.
            seen_products.insert(product_key);
            seen_paths.insert(path_key);
            games.push(DetectedGame {
                platform_id: Some(platform_id.to_owned()),
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
    let mut seen_products = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    for row in rows.flatten() {
        let (platform_id, name, install_location) = row;
        let platform_id = platform_id.trim();
        let name = name.trim();
        let install_location = install_location.trim();
        if platform_id.is_empty() || name.is_empty() || install_location.is_empty() {
            continue;
        }
        let install_path = PathBuf::from(install_location);
        let product_key = platform_id.to_owned();
        let path_key = install_path_key(&install_path);
        if install_path.is_dir()
            && !seen_products.contains(&product_key)
            && !seen_paths.contains(&path_key)
        {
            // Do not reserve an ID when its path is rejected as a duplicate.
            seen_products.insert(product_key);
            seen_paths.insert(path_key);
            games.push(DetectedGame {
                platform_id: Some(platform_id.to_owned()),
                name: name.to_owned(),
                install_path,
                platform: "Amazon Games",
            });
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Expands Windows-style %NAME% references without invoking a shell.
/// Unknown variables are preserved so an unresolved path cannot become a false hit.
fn expand_windows_environment_variables_with<F>(value: &str, mut lookup: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let mut expanded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        expanded.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            expanded.push_str(&remaining[start..]);
            return expanded;
        };
        let variable = &after_start[..end];
        if variable.is_empty() {
            expanded.push_str("%%");
        } else if let Some(replacement) = lookup(variable) {
            expanded.push_str(&replacement);
        } else {
            expanded.push('%');
            expanded.push_str(variable);
            expanded.push('%');
        }
        remaining = &after_start[end + 1..];
    }
    expanded.push_str(remaining);
    expanded
}

fn expand_windows_environment_variables(value: &str) -> String {
    expand_windows_environment_variables_with(value, |name| std::env::var(name).ok())
}

/// Parses EA app registry installation records and validates each game's installer metadata.
///
/// EA app stores per-game install locations under HKLM\\SOFTWARE\\EA Games.
/// A registry path alone is not enough: the install directory must exist and
/// contain EA's installerdata.xml before it is reported.
pub fn parse_ea_registry_output(contents: &str) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut current_id: Option<String> = None;
    let mut seen_ids = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    let prefixes = [
        r"HKEY_LOCAL_MACHINE\SOFTWARE\EA Games\",
        r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\EA Games\",
    ];

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("HKEY_") {
            // Recursive registry output can contain unrelated vendors between
            // EA keys. Clear the current ID so their Install Dir values cannot
            // inherit the previous EA product identifier.
            current_id = prefixes.iter()
                .find_map(|prefix| trimmed.strip_prefix(prefix))
                .map(|id| id.trim().to_owned());
            continue;
        }
        let Some((name, value)) = trimmed.split_once("REG_SZ")
            .or_else(|| trimmed.split_once("REG_EXPAND_SZ")) else { continue; };
        if !["Install Dir", "InstallDir"].iter().any(|candidate| name.trim().eq_ignore_ascii_case(candidate)) { continue; }
        let expanded_path = expand_windows_environment_variables(value.trim().trim_matches('"'));
        let install_path = PathBuf::from(expanded_path);
        if !install_path.is_dir() || !install_path.join("__Installer/installerdata.xml").is_file() {
            continue;
        }
        let Ok(metadata) = fs::read_to_string(install_path.join("__Installer/installerdata.xml")) else {
            continue;
        };
        let name = xml_element_text(&metadata, "gameTitle")
            .filter(|value| !value.trim().is_empty())
            .or_else(|| install_path.file_name().and_then(|value| value.to_str()).map(str::to_owned));
        let Some(name) = name.filter(|value| !value.trim().is_empty()) else { continue; };
        let platform_id = xml_element_text(&metadata, "contentID")
            .filter(|value| !value.trim().is_empty())
            .or_else(|| current_id.clone().filter(|value| !value.is_empty()));
        let path_key = install_path_key(&install_path);
        if platform_id.as_ref().is_some_and(|id| seen_ids.contains(id))
            || seen_paths.contains(&path_key)
        {
            continue;
        }
        // Reserve identifiers and paths only after the record passes both
        // deduplication checks, so a rejected alias cannot hide a later install.
        if let Some(id) = &platform_id {
            seen_ids.insert(id.clone());
        }
        seen_paths.insert(path_key);
        games.push(DetectedGame {
            platform_id,
            name,
            install_path,
            platform: "EA app",
        });
    }

    let mut seen_paths = BTreeSet::new();
    games.retain(|game| insert_unique_install_path(&mut seen_paths, &game.install_path));
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Reads EA app's machine-wide registry records, including custom game locations.
#[cfg(windows)]
pub fn discover_ea_games() -> Vec<DetectedGame> {
    const INSTALL_KEYS: [&str; 2] = [
        r"HKLM\SOFTWARE\EA Games",
        r"HKLM\SOFTWARE\WOW6432Node\EA Games",
    ];
    let mut games = Vec::new();
    for key in INSTALL_KEYS {
        let output = match std::process::Command::new("reg")
            .args(["query", key, "/s"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        games.extend(parse_ea_registry_output(&String::from_utf8_lossy(&output.stdout)));
    }
    let mut seen_paths = BTreeSet::new();
    games.retain(|game| insert_unique_install_path(&mut seen_paths, &game.install_path));
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Reads EA app installations using the installer metadata shipped with each game.
pub fn scan_ea_games(roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();

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
            if insert_unique_install_path(&mut seen_paths, &install_path) {
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
    // EA metadata may contain tag-looking text in comments or CDATA.
    let open_start = find_xml_tag(contents, element, false, 0)?;
    let open_end = find_xml_tag_end(contents, open_start)?;
    let close_start = find_xml_tag(contents, element, true, open_end)?;
    let raw = contents[open_end..close_start].trim();
    if raw.is_empty() {
        return None;
    }
    Some(decode_xml_entities(raw))
}

/// Finds an exact opening or closing tag while skipping comments, CDATA and
/// processing instructions. This is a focused metadata reader, not a full XML parser.
fn find_xml_tag(contents: &str, element: &str, closing: bool, mut from: usize) -> Option<usize> {
    let tag = if closing { format!("</{element}") } else { format!("<{element}") };

    while let Some(relative) = contents.get(from..)?.find('<') {
        let start = from + relative;
        let remaining = &contents[start..];
        let skip_marker = if remaining.starts_with("<!--") {
            Some("-->")
        } else if remaining.starts_with("<![CDATA[") {
            Some("]]>")
        } else if remaining.starts_with("<?") {
            Some("?>")
        } else {
            None
        };
        if let Some(marker) = skip_marker {
            from = start + remaining.find(marker)? + marker.len();
            continue;
        }
        if remaining.starts_with(&tag) {
            let after_name = start + tag.len();
            let next = *contents.as_bytes().get(after_name)?;
            if next == b'>' || (!closing && next.is_ascii_whitespace()) {
                return Some(start);
            }
        }
        from = start + 1;
    }
    None
}

/// Finds the end of an opening tag without treating '>' inside attributes as markup.
fn find_xml_tag_end(contents: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, character) in contents.get(start..)?.char_indices() {
        match (quote, character) {
            (Some(delimiter), current) if delimiter == current => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '>') => return Some(start + offset + 1),
            _ => {}
        }
    }
    None
}

/// Decodes the five predefined XML entities and valid numeric character references.
/// Unknown or invalid entities are preserved rather than silently corrupted.
fn decode_xml_entities(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut remaining = raw;

    while let Some(start) = remaining.find('&') {
        decoded.push_str(&remaining[..start]);
        let after_ampersand = &remaining[start + 1..];
        let Some(end) = after_ampersand.find(';') else {
            decoded.push_str(&remaining[start..]);
            return decoded;
        };
        let entity = &after_ampersand[..end];
        let character = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "amp" => Some('&'),
            _ => {
                let numeric = entity.strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|value| u32::from_str_radix(value, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|value| value.parse::<u32>().ok()));
                numeric.and_then(xml_char_from_reference)
            }
        };

        if let Some(character) = character {
            decoded.push(character);
        } else {
            decoded.push('&');
            decoded.push_str(entity);
            decoded.push(';');
        }
        remaining = &after_ampersand[end + 1..];
    }

    decoded.push_str(remaining);
    decoded
}

/// Converts a numeric XML character reference only when it is legal in XML 1.0.
fn xml_char_from_reference(value: u32) -> Option<char> {
    let is_valid = matches!(value, 0x9 | 0xA | 0xD)
        || (0x20..=0xD7FF).contains(&value)
        || (0xE000..=0xFFFD).contains(&value)
        || (0x10000..=0x10FFFF).contains(&value);
    is_valid.then(|| char::from_u32(value)).flatten()
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
    let mut seen_ids = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        let install_key_prefixes = [
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\",
            r"HKEY_LOCAL_MACHINE\SOFTWARE\Ubisoft\Launcher\Installs\",
        ];
        if trimmed.starts_with("HKEY_") {
            // reg.exe /s emits unrelated subkeys as well. Never let an
            // InstallDir value under another key inherit the previous game ID.
            current_id = install_key_prefixes.iter()
                .find_map(|prefix| trimmed.strip_prefix(prefix))
                .map(|id| id.trim().to_owned());
            continue;
        }
        if let Some((_, value)) = trimmed.split_once("REG_SZ")
            .or_else(|| trimmed.split_once("REG_EXPAND_SZ"))
            .filter(|(name, _)| name.trim().eq_ignore_ascii_case("InstallDir"))
        {
            let expanded_path = expand_windows_environment_variables(value.trim().trim_matches('"'));
            let install_path = PathBuf::from(expanded_path);
            if !install_path.is_dir() {
                continue;
            }
            let Some(name) = install_path.file_name().and_then(|value| value.to_str())
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            if let Some(platform_id) = current_id.as_ref().filter(|id| !id.is_empty()) {
                // Do not reserve an ID until its installation path is accepted.
                // A duplicate path may belong to another registry record; a later
                // valid path for this same ID must still be eligible for discovery.
                if !seen_ids.contains(platform_id)
                    && insert_unique_install_path(&mut seen_paths, &install_path)
                {
                    seen_ids.insert(platform_id.clone());
                    games.push(DetectedGame {
                        platform_id: Some(platform_id.clone()),
                        name: name.to_owned(),
                        install_path,
                        platform: "Ubisoft Connect",
                    });
                }
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
    let mut seen_paths = BTreeSet::new();
    games.retain(|game| insert_unique_install_path(&mut seen_paths, &game.install_path));
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}

/// Finds game folders inside known libraries for launchers without a stable,
/// documented local manifest format. Only immediate child folders are considered.
pub fn scan_game_folders(platform: &'static str, roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();
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
            if insert_unique_install_path(&mut seen_paths, &path) {
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
    let mut seen_paths = BTreeSet::new();
    let mut seen_product_codes = BTreeSet::new();
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
        // A product code identifies one Battle.net product. Stale or duplicated
        // Agent records must not surface the same game more than once merely
        // because they point at different installation directories.
        let product_key = product_code.to_ascii_lowercase();
        if seen_product_codes.contains(&product_key) {
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
        if !insert_unique_install_path(&mut seen_paths, &install_path) {
            continue;
        }
        let name = battlenet_product_name(&product_key)
            .map(str::to_owned)
            .or_else(|| install_path.file_name().and_then(|name| name.to_str()).map(str::to_owned));
        seen_product_codes.insert(product_key);
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
        // Protobuf reserves field numbers above 2^29 - 1. Reject invalid
        // tags instead of interpreting malformed bytes as product metadata.
        if field == 0 || field > 0x1fff_ffff {
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
        if field == 0 || field > 0x1fff_ffff { return None; }
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
        if let Some(root) = program_files.as_ref() {
            games.extend(scan_ea_games(&[root.join("EA Games")]));
            games.extend(discover_ea_games());
            // GOG is detected from Galaxy's installed-product database rather
            // than guessed from folders that may contain unrelated files.
            // Xbox is intentionally not inferred from `XboxGames` folders:
            // that directory alone does not prove a package is installed.
            // Add Xbox discovery only when package metadata can identify games
            // reliably without bypassing Windows package permissions.
        }
        // Ubisoft is detected from its registry install records. A generic
        // folder scan could report manually-created or leftover directories.
        games.extend(discover_ubisoft_games());
        if let Some(database) = battlenet_product_database() {
            games.extend(scan_battlenet_product_db(&database));
        }
    }

    // Do not infer installed games from generic `GOG Games` directories.
    // GOG Galaxy metadata is the source of truth when its database is available.

    games.sort_by_key(|game| (game.platform, game.name.to_lowercase()));
    let mut seen_installations = BTreeSet::new();
    games.retain(|game| {
        insert_unique_platform_install_path(
            &mut seen_installations,
            game.platform,
            &game.install_path,
        )
    });
    games
}

/// Adds an install path once per platform, respecting the host filesystem's case rules.
fn insert_unique_platform_install_path(
    seen: &mut BTreeSet<(String, PathBuf)>,
    platform: &str,
    candidate: &Path,
) -> bool {
    seen.insert((platform.to_owned(), install_path_key(candidate)))
}

/// Adds an install path once, respecting the host filesystem's case rules.
fn insert_unique_install_path(seen: &mut BTreeSet<PathBuf>, candidate: &Path) -> bool {
    seen.insert(install_path_key(candidate))
}

/// Builds an ordered key using the host filesystem's case rules.
fn install_path_key(path: &Path) -> PathBuf {
    // Existing installations can be referenced through junctions/symlinks or
    // through different relative path spellings. Canonicalize when possible so
    // those aliases do not appear as separate installs. Keep the original path
    // as a fallback: callers may also use this helper with synthetic paths.
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    #[cfg(windows)]
    {
        PathBuf::from(normalized.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

/// Compares paths for platform-specific case-rule tests.
#[cfg(test)]
fn same_install_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn find_vdf_value(contents: &str, wanted_key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (key, value) = quoted_pair(line)?;
        (key == wanted_key).then(|| unescape_vdf(&value))
    })
}

fn quoted_pair(line: &str) -> Option<(String, String)> {
    let mut quote_positions = Vec::with_capacity(4);
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character == '"' && !escaped {
            quote_positions.push(index);
        }
        if character == '\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }

    let key_start = *quote_positions.first()?;
    let key_end = *quote_positions.get(1)?;
    let value_start = *quote_positions.get(2)?;
    let value_end = *quote_positions.get(3)?;
    let key = line.get(key_start + 1..key_end)?;
    let value = line.get(value_start + 1..value_end)?;
    Some((key.to_owned(), value.to_owned()))
}

fn unescape_vdf(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next() {
                Some('\\') => decoded.push('\\'),
                Some('"') => decoded.push('"'),
                Some(next) => {
                    decoded.push('\\');
                    decoded.push(next);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    #[test]
    fn unescapes_vdf_sequences_without_consuming_adjacent_backslashes() {
        assert_eq!(super::unescape_vdf(r#"C:\\Games\\Example"#), r#"C:\Games\Example"#);
        assert_eq!(super::unescape_vdf(r#"quoted\\\"name"#), r#"quoted\"name"#);
        assert_eq!(super::unescape_vdf(r#"unknown\q"#), r#"unknown\q"#);
        assert_eq!(super::unescape_vdf("trailing\\"), "trailing\\");
    }

    #[test]
    fn preserves_numeric_references_to_characters_forbidden_by_xml_10() {
        assert_eq!(
            super::decode_xml_entities("A&#0;B &#x1; &#xD800; &#xFFFE; &#x10FFFF;"),
            "A&#0;B &#x1; &#xD800; &#xFFFE; \u{10FFFF}"
        );
        assert_eq!(
            super::decode_xml_entities("line&#10;break&#13;return"),
            "line\nbreak\rreturn"
        );
    }

    #[test]
    fn decodes_numeric_xml_entities_without_redecoding_escaped_text() {
        assert_eq!(
            super::decode_xml_entities("Rock &#38; Roll &#x1F680; &amp;lt; &unknown;"),
            "Rock & Roll 🚀 &lt; &unknown;"
        );
        assert_eq!(super::decode_xml_entities("Bad &#xZZ; ref"), "Bad &#xZZ; ref");
    }

    use super::{
        parse_app_manifest, parse_library_folders, scan_epic_manifests, scan_game_folders,
        scan_steam_libraries,
    };
    use std::{collections::BTreeSet, path::Path};

    #[cfg(not(windows))]
    #[test]
    fn install_path_comparison_preserves_case_on_case_sensitive_filesystems() {
        assert!(!super::same_install_path(
            Path::new("/games/Example"),
            Path::new("/games/example"),
        ));
        assert!(super::same_install_path(
            Path::new("/games/Example"),
            Path::new("/games/Example"),
        ));
    }

    #[cfg(windows)]
    #[test]
    fn install_path_comparison_ignores_case_on_windows() {
        assert!(super::same_install_path(
            Path::new(r"C:\Games\Example"),
            Path::new(r"c:\games\example"),
        ));
    }

    #[test]
    fn global_deduplication_keeps_non_adjacent_paths_and_separate_platforms() {
        let mut seen = BTreeSet::new();
        assert!(super::insert_unique_platform_install_path(
            &mut seen, "Steam", Path::new("/games/shared")
        ));
        assert!(super::insert_unique_platform_install_path(
            &mut seen, "Steam", Path::new("/games/other")
        ));
        assert!(!super::insert_unique_platform_install_path(
            &mut seen, "Steam", Path::new("/games/shared")
        ));
        assert!(super::insert_unique_platform_install_path(
            &mut seen, "Epic Games", Path::new("/games/shared")
        ));
        assert_eq!(seen.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn global_deduplication_collapses_symlink_aliases_to_same_install() {
        let root = std::env::temp_dir().join(format!(
            "amethys-discovery-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let actual = root.join("actual");
        let alias = root.join("alias");
        std::fs::create_dir_all(&actual).expect("create install directory");
        std::os::unix::fs::symlink(&actual, &alias).expect("create install alias");

        let mut seen = BTreeSet::new();
        assert!(super::insert_unique_platform_install_path(
            &mut seen, "Steam", &actual
        ));
        assert!(!super::insert_unique_platform_install_path(
            &mut seen, "Steam", &alias
        ));
        assert_eq!(seen.len(), 1);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn quoted_vdf_pairs_ignore_escaped_quotes_inside_values() {
        assert_eq!(
            super::quoted_pair(r#""path" "C:\\Games\\A \"Quoted\" Game""#),
            Some(("path".to_owned(), r#"C:\\Games\\A \"Quoted\" Game"#.to_owned())),
        );
    }

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
    fn ignores_numeric_app_ids_inside_nested_steam_library_metadata() {
        let contents = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path" "C:\\Steam"
                    "apps"
                    {
                        "570" "123456"
                        "730" "654321"
                    }
                }
                "1" "D:\\SteamLibrary"
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("C:/DefaultSteam"));
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&Path::new(r"C:\Steam").to_path_buf()));
        assert!(paths.contains(&Path::new(r"D:\SteamLibrary").to_path_buf()));
        assert!(!paths.contains(&Path::new("123456").to_path_buf()));
        assert!(!paths.contains(&Path::new("654321").to_path_buf()));
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
    fn steam_manifest_filename_must_match_embedded_app_id() {
        let root = std::env::temp_dir().join(format!(
            "amethys-steam-mismatched-id-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let steamapps = root.join("steamapps");
        let install = steamapps.join("common").join("Example");
        std::fs::create_dir_all(&install).expect("create game install");
        std::fs::write(
            steamapps.join("appmanifest_570.acf"),
            "\n                \"appid\" \"730\"\n                \"name\" \"Wrong attribution\"\n                \"installdir\" \"Example\"\n            ",
        ).expect("write mismatched manifest");

        assert!(scan_steam_libraries(std::slice::from_ref(&root)).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn steam_manifest_rejects_non_numeric_filename_identifiers() {
        let root = std::env::temp_dir().join(format!(
            "amethys-steam-nonnumeric-id-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let steamapps = root.join("steamapps");
        let install = steamapps.join("common").join("Example");
        std::fs::create_dir_all(&install).expect("create game install");
        std::fs::write(
            steamapps.join("appmanifest_example.acf"),
            "\n                \"appid\" \"example\"\n                \"name\" \"Example\"\n                \"installdir\" \"Example\"\n            ",
        ).expect("write non-numeric manifest");

        assert!(scan_steam_libraries(std::slice::from_ref(&root)).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
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
        let whitespace_install = root.join("Installed").join("Whitespace Game");
        let duplicate_install = root.join("Installed").join("Duplicate Example");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&installed).expect("create game folder");
        std::fs::create_dir_all(&whitespace_install).expect("create whitespace game folder");
        std::fs::create_dir_all(&duplicate_install).expect("create duplicate game folder");
        std::fs::write(manifests.join("installed.item"), format!(
            r#"{{"AppName":"ExampleApp","DisplayName":"Example Game","InstallLocation":{}}}"#,
            serde_json::to_string(&installed.to_string_lossy()).expect("encode path")
        )).expect("write manifest");
        std::fs::write(manifests.join("z-duplicate.item"), format!(
            r#"{{"AppName":"ExampleApp","DisplayName":"Duplicate Example","InstallLocation":{}}}"#,
            serde_json::to_string(&duplicate_install.to_string_lossy()).expect("encode duplicate path")
        )).expect("write duplicate app manifest");
        std::fs::write(manifests.join("missing.item"),
            r#"{"AppName":"Missing","DisplayName":"Missing Game","InstallLocation":"not-a-real-path"}"#
        ).expect("write stale manifest");
        std::fs::write(manifests.join("broken.item"), "{broken").expect("write invalid manifest");
        std::fs::write(
            manifests.join("whitespace.item"),
            format!(
                r#"{{"AppName":"  ","DisplayName":"  Example Game  ","InstallLocation":{}}}"#,
                serde_json::to_string(&format!("  {}  ", whitespace_install.display()))
                    .expect("encode whitespace install path")
            ),
        ).expect("write manifest with surrounding whitespace");

        let games = scan_epic_manifests(&manifests);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].platform, "Epic Games");
        assert_eq!(games.iter().filter(|game| game.platform_id.as_deref() == Some("ExampleApp")).count(), 1);
        let whitespace_game = games.iter().find(|game| game.install_path == whitespace_install).unwrap();
        assert_eq!(whitespace_game.name, "Example Game");
        assert_eq!(whitespace_game.platform_id, None);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn reads_epic_manifests_with_uppercase_item_extension() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-extension-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let manifests = root.join("Manifests");
        let install = root.join("Installed Game");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&install).expect("create install");
        let json = format!(
            r#"{{"AppName":"UppercaseExtension","DisplayName":"Uppercase Extension","InstallLocation":{}}}"#,
            serde_json::to_string(&install.to_string_lossy()).expect("encode path")
        );
        std::fs::write(manifests.join("uppercase.ITEM"), json)
            .expect("write manifest");

        let games = scan_epic_manifests(&manifests);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Uppercase Extension");
        assert_eq!(games[0].platform_id.as_deref(), Some("UppercaseExtension"));
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
        connection.execute(
            "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
            rusqlite::params!["duplicate-product", install.to_string_lossy()],
        )
        .expect("insert duplicate install path");
        connection.execute("INSERT INTO GamePieces VALUES (?1, 1, ?2)",
            rusqlite::params!["gog_duplicate-product", r#"{"title":"Duplicate entry"}"#])
            .expect("insert duplicate GOG title");
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
    fn keeps_gog_product_id_available_after_rejecting_duplicate_path() {
        use rusqlite::{params, Connection};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_second_install = root.join("Second Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_second_install).expect("create second install");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection.execute_batch(
            "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);
             INSERT INTO GamePieceTypes VALUES (1, 'originalTitle');"
        ).expect("create GOG metadata tables");
        for (id, path, title) in [
            ("first", first_install.to_string_lossy().into_owned(), "First Game"),
            ("second", first_install.to_string_lossy().into_owned(), "Duplicate Path"),
            ("second", valid_second_install.to_string_lossy().into_owned(), "Second Game"),
        ] {
            connection.execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)", params![id, path],
            ).expect("insert GOG product");
            connection.execute(
                "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                params![format!("gog_{id}"), serde_json::json!({"title": title}).to_string()],
            ).expect("insert GOG title");
        }
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| game.platform_id.as_deref() == Some("second")
            && game.install_path == valid_second_install));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn skips_invalid_gog_titles_and_stale_install_paths() {
        use rusqlite::{params, Connection};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let installed = root.join("Valid Game");
        std::fs::create_dir_all(&installed).expect("create installed game");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection.execute_batch(
            "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);
             INSERT INTO GamePieceTypes VALUES (1, 'originalTitle');"
        ).expect("create GOG metadata tables");

        let rows = [
            ("valid", installed.to_string_lossy().into_owned(), r#"{"title":"Valid Game"}"#),
            ("bad-json", installed.to_string_lossy().into_owned(), "{not-json"),
            ("empty-title", installed.to_string_lossy().into_owned(), r#"{"title":"  "}"#),
            ("stale-path", root.join("Removed Game").to_string_lossy().into_owned(), r#"{"title":"Removed Game"}"#),
        ];
        for (id, location, title) in rows {
            connection.execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                params![id, location],
            ).expect("insert GOG product");
            connection.execute(
                "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                params![format!("gog_{id}"), title],
            ).expect("insert GOG title");
        }
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("valid"));
        assert_eq!(games[0].name, "Valid Game");
        assert_eq!(games[0].install_path, installed);
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
    fn detects_ea_games_from_registry_install_paths_outside_program_files() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-registry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Games").join("Custom EA Game");
        std::fs::create_dir_all(install.join("__Installer")).expect("create EA install metadata");
        std::fs::write(
            install.join("__Installer/installerdata.xml"),
            r#"<DiPManifest><gameTitle>Custom EA Game</gameTitle><contentID>EA:custom-123</contentID></DiPManifest>"#,
        ).expect("write EA metadata");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\CustomGame\n    Install Dir    REG_SZ    {}\n",
            install.display()
        );

        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, "EA app");
        assert_eq!(games[0].platform_id.as_deref(), Some("EA:custom-123"));
        assert_eq!(games[0].name, "Custom EA Game");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn detects_ea_games_from_32_bit_registry_key_and_installdir_value() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-registry-wow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Games").join("Another EA Game");
        std::fs::create_dir_all(install.join("__Installer")).expect("create EA install metadata");
        std::fs::write(
            install.join("__Installer/installerdata.xml"),
            r#"<DiPManifest><gameTitle>Another EA Game</gameTitle></DiPManifest>"#,
        ).expect("write EA metadata");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\EA Games\\legacy-id\n    installdir    REG_SZ    {}\n",
            install.display()
        );

        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("legacy-id"));
        assert_eq!(games[0].name, "Another EA Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_ea_registry_paths_without_valid_game_metadata() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-registry-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create unrelated directory");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\Fake\\n    Install Dir    REG_SZ    {}\\n",
            root.display()
        );
        assert!(super::parse_ea_registry_output(&output).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn keeps_ubisoft_id_available_after_rejecting_duplicate_install_path() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_install = root.join("Valid Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_install).expect("create valid install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\first\\n    InstallDir    REG_SZ    {}\\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\target\\n    InstallDir    REG_SZ    {}\\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\target\\n    InstallDir    REG_SZ    {}\\n",
            first_install.display(),
            first_install.display(),
            valid_install.display(),
        );

        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| {
            game.platform_id.as_deref() == Some("target")
                && game.install_path == valid_install
        }));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn expands_windows_environment_variables_and_preserves_unknown_names() {
        let expanded = super::expand_windows_environment_variables_with(
            r#"%GAMES_ROOT%\\Title\\%MISSING%"#,
            |name| (name == "GAMES_ROOT").then(|| r#"D:\\Game Library"#.to_owned()),
        );
        assert_eq!(expanded, r#"D:\\Game Library\\Title\\%MISSING%"#);
        assert_eq!(
            super::expand_windows_environment_variables_with("100%% complete", |_| None),
            "100%% complete",
        );
    }

    #[test]
    fn ignores_ea_tag_text_inside_comments_and_cdata() {
        let metadata = r#"
            <!-- <gameTitle>Commented fake title</gameTitle> -->
            <gameTitle data-note="a > b">Real &amp; Valid Title</gameTitle>
            <![CDATA[ <contentID>fake-id</contentID> ]]>
            <contentID>real-id</contentID>
        "#;
        assert_eq!(super::xml_element_text(metadata, "gameTitle").as_deref(), Some("Real & Valid Title"));
        assert_eq!(super::xml_element_text(metadata, "contentID").as_deref(), Some("real-id"));
    }

    #[test]
    fn xml_entity_decoding_does_not_decode_escaped_entity_text_twice() {
        assert_eq!(
            super::xml_element_text("<gameTitle>Rock &amp;lt; Roll &amp;amp; More</gameTitle>", "gameTitle")
                .as_deref(),
            Some("Rock &lt; Roll &amp; More"),
        );
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
        connection.execute(
            "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["amzn1.blue-fire-alias", installed.to_string_lossy(), "Duplicate Blue Fire"],
        ).expect("insert duplicate installation path");
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
    fn skips_malformed_gog_rows_without_losing_valid_games() {
        use rusqlite::{params, Connection};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-malformed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let installed = root.join("Valid GOG Game");
        std::fs::create_dir_all(&installed).expect("create install");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create database");
        connection.execute_batch(
            "CREATE TABLE InstalledBaseProducts (productId, installationPath);
             CREATE TABLE GamePieces (releaseKey, gamePieceTypeId, value);
             CREATE TABLE GamePieceTypes (id, type);"
        ).expect("create GOG metadata tables");
        connection.execute(
            "INSERT INTO GamePieceTypes VALUES (?1, ?2)",
            params![1_i64, "originalTitle"],
        ).expect("insert title type");
        connection.execute(
            "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
            params![42_i64, installed.to_string_lossy()],
        ).expect("insert malformed product row");
        connection.execute(
            "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
            params!["valid-product", installed.to_string_lossy()],
        ).expect("insert valid product row");
        connection.execute(
            "INSERT INTO GamePieces VALUES (?1, ?2, ?3)",
            params!["gog_42", 1_i64, r#"{"title":"Malformed Game"}"#],
        ).expect("insert malformed title metadata");
        connection.execute(
            "INSERT INTO GamePieces VALUES (?1, ?2, ?3)",
            params!["gog_valid-product", 1_i64, r#"{"title":"Valid GOG Game"}"#],
        ).expect("insert valid title metadata");
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("valid-product"));
        assert_eq!(games[0].name, "Valid GOG Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn skips_malformed_amazon_rows_without_losing_valid_games() {
        use rusqlite::{params, Connection};

        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-malformed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let installed = root.join("Valid Game");
        std::fs::create_dir_all(&installed).expect("create install");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create database");
        connection.execute_batch(
            "CREATE TABLE DbSet (Id, InstallDirectory, ProductTitle);"
        ).expect("create metadata table");
        connection.execute(
            "INSERT INTO DbSet VALUES (?1, ?2, ?3)",
            params![42_i64, installed.to_string_lossy(), "Malformed ID"],
        ).expect("insert malformed ID row");
        connection.execute(
            "INSERT INTO DbSet VALUES (?1, ?2, ?3)",
            params!["amzn.valid", installed.to_string_lossy(), "Valid Game"],
        ).expect("insert valid row");
        drop(connection);

        let games = super::scan_amazon_games(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("amzn.valid"));
        assert_eq!(games[0].name, "Valid Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn keeps_amazon_product_id_available_after_rejecting_duplicate_path() {
        use rusqlite::{params, Connection};

        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_second_install = root.join("Second Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_second_install).expect("create second install");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create fixture database");
        connection.execute_batch(
            "CREATE TABLE DbSet (Id TEXT, InstallDirectory TEXT, ProductTitle TEXT);"
        ).expect("create Amazon metadata table");
        for (id, path, title) in [
            ("amzn.first", first_install.to_string_lossy().into_owned(), "First Game"),
            ("amzn.second", first_install.to_string_lossy().into_owned(), "Duplicate Path"),
            ("amzn.second", valid_second_install.to_string_lossy().into_owned(), "Second Game"),
        ] {
            connection.execute(
                "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle) VALUES (?1, ?2, ?3)",
                params![id, path, title],
            ).expect("insert Amazon product");
        }
        drop(connection);

        let games = super::scan_amazon_games(&database);
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| game.platform_id.as_deref() == Some("amzn.second")
            && game.install_path == valid_second_install));
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
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\55\n    installdir    REG_SZ    \"{}\"\n",
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

    #[test]
    fn deduplicates_ea_registry_records_by_content_id() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-duplicates-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("Game");
        let duplicate_install = root.join("Game-copy");
        for install in [&first_install, &duplicate_install] {
            std::fs::create_dir_all(install.join("__Installer"))
                .expect("create installer metadata directory");
            std::fs::write(
                install.join("__Installer/installerdata.xml"),
                "<gameTitle>Same Game</gameTitle><contentID>EA-123</contentID>",
            ).expect("write EA installer metadata");
        }
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\First\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\EA Games\\Second\n    Install Dir    REG_SZ    {}\n",
            first_install.display(), duplicate_install.display()
        );
        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("EA-123"));
        assert_eq!(games[0].install_path, first_install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn keeps_ea_content_id_available_after_rejecting_duplicate_install_path() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_second_install = root.join("Second Game");
        for install in [&first_install, &valid_second_install] {
            std::fs::create_dir_all(install.join("__Installer"))
                .expect("create installer metadata directory");
        }
        for (install, title) in [
            (&first_install, "First Game"),
            (&valid_second_install, "Second Game"),
        ] {
            std::fs::write(
                install.join("__Installer/installerdata.xml"),
                format!("<gameTitle>{title}</gameTitle>"),
            ).expect("write EA installer metadata");
        }
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\First\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\Second\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\Second\n    Install Dir    REG_SZ    {}\n",
            first_install.display(), first_install.display(), valid_second_install.display()
        );
        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| game.platform_id.as_deref() == Some("Second")
            && game.install_path == valid_second_install));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn does_not_attribute_unrelated_registry_install_values_to_previous_ea_game() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-unrelated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let ea_install = root.join("EA Game");
        let unrelated_install = root.join("Other Vendor Game");
        std::fs::create_dir_all(ea_install.join("__Installer"))
            .expect("create EA installer metadata directory");
        std::fs::write(
            ea_install.join("__Installer/installerdata.xml"),
            "<gameTitle>Valid Game</gameTitle><contentID>EA-123</contentID>",
        ).expect("write EA installer metadata");
        std::fs::create_dir_all(&unrelated_install)
            .expect("create unrelated install directory");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\EA-123\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\OtherVendor\\Installs\\99\n    Install Dir    REG_SZ    {}\n",
            ea_install.display(), unrelated_install.display()
        );
        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].install_path, ea_install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn does_not_attribute_unrelated_registry_install_values_to_previous_ubisoft_game() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-unrelated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("Not Ubisoft");
        std::fs::create_dir_all(&install).expect("create unrelated install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\OtherVendor\\Installs\\99\n    InstallDir    REG_SZ    {}\n",
            install.display(), install.display()
        );
        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("44"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deduplicates_ubisoft_registry_records_by_game_id_and_install_path() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-duplicates-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("Game");
        let duplicate_install = root.join("Game-copy");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&duplicate_install).expect("create duplicate install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\n",
            first_install.display(), duplicate_install.display()
        );
        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("44"));
        assert_eq!(games[0].install_path, first_install);
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
    fn rejects_protobuf_field_numbers_above_the_specification_limit() {
        let invalid_field = 0x2000_0000u64;
        let length_delimited_tag = (invalid_field << 3) | 2;
        let varint_tag = invalid_field << 3;

        let mut length_delimited = encode_varint(length_delimited_tag);
        length_delimited.push(0);
        assert!(super::protobuf_length_delimited_fields(&length_delimited, 1).is_none());

        let mut varint = encode_varint(varint_tag);
        varint.push(1);
        assert!(super::protobuf_varint_field(&varint, 1).is_none());
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
    fn deduplicates_battlenet_records_by_product_code() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-duplicates-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let first_install = root.join("WoW");
        let second_install = root.join("WoW-copy");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&second_install).expect("create second install");

        let make_product = |code: &[u8], install: &Path| {
            let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
            let base_state = encode_varint(8).into_iter().chain(encode_varint(1)).collect::<Vec<_>>();
            let cached_state = encode_bytes_field(1, &base_state);
            let mut product = encode_bytes_field(2, code);
            product.extend(encode_bytes_field(3, &settings));
            product.extend(encode_bytes_field(4, &cached_state));
            product
        };
        let mut database_bytes = encode_bytes_field(1, &make_product(b"wow", &first_install));
        database_bytes.extend(encode_bytes_field(1, &make_product(b"WOW", &second_install)));
        let database = root.join("product.db");
        std::fs::write(&database, database_bytes).expect("write protobuf fixture");

        let games = super::scan_battlenet_product_db(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("wow"));
        assert_eq!(games[0].install_path, first_install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn recognizes_battlenet_product_names_with_uppercase_codes() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-uppercase-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("WoW");
        std::fs::create_dir_all(&install).expect("create install");

        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8).into_iter().chain(encode_varint(1)).collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"WOW");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database = root.join("product.db");
        std::fs::write(&database, encode_bytes_field(1, &product))
            .expect("write protobuf fixture");

        let games = super::scan_battlenet_product_db(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("WOW"));
        assert_eq!(games[0].name, "World of Warcraft");
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
        let whitespace_install = root.join("Celeste");
        let duplicate_install = root.join("Duplicate Hades");
        std::fs::create_dir_all(&installed).expect("create installed game");
        std::fs::create_dir_all(&whitespace_install).expect("create whitespace fixture");
        std::fs::create_dir_all(&duplicate_install).expect("create duplicate fixture");
        let database = root.join("installed.json");
        let contents = serde_json::json!({
            "HadesApp": { "app_name": " HadesApp ", "title": " Hades ", "install_path": format!(" {} ", installed.display()) },
            "ZDuplicateHades": { "app_name": "HadesApp", "title": "Duplicate Hades", "install_path": duplicate_install.to_string_lossy() },
            "CelesteApp": { "app_name": " CelesteApp ", "title": " Celeste ", "install_path": format!(" {} ", whitespace_install.display()) },
            "RemovedApp": { "app_name": "RemovedApp", "title": "Removed Game", "install_path": root.join("Removed Game").to_string_lossy() },
            "BrokenApp": { "app_name": "BrokenApp", "title": "Broken Game" }
        });
        std::fs::write(&database, contents.to_string()).expect("write Legendary metadata");

        let games = super::scan_heroic_legendary_installed(&database);
        assert_eq!(games.len(), 2);
        assert!(games.iter().all(|game| game.platform == "Heroic"));
        let hades = games.iter().find(|game| game.platform_id.as_deref() == Some("HadesApp")).unwrap();
        assert_eq!(hades.name, "Hades");
        assert_eq!(hades.install_path, installed);
        let celeste = games.iter().find(|game| game.platform_id.as_deref() == Some("CelesteApp")).unwrap();
        assert_eq!(celeste.name, "Celeste");
        assert_eq!(celeste.install_path, whitespace_install);
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
    fn trims_whitespace_from_required_app_manifest_values() {
        let contents = r#"
            "AppState"
            {
                "appid" " 570 "
                "name" " Dota 2 "
                "installdir" " Dota 2 "
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
    #[test]
    fn falls_back_to_install_folder_for_unknown_battlenet_products() {
        let root = std::env::temp_dir().join(format!(
            "amethys-battlenet-unknown-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .expect("clock").as_nanos()
        ));
        let install = root.join("CustomGame");
        std::fs::create_dir_all(&install).expect("create install");

        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8).into_iter().chain(encode_varint(1)).collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"future_product_code");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database = root.join("product.db");
        std::fs::write(&database, encode_bytes_field(1, &product))
            .expect("write protobuf fixture");

        let games = super::scan_battlenet_product_db(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("future_product_code"));
        assert_eq!(games[0].name, "CustomGame");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

}
