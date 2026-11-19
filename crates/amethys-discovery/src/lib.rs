mod xbox;
pub use xbox::{discover_xbox_games, scan_xbox_game_roots};

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Maximum size accepted for a single external launcher metadata file.
const MAX_METADATA_FILE_SIZE: u64 = 4 * 1024 * 1024;

/// Reads UTF-8 metadata with a strict allocation/read limit.
fn read_bounded_text(path: &Path) -> Option<String> {
    let mut contents = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_METADATA_FILE_SIZE + 1)
        .read_to_string(&mut contents)
        .ok()?;
    (contents.len() as u64 <= MAX_METADATA_FILE_SIZE).then_some(contents)
}

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
    // VDF metadata is external input. Never let a relative library entry (or
    // relative root supplied by a caller) resolve against the launcher's CWD.
    if is_absolute_metadata_path(steam_root) {
        paths.insert(steam_root.to_path_buf());
    }

    let mut depth = 0usize;
    let mut awaiting_library_open = false;
    let mut library_folders_depth = None;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("//")
            && depth == 0
            && trimmed
                .split('"')
                .nth(1)
                .is_some_and(|key| key.eq_ignore_ascii_case("libraryfolders"))
        {
            awaiting_library_open = true;
        }

        // Process entries only after entering the actual libraryfolders block.
        if let Some(root_depth) = library_folders_depth {
            if depth < root_depth {
                library_folders_depth = None;
            } else if let Some((key, value)) = quoted_pair(line) {
                let is_legacy_library = depth == root_depth && key.parse::<u32>().is_ok();
                let is_modern_library_path =
                    depth == root_depth + 1 && key.eq_ignore_ascii_case("path");
                if is_legacy_library || is_modern_library_path {
                    let value = unescape_vdf(&value);
                    let value = value.trim();
                    if !value.is_empty() {
                        let path = PathBuf::from(value);
                        if is_absolute_metadata_path(&path) {
                            paths.insert(path);
                        }
                    }
                }
            }
        }

        let next_depth = vdf_update_depth(depth, line);
        if awaiting_library_open && next_depth > depth {
            library_folders_depth = Some(next_depth);
            awaiting_library_open = false;
        }
        depth = next_depth;
    }

    paths.into_iter().collect()
}

/// Recognizes absolute paths from Windows and Unix launcher metadata even when
/// fixtures are parsed on a host with a different path syntax.
fn is_absolute_metadata_path(path: &Path) -> bool {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    path.is_absolute()
        || value.starts_with('/')
        || value.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// Counts VDF braces outside quoted strings so braces in paths do not affect nesting.
fn vdf_update_depth(depth: usize, line: &str) -> usize {
    let mut depth = depth;
    let mut in_quotes = false;
    let mut escaped = false;

    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
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
                '/' if characters.peek() == Some(&'/') => break,
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

    if install_dir.contains('/')
        || install_dir.contains('\\')
        || Path::new(&install_dir)
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    Some((app_id, name, install_dir))
}

/// Scans known Steam library folders without starting Steam or modifying any files.
pub fn scan_steam_libraries(libraries: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    // Configuration and filesystem enumeration order can vary. Sort both so
    // duplicate AppIDs resolve consistently across scans and platforms.
    let mut libraries = libraries.to_vec();
    libraries.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));

    for library in &libraries {
        let manifests = library.join("steamapps");
        let entries = match fs::read_dir(&manifests) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }

            let Some(contents) = read_bounded_text(&path) else {
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
            if filename_app_id
                .and_then(|value| value.parse::<u32>().ok())
                .is_none()
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

    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

fn is_steam_runtime_component(app_id: &str, name: &str) -> bool {
    matches!(
        app_id,
        "228980" | "1070560" | "1391110" | "1628350" | "1826330"
    ) || name.starts_with("Steamworks Common Redistributables")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Proton ")
}

/// Extracts Steam's registered installation root from Windows `reg query` output.
#[cfg(any(windows, test))]
fn parse_steam_registry_output(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let (name, value) = line
            .split_once("REG_SZ")
            .or_else(|| line.split_once("REG_EXPAND_SZ"))?;
        if !name.trim().eq_ignore_ascii_case("SteamPath") {
            return None;
        }
        let value = value.trim().trim_matches('"').trim();
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Builds per-user Steam roots for Unix layouts without reading process-global
/// environment state. This keeps Linux distro/Flatpak and macOS path rules
/// independently testable on every CI host.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn unix_steam_roots(home: &Path, xdg_data_home: Option<&Path>, target_os: &str) -> Vec<PathBuf> {
    if !is_absolute_metadata_path(home) {
        return Vec::new();
    }

    let mut roots = BTreeSet::new();
    match target_os {
        "linux" => {
            roots.insert(home.join(".steam/steam"));
            roots.insert(home.join(".local/share/Steam"));
            roots.insert(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            if let Some(data_home) = xdg_data_home.filter(|path| is_absolute_metadata_path(path)) {
                roots.insert(data_home.join("Steam"));
            }
        }
        "macos" => {
            roots.insert(home.join("Library/Application Support/Steam"));
        }
        _ => return Vec::new(),
    }
    roots.into_iter().collect()
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
        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
            .output()
        {
            if output.status.success() {
                if let Some(root) =
                    parse_steam_registry_output(&String::from_utf8_lossy(&output.stdout))
                {
                    roots.insert(root);
                }
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        #[cfg(target_os = "linux")]
        let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        #[cfg(target_os = "macos")]
        let xdg_data_home: Option<PathBuf> = None;
        roots.extend(unix_steam_roots(
            &home,
            xdg_data_home.as_deref(),
            std::env::consts::OS,
        ));
    }

    roots
        .into_iter()
        .filter(|path| path.is_absolute() && path.is_dir())
        .collect()
}
/// Reads libraryfolders.vdf files and returns their declared library roots.
pub fn discover_steam_libraries() -> Vec<PathBuf> {
    let mut libraries = BTreeSet::new();

    for root in steam_install_roots() {
        libraries.insert(root.clone());
        let metadata = root.join("steamapps/libraryfolders.vdf");
        let Some(contents) = read_bounded_text(&metadata) else {
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

/// Returns true only for an existing absolute directory reported by launcher metadata.
/// Relative paths are rejected rather than being resolved against the launcher's
/// current working directory, which could otherwise create false detections.
fn is_existing_absolute_directory(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
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
    entries.sort_by_key(|entry| {
        let filename = entry.file_name();
        (filename.to_string_lossy().to_ascii_lowercase(), filename)
    });
    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut seen_app_ids = BTreeSet::new();

    for entry in entries {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        let Some(contents) = read_bounded_text(&path) else {
            continue;
        };
        // Epic manifests may be written with a UTF-8 BOM.
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(contents) else {
            continue;
        };
        let Some(install_location) = manifest
            .get("InstallLocation")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        if !is_existing_absolute_directory(&install_path) {
            continue;
        }
        let Some(name) = manifest
            .get("DisplayName")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| install_path.file_name().and_then(|value| value.to_str()))
        else {
            continue;
        };
        let platform_id = manifest
            .get("AppName")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if platform_id
            .as_ref()
            .is_some_and(|id| seen_app_ids.contains(id))
        {
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

/// Resolves Epic's declared launch executable for an installed game.
///
/// The executable is accepted only when the manifest identifies the same app,
/// the path is relative, and the resolved file remains inside the installation.
pub fn epic_launch_executable(
    manifests_dir: &Path,
    app_id: &str,
    install_path: &Path,
) -> Option<PathBuf> {
    if app_id.trim().is_empty() || !is_existing_absolute_directory(install_path) {
        return None;
    }
    let entries = fs::read_dir(manifests_dir).ok()?;
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();

    let canonical_root = install_path.canonicalize().ok()?;
    for manifest_path in paths {
        if !manifest_path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        let Some(contents) = read_bounded_text(&manifest_path) else {
            continue;
        };
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(contents) else {
            continue;
        };
        let matches_app = manifest
            .get("AppName")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(app_id.trim()));
        let matches_install = manifest
            .get("InstallLocation")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .is_some_and(|value| Path::new(value) == install_path);
        if !matches_app || !matches_install {
            continue;
        }
        let executable = manifest
            .get("LaunchExecutable")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let relative = Path::new(executable);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return None;
        }
        let candidate = install_path.join(relative);
        let canonical_candidate = candidate.canonicalize().ok()?;
        if !canonical_candidate.is_file() || !canonical_candidate.starts_with(&canonical_root) {
            return None;
        }
        return canonical_candidate
            .strip_prefix(&canonical_root)
            .ok()
            .map(Path::to_path_buf);
    }
    None
}

/// Builds Epic Games Launcher manifest locations from explicit platform inputs.
/// Keeping path selection separate from environment access makes every OS layout
/// testable on every CI runner.
fn epic_manifest_roots_for(
    target_os: &str,
    home: Option<&Path>,
    program_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    match target_os {
        "windows" => {
            if let Some(program_data) = program_data.filter(|path| is_absolute_metadata_path(path))
            {
                roots.insert(program_data.join("Epic/EpicGamesLauncher/Data/Manifests"));
            }
        }
        "macos" => {
            if let Some(home) = home.filter(|path| is_absolute_metadata_path(path)) {
                roots.insert(
                    home.join("Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests"),
                );
            }
        }
        // On Linux, Epic game installations are discovered through Heroic /
        // Legendary metadata; the official Epic Games Launcher is not supported.
        _ => {}
    }
    roots.into_iter().collect()
}

/// Returns the usual Epic Games manifest folders for the current operating system.
pub fn epic_manifest_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let program_data = std::env::var_os("ProgramData").map(PathBuf::from);
    epic_manifest_roots_for(
        std::env::consts::OS,
        home.as_deref(),
        program_data.as_deref(),
    )
    .into_iter()
    .filter(|path| path.is_dir())
    .collect()
}

/// Reads Heroic/Legendary installed-game metadata without starting the launcher.
///
/// Legendary stores one JSON object per installed Epic game in installed.json.
/// Only entries with a non-empty app name/title and an existing install directory
/// are returned; malformed entries and stale paths are ignored.
pub fn scan_heroic_legendary_installed(installed_json: &Path) -> Vec<DetectedGame> {
    let Some(contents) = read_bounded_text(installed_json) else {
        return Vec::new();
    };
    // Some Windows tools export UTF-8 JSON with a BOM. Accept the same
    // harmless prefix already handled by the Epic manifest reader.
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
    let Ok(entries) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };
    let Some(entries) = entries.as_object() else {
        return Vec::new();
    };

    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut seen_app_ids = BTreeSet::new();
    // serde_json map ordering depends on enabled features; sort explicitly so
    // duplicate app IDs always resolve to the same record on every build.
    let mut entries: Vec<_> = entries.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (key, value) in entries {
        let Some(record) = value.as_object() else {
            continue;
        };
        let app_name = record
            .get("app_name")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(key.trim());
        if app_name.is_empty() || seen_app_ids.contains(app_name) {
            continue;
        }
        let Some(title) = record
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(install_location) = record
            .get("install_path")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let install_path = PathBuf::from(install_location);
        if !is_existing_absolute_directory(&install_path) {
            continue;
        }
        if !insert_unique_install_path(&mut seen_paths, &install_path) {
            continue;
        }
        seen_app_ids.insert(app_name.to_owned());
        games.push(DetectedGame {
            platform_id: Some(app_name.to_owned()),
            name: title.to_owned(),
            install_path,
            platform: "Heroic",
        });
    }
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

/// Resolves Linux XDG configuration storage without interpreting relative
/// environment values against the launcher's current working directory.
#[cfg(any(target_os = "linux", test))]
fn linux_heroic_config_home(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    xdg_config_home
        .filter(|path| is_absolute_metadata_path(path))
        .map(Path::to_path_buf)
        .or_else(|| {
            home.filter(|path| is_absolute_metadata_path(path))
                .map(|path| path.join(".config"))
        })
}

/// Returns known Heroic and standalone Legendary installed.json locations.
///
/// Heroic stores its configuration below Electron's appData/heroic directory;
/// standalone Legendary uses its own XDG-style config directory.
pub fn heroic_legendary_installed_files() -> Vec<PathBuf> {
    let mut files = BTreeSet::new();

    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        files.insert(
            PathBuf::from(app_data).join("heroic/legendaryConfig/legendary/installed.json"),
        );
    }

    #[cfg(target_os = "linux")]
    {
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let config_home = linux_heroic_config_home(xdg_config_home.as_deref(), home.as_deref());

        if let Some(config_home) = config_home {
            files.insert(config_home.join("heroic/legendaryConfig/legendary/installed.json"));
            files.insert(config_home.join("legendary/installed.json"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            files.insert(home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json"));
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        files.insert(
            home.join(
                "Library/Application Support/heroic/legendaryConfig/legendary/installed.json",
            ),
        );
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
         WHERE gpt.type = 'originalTitle'",
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
        if !install_path.is_absolute() {
            continue;
        }
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
    databases.insert(PathBuf::from(
        "/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db",
    ));
    databases
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
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
    let mut statement =
        match connection.prepare("SELECT Id, ProductTitle, InstallDirectory FROM DbSet") {
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
        if is_existing_absolute_directory(&install_path)
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
        if trimmed
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("HKEY_"))
        {
            // Recursive registry output can contain unrelated vendors between
            // EA keys. Clear the current ID so their Install Dir values cannot
            // inherit the previous EA product identifier.
            current_id = prefixes
                .iter()
                .find_map(|prefix| strip_prefix_ignore_ascii_case(trimmed, prefix))
                .map(|id| id.trim().to_owned());
            continue;
        }
        let Some((name, value)) = trimmed
            .split_once("REG_SZ")
            .or_else(|| trimmed.split_once("REG_EXPAND_SZ"))
        else {
            continue;
        };
        if !["Install Dir", "InstallDir"]
            .iter()
            .any(|candidate| name.trim().eq_ignore_ascii_case(candidate))
        {
            continue;
        }
        let expanded_path = expand_windows_environment_variables(value.trim().trim_matches('"'));
        let install_path = PathBuf::from(expanded_path);
        if !install_path.is_dir() || !install_path.join("__Installer/installerdata.xml").is_file() {
            continue;
        }
        let Some(metadata) = read_bounded_text(&install_path.join("__Installer/installerdata.xml"))
        else {
            continue;
        };
        let name = xml_element_text(&metadata, "gameTitle")
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                install_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned)
            });
        let Some(name) = name.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
        games.extend(parse_ea_registry_output(&String::from_utf8_lossy(
            &output.stdout,
        )));
    }
    let mut seen_paths = BTreeSet::new();
    games.retain(|game| insert_unique_install_path(&mut seen_paths, &game.install_path));
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

/// Parses optional custom EA game-library roots from
/// AMETHYS_EA_LIBRARY_ROOTS (semicolon-separated on every platform).
#[cfg(any(windows, test))]
fn parse_ea_library_roots(value: &str) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    for entry in value.split(';') {
        let entry = entry.trim().trim_matches('"');
        if entry.is_empty() {
            continue;
        }
        let path = PathBuf::from(entry);
        if is_absolute_metadata_path(&path) {
            roots.insert(path);
        }
    }
    roots.into_iter().collect()
}

/// Returns user-configured EA library roots without probing arbitrary drives.
/// This supports libraries outside EA's default Program Files locations.
#[cfg(windows)]
fn configured_ea_library_roots() -> Vec<PathBuf> {
    std::env::var("AMETHYS_EA_LIBRARY_ROOTS")
        .map(|value| parse_ea_library_roots(&value))
        .unwrap_or_default()
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
            if !is_existing_absolute_directory(&install_path) {
                continue;
            }
            let metadata = install_path.join("__Installer/installerdata.xml");
            let Some(contents) = read_bounded_text(&metadata) else {
                continue;
            };
            let name = xml_element_text(&contents, "gameTitle")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    install_path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_owned()
                });
            if name.trim().is_empty() {
                continue;
            }
            let platform_id =
                xml_element_text(&contents, "contentID").filter(|value| !value.trim().is_empty());
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

fn xml_element_text(contents: &str, element: &str) -> Option<String> {
    // EA metadata may contain tag-looking text in comments or CDATA. Empty
    // self-closing elements are skipped so a later populated element remains
    // discoverable, without pairing it with the wrong closing tag.
    let mut search_from = 0;
    let (_open_start, open_end) = loop {
        let open_start = find_xml_tag(contents, element, false, search_from)?;
        let open_end = find_xml_tag_end(contents, open_start)?;
        let opening_tag = contents.get(open_start..open_end)?.trim_end();
        if !opening_tag.strip_suffix('>')?.trim_end().ends_with('/') {
            break (open_start, open_end);
        }
        search_from = open_end;
    };
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
    let tag = if closing {
        format!("</{element}")
    } else {
        format!("<{element}")
    };

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
                let numeric = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|value| u32::from_str_radix(value, 16).ok())
                    .or_else(|| {
                        entity
                            .strip_prefix('#')
                            .and_then(|value| value.parse::<u32>().ok())
                    });
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
            PathBuf::from(local_app_data).join("Amazon Games/Data/Games/Sql/GameInstallInfo.sqlite")
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
        if trimmed
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("HKEY_"))
        {
            // reg.exe /s emits unrelated subkeys as well. Never let an
            // InstallDir value under another key inherit the previous game ID.
            current_id = install_key_prefixes
                .iter()
                .find_map(|prefix| strip_prefix_ignore_ascii_case(trimmed, prefix))
                .map(|id| id.trim().to_owned());
            continue;
        }
        if let Some((_, value)) = trimmed
            .split_once("REG_SZ")
            .or_else(|| trimmed.split_once("REG_EXPAND_SZ"))
            .filter(|(name, _)| name.trim().eq_ignore_ascii_case("InstallDir"))
        {
            let expanded_path =
                expand_windows_environment_variables(value.trim().trim_matches('"'));
            let install_path = PathBuf::from(expanded_path);
            if !is_existing_absolute_directory(&install_path) {
                continue;
            }
            let Some(name) = install_path
                .file_name()
                .and_then(|value| value.to_str())
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
        games.extend(parse_ubisoft_registry_output(&String::from_utf8_lossy(
            &output.stdout,
        )));
    }
    let mut seen_paths = BTreeSet::new();
    games.retain(|game| insert_unique_install_path(&mut seen_paths, &game.install_path));
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
    games
}

/// Finds game folders inside known libraries for launchers without a stable,
/// documented local manifest format. Only immediate child folders are considered.
pub fn scan_game_folders(platform: &'static str, roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut games = Vec::new();
    let mut seen_paths = BTreeSet::new();
    // Directory enumeration and caller-provided root order are unspecified.
    // Sort both before de-duplication so aliases resolve consistently.
    let mut roots = roots.to_vec();
    roots.sort();
    for root in &roots {
        if !is_existing_absolute_directory(root) {
            continue;
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path
                .file_name()
                .and_then(|value| value.to_str())
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
    // The file can be replaced or grow after the metadata check above.
    // Bound the actual read as well, so a concurrent change cannot trigger an
    // unbounded allocation from launcher-controlled metadata.
    let Ok(file) = fs::File::open(database) else {
        return Vec::new();
    };
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take(MAX_DATABASE_SIZE + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_DATABASE_SIZE
    {
        return Vec::new();
    }

    let mut installs = protobuf_length_delimited_fields(&bytes, 1).unwrap_or_default();
    // Some Agent versions store a single ProductInstall message directly.
    if installs.is_empty()
        && protobuf_length_delimited_fields(&bytes, 2).is_some_and(|fields| !fields.is_empty())
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
        else {
            continue;
        };
        let Some(base_state) = protobuf_length_delimited_fields(cached_state, 1)
            .and_then(|fields| fields.into_iter().next())
        else {
            continue;
        };
        if protobuf_varint_field(base_state, 1) != Some(1) {
            continue;
        }
        let Ok(path_text) = std::str::from_utf8(path_bytes) else {
            continue;
        };
        let path_text = path_text.trim_matches(char::from(0)).trim();
        if path_text.is_empty() {
            continue;
        }
        let install_path = PathBuf::from(path_text);
        if !is_existing_absolute_directory(&install_path) {
            continue;
        }
        if !insert_unique_install_path(&mut seen_paths, &install_path) {
            continue;
        }
        let name = battlenet_product_name(&product_key)
            .map(str::to_owned)
            .or_else(|| {
                install_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            });
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
    games.sort_by_key(|game| {
        (
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
        std::env::var_os("ProgramData")
            .map(|program_data| PathBuf::from(program_data).join("Battle.net/Agent/product.db"))
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
        if field == 0 || field > 0x1fff_ffff {
            return None;
        }
        match wire {
            0 => {
                let (value, len) = protobuf_varint(&bytes[offset..])?;
                offset = offset.checked_add(len)?;
                if field == wanted {
                    found = Some(value);
                }
            }
            1 => offset = offset.checked_add(8)?,
            2 => {
                let (length, len) = protobuf_varint(&bytes[offset..])?;
                offset = offset.checked_add(len)?;
                offset = offset.checked_add(usize::try_from(length).ok()?)?;
            }
            5 => offset = offset.checked_add(4)?,
            _ => return None,
        }
        if offset > bytes.len() {
            return None;
        }
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

/// Scans Riot Client installation metadata for installed Riot games.
pub fn scan_riot_client_installs(installs_json: &Path) -> Vec<DetectedGame> {
    let Some(contents) = read_bounded_text(installs_json) else {
        return Vec::new();
    };
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
    let Ok(document) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };

    fn collect_paths(value: &serde_json::Value, output: &mut Vec<PathBuf>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if matches!(
                        key.to_ascii_lowercase().as_str(),
                        "product_install_root"
                            | "install_path"
                            | "install_dir"
                            | "install_location"
                    ) {
                        if let Some(path) = value
                            .as_str()
                            .map(str::trim)
                            .filter(|path| !path.is_empty())
                        {
                            output.push(PathBuf::from(path));
                        }
                    }
                    collect_paths(value, output);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect_paths(value, output);
                }
            }
            _ => {}
        }
    }

    let mut candidates = Vec::new();
    collect_paths(&document, &mut candidates);
    candidates.sort();
    candidates.dedup();

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for path in candidates {
        if !is_existing_absolute_directory(&path) {
            continue;
        }
        let normalized = path.to_string_lossy().to_ascii_lowercase();
        let (name, platform_id) = if normalized.contains("valorant") {
            ("VALORANT", "valorant")
        } else if normalized.contains("league of legends")
            || path.join("LeagueClient.exe").is_file()
        {
            ("League of Legends", "league_of_legends")
        } else {
            // Riot Client and Vanguard folders are components, not games.
            continue;
        };
        if !seen.insert(install_path_key(&path)) {
            continue;
        }
        games.push(DetectedGame {
            platform_id: Some(platform_id.to_owned()),
            name: name.to_owned(),
            install_path: path,
            platform: "Riot Games",
        });
    }
    games.sort_by_key(|game| (game.name.to_lowercase(), game.install_path.clone()));
    games
}

/// Parses the scalar subset used by Riot's product_settings.yaml paths.
///
/// Double-quoted YAML strings decode common escapes while preserving unknown ones; single-quoted YAML
/// strings use doubled apostrophes. Plain scalars may carry a trailing comment.
fn parse_yaml_scalar(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.starts_with('"') {
        let mut escaped = false;
        let closing = value
            .char_indices()
            .skip(1)
            .find_map(|(index, character)| {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    return Some(index);
                }
                None
            })?;
        let trailing = value[closing + 1..].trim();
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return None;
        }
        let inner = &value[1..closing];
        let mut output = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(character) = chars.next() {
            if character != '\\' {
                if character == '"' {
                    return None;
                }
                output.push(character);
                continue;
            }
            match chars.next() {
                // YAML escapes recognized here; preserve unknown backslash
                // sequences because Riot metadata commonly contains raw
                // Windows paths such as C:\\Riot Games\\VALORANT.
                Some('\\') => output.push('\\'),
                Some('"') => output.push('"'),
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => return None,
            }
        }
        return Some(output);
    }
    if let Some(value) = value.strip_prefix('\'') {
        let mut inner = String::new();
        let mut chars = value.chars().peekable();
        let mut closed = false;
        while let Some(character) = chars.next() {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    inner.push('\'');
                } else {
                    closed = true;
                    break;
                }
            } else {
                inner.push(character);
            }
        }
        let trailing: String = chars.collect();
        let trailing = trailing.trim();
        if !closed || (!trailing.is_empty() && !trailing.starts_with('#')) {
            return None;
        }
        return Some(inner);
    }
    let plain = value
        .split_once(" #")
        .map_or(value, |(value, _)| value)
        .trim();
    (!plain.is_empty()).then(|| plain.to_owned())
}

/// Reads Riot product_settings.yaml files for known installed Riot titles.
pub fn scan_riot_product_settings(metadata_root: &Path) -> Vec<DetectedGame> {
    const PRODUCTS: [(&str, &str, &str); 4] = [
        ("valorant.live", "VALORANT", "valorant"),
        (
            "league_of_legends.live",
            "League of Legends",
            "league_of_legends",
        ),
        ("lor.live", "Legends of Runeterra", "lor"),
        ("2xko.live", "2XKO", "2xko"),
    ];
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();

    for (directory, name, product_id) in PRODUCTS {
        let file = metadata_root
            .join(directory)
            .join(format!("{directory}.product_settings.yaml"));
        let Some(contents) = read_bounded_text(&file) else {
            continue;
        };
        // YAML files may be saved with a UTF-8 BOM; it belongs to the
        // encoding marker, not to the first mapping key.
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let install_root = contents.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if !key.trim().eq_ignore_ascii_case("product_install_root") {
                return None;
            }
            let value = parse_yaml_scalar(value)?;
            (!value.is_empty()).then(|| PathBuf::from(value))
        });
        let Some(install_path) = install_root else {
            continue;
        };
        if !is_existing_absolute_directory(&install_path) {
            continue;
        }
        if !seen.insert(install_path_key(&install_path)) {
            continue;
        }
        games.push(DetectedGame {
            platform_id: Some(product_id.to_owned()),
            name: name.to_owned(),
            install_path,
            platform: "Riot Games",
        });
    }

    games.sort_by_key(|game| (game.name.to_lowercase(), game.install_path.clone()));
    games
}

/// Returns the standard machine-wide Riot Client installation metadata file.
#[cfg(windows)]
pub fn riot_client_installs_file() -> Option<PathBuf> {
    let program_data = std::env::var_os("ProgramData").map(PathBuf::from)?;
    let path = program_data.join("Riot Games/RiotClientInstalls.json");
    path.is_file().then_some(path)
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
        let mut ea_roots = configured_ea_library_roots();
        if let Some(program_files) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            ea_roots.push(program_files.join("EA Games"));
        }
        // EA app may store games in either Program Files tree. Every candidate
        // still needs EA's installerdata.xml and an existing absolute directory.
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            ea_roots.push(PathBuf::from(program_files_x86).join("EA Games"));
        }
        games.extend(scan_ea_games(&ea_roots));
        // Registry records also cover custom EA game-library locations.

        // GOG is detected from Galaxy's installed-product database rather
        // than guessed from folders that may contain unrelated files.
        // Xbox is intentionally not inferred from XboxGames folders:
        // that directory alone does not prove a package is installed.
        // Add Xbox discovery only when package metadata can identify games
        // reliably without bypassing Windows package permissions.
        // Ubisoft is detected from its registry install records. A generic
        // folder scan could report manually-created or leftover directories.
        games.extend(discover_ubisoft_games());
        if let Some(installs_file) = riot_client_installs_file() {
            games.extend(scan_riot_client_installs(&installs_file));
        }
        if let Some(program_data) = std::env::var_os("ProgramData").map(PathBuf::from) {
            games.extend(scan_riot_product_settings(
                &program_data.join("Riot Games/Metadata"),
            ));
        }
        if let Some(database) = battlenet_product_database() {
            games.extend(scan_battlenet_product_db(&database));
        }
        games.extend(discover_xbox_games());
    }

    // Do not infer installed games from generic `GOG Games` directories.
    // GOG Galaxy metadata is the source of truth when its database is available.

    games.sort_by_key(|game| {
        (
            game.platform,
            game.name.to_lowercase(),
            game.install_path.clone(),
            game.platform_id.clone(),
        )
    });
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
/// Matches Windows registry key prefixes without relying on their displayed casing.
fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = value.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

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
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn find_vdf_value(contents: &str, wanted_key: &str) -> Option<String> {
    // Real Steam app manifests place their fields directly inside AppState.
    // Restrict lookups to that object so nested UserConfig/SharedConfig values
    // cannot shadow the actual game name or install directory. Keep accepting
    // the flat form used by older exports and small standalone fixtures.
    let has_app_state = contents.lines().any(|line| {
        let line = line.trim_start();
        if line.starts_with("//") {
            return false;
        }
        line.split('"')
            .nth(1)
            .is_some_and(|key| key.eq_ignore_ascii_case("AppState"))
    });
    let target_depth = usize::from(has_app_state);
    let mut depth = 0usize;

    for line in contents.lines() {
        if depth == target_depth {
            if let Some((key, value)) = quoted_pair(line) {
                if key.eq_ignore_ascii_case(wanted_key) {
                    return Some(unescape_vdf(&value));
                }
            }
        }
        depth = vdf_update_depth(depth, line);
    }
    None
}

fn quoted_pair(line: &str) -> Option<(String, String)> {
    // Valve KeyValues uses // for line comments. Do not treat quoted examples
    // inside comments as real metadata (for example, an old appid/path).
    if line.trim_start().starts_with("//") {
        return None;
    }

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
    fn linux_steam_roots_cover_native_flatpak_and_custom_xdg_layouts() {
        let home = std::path::Path::new("/home/player");
        let xdg = std::path::Path::new("/mnt/data/player");
        let roots = super::unix_steam_roots(home, Some(xdg), "linux");
        assert!(roots.contains(&home.join(".steam/steam")));
        assert!(roots.contains(&home.join(".local/share/Steam")));
        assert!(roots.contains(&home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")));
        assert!(roots.contains(&xdg.join("Steam")));
    }

    #[test]
    fn macos_steam_root_uses_application_support() {
        let home = std::path::Path::new("/Users/player");
        let roots = super::unix_steam_roots(home, None, "macos");
        assert_eq!(roots, vec![home.join("Library/Application Support/Steam")]);
    }

    #[test]
    fn unix_steam_roots_reject_relative_home_and_xdg_paths() {
        assert!(
            super::unix_steam_roots(
                std::path::Path::new("relative/home"),
                Some(std::path::Path::new("relative/data")),
                "linux",
            )
            .is_empty()
        );
        let roots = super::unix_steam_roots(
            std::path::Path::new("/home/player"),
            Some(std::path::Path::new("relative/data")),
            "linux",
        );
        assert!(!roots.contains(&std::path::PathBuf::from("relative/data/Steam")));
    }

    #[test]
    fn bounded_metadata_reader_accepts_small_files_and_rejects_oversized_files() {
        let root = std::env::temp_dir().join(format!(
            "amethys-metadata-limit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create test directory");
        let small = root.join("small.json");
        let oversized = root.join("oversized.json");
        std::fs::write(&small, br#"{"ok":true}"#).expect("write small metadata");
        std::fs::write(
            &oversized,
            vec![b'x'; (super::MAX_METADATA_FILE_SIZE + 1) as usize],
        )
        .expect("write oversized metadata");
        assert_eq!(
            super::read_bounded_text(&small).as_deref(),
            Some(r#"{"ok":true}"#)
        );
        assert!(super::read_bounded_text(&oversized).is_none());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_relative_install_paths_even_when_they_could_resolve_from_current_directory() {
        assert!(!super::is_existing_absolute_directory(Path::new(
            "relative/game"
        )));
        let absolute = std::env::temp_dir();
        assert!(absolute.is_absolute());
        assert!(super::is_existing_absolute_directory(&absolute));
    }

    #[test]
    fn ignores_quoted_pairs_inside_vdf_line_comments() {
        assert_eq!(
            super::parse_app_manifest(
                r#"
                // "appid" "999"
                "AppState"
                {
                    // "name" "Commented Game"
                    "appid" "123"
                    "name" "Real Game"
                    "installdir" "RealGame"
                }
                "AppState"
                "appid" "456"
                "name" "Another Game"
                "installdir" "AnotherGame"
                "#
            ),
            Some((
                "123".to_owned(),
                "Real Game".to_owned(),
                "RealGame".to_owned()
            ))
        );

        let libraries = super::parse_library_folders(
            r#"
            "libraryfolders"
            {
                // "1" "/ignored/library"
                "1" "/real/library"
            }
            "#,
            Path::new("/steam"),
        );
        assert!(libraries.contains(&PathBuf::from("/real/library")));
        assert!(!libraries.contains(&PathBuf::from("/ignored/library")));
    }

    #[test]
    fn ignores_relative_steam_library_paths_and_roots() {
        let libraries = super::parse_library_folders(
            r#"
            "libraryfolders"
            {
                "1" "relative/library"
                "2" "/absolute/library"
            }
            "#,
            Path::new("relative/steam"),
        );

        assert_eq!(libraries, vec![PathBuf::from("/absolute/library")]);
    }

    #[test]
    fn ignores_braces_inside_vdf_comments_when_reading_library_depth() {
        let libraries = super::parse_library_folders(
            r#"
            "libraryfolders"
            {
                // { "9" "/commented/library" }
                "1"
                {
                    "path" "/real/library"
                    // } extra closing brace must not end the object
                }
            }
            "#,
            Path::new("/steam"),
        );

        assert!(libraries.contains(&PathBuf::from("/real/library")));
        assert!(!libraries.contains(&PathBuf::from("/commented/library")));
    }

    #[test]
    fn unescapes_vdf_sequences_without_consuming_adjacent_backslashes() {
        assert_eq!(
            super::unescape_vdf(r#"C:\\Games\\Example"#),
            r#"C:\Games\Example"#
        );
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
        assert_eq!(
            super::decode_xml_entities("Bad &#xZZ; ref"),
            "Bad &#xZZ; ref"
        );
    }

    use super::{
        epic_launch_executable, parse_app_manifest, parse_library_folders, scan_epic_manifests,
        scan_game_folders, scan_steam_libraries,
    };
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
    };

    #[test]
    fn epic_manifest_roots_cover_windows_and_macos_without_host_environment() {
        assert_eq!(
            super::epic_manifest_roots_for("windows", None, Some(Path::new("C:/ProgramData")),),
            vec![PathBuf::from(
                "C:/ProgramData/Epic/EpicGamesLauncher/Data/Manifests"
            )]
        );
        assert_eq!(
            super::epic_manifest_roots_for("macos", Some(Path::new("/Users/test-user")), None,),
            vec![PathBuf::from(
                "/Users/test-user/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests"
            )]
        );
    }

    #[test]
    fn epic_manifest_roots_reject_relative_environment_paths_and_skip_linux_launcher() {
        assert!(
            super::epic_manifest_roots_for(
                "windows",
                None,
                Some(Path::new("relative/ProgramData")),
            )
            .is_empty()
        );
        assert!(
            super::epic_manifest_roots_for("macos", Some(Path::new("relative/home")), None,)
                .is_empty()
        );
        assert!(super::epic_manifest_roots_for("linux", None, None).is_empty());
    }

    #[test]
    fn relative_xdg_config_home_is_ignored_in_favor_of_absolute_home_fallback() {
        let home = Path::new("/home/test-user");
        let config =
            super::linux_heroic_config_home(Some(Path::new("relative/config")), Some(home));
        assert_eq!(config, Some(home.join(".config")));
    }

    #[test]
    fn absolute_xdg_config_home_takes_precedence_over_home_fallback() {
        let config = super::linux_heroic_config_home(
            Some(Path::new("/custom/config")),
            Some(Path::new("/home/test-user")),
        );
        assert_eq!(config, Some(PathBuf::from("/custom/config")));
    }

    #[test]
    fn missing_absolute_config_locations_do_not_fall_back_to_current_directory() {
        assert_eq!(
            super::linux_heroic_config_home(Some(Path::new("relative")), None),
            None
        );
    }

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
            &mut seen,
            "Steam",
            Path::new("/games/shared")
        ));
        assert!(super::insert_unique_platform_install_path(
            &mut seen,
            "Steam",
            Path::new("/games/other")
        ));
        assert!(!super::insert_unique_platform_install_path(
            &mut seen,
            "Steam",
            Path::new("/games/shared")
        ));
        assert!(super::insert_unique_platform_install_path(
            &mut seen,
            "Epic Games",
            Path::new("/games/shared")
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
    fn parses_steam_manifest_keys_case_insensitively() {
        let manifest = r#"
            "AppID" "570"
            "Name" "Dota 2"
            "InstallDir" "dota 2"
        "#;
        assert_eq!(
            super::parse_app_manifest(manifest),
            Some(("570".to_owned(), "Dota 2".to_owned(), "dota 2".to_owned())),
        );
    }

    #[test]
    fn steam_manifest_fields_are_read_only_from_the_app_state_object() {
        let manifest = r#"
            "AppState"
            {
                "appid" "570"
                "name" "Dota 2"
                "installdir" "dota2"
                "UserConfig"
                {
                    "name" "Incorrect Nested Name"
                    "installdir" "incorrect-folder"
                }
            }
        "#;

        assert_eq!(
            super::parse_app_manifest(manifest),
            Some(("570".to_owned(), "Dota 2".to_owned(), "dota2".to_owned()))
        );
    }

    #[test]
    fn quoted_vdf_pairs_ignore_escaped_quotes_inside_values() {
        assert_eq!(
            super::quoted_pair(r#""path" "C:\\Games\\A \"Quoted\" Game""#),
            Some((
                "path".to_owned(),
                r#"C:\\Games\\A \"Quoted\" Game"#.to_owned()
            )),
        );
    }

    #[test]
    fn parses_registered_steam_root_without_matching_unrelated_registry_values() {
        let output = r#"
HKEY_CURRENT_USER\Software\Valve\Steam
    InstallPath    REG_SZ    D:\Games\Steam
    SteamPath      REG_SZ    "E:\Custom Steam"
"#;
        assert_eq!(
            super::parse_steam_registry_output(output),
            Some(std::path::PathBuf::from(r"E:\Custom Steam"))
        );
        assert_eq!(
            super::parse_steam_registry_output(
                "HKEY_CURRENT_USER\\Software\\Valve\\Steam\n    InstallPath REG_SZ D:\\Games\\Steam"
            ),
            None
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
    fn parses_steam_library_path_keys_case_insensitively() {
        let contents = r#"
            "libraryfolders"
            {
                "0"
                {
                    "Path" "D:\\SteamLibrary"
                }
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("C:/Steam"));
        assert!(paths.contains(&Path::new(r"D:\SteamLibrary").to_path_buf()));
    }

    #[test]
    fn trims_steam_library_paths_and_ignores_empty_values() {
        let contents = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path" "   "
                }
                "1"
                {
                    "path" "  D:\\SteamLibrary  "
                }
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("C:/Steam"));
        assert_eq!(
            paths,
            vec![
                Path::new("C:/Steam").to_path_buf(),
                Path::new(r"D:\SteamLibrary").to_path_buf(),
            ]
        );
    }

    #[test]
    fn ignores_numeric_paths_outside_the_libraryfolders_object() {
        let contents = r#"
            "other_metadata"
            {
                "1" "/not/a/steam/library"
            }
            "libraryfolders"
            {
                "0" "/real/steam/library"
            }
        "#;
        let paths = parse_library_folders(contents, Path::new("/default/steam"));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/default/steam"),
                PathBuf::from("/real/steam/library"),
            ]
        );
        assert!(!paths.contains(&PathBuf::from("/not/a/steam/library")));
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
            assert_eq!(
                parse_app_manifest(&contents),
                None,
                "accepted {install_dir}"
            );
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
    fn steam_duplicate_app_ids_resolve_independently_of_library_input_order() {
        let root = std::env::temp_dir().join(format!(
            "amethys-steam-library-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_library = root.join("a-library");
        let second_library = root.join("z-library");
        for (library, folder, title) in [
            (&first_library, "First Copy", "First Copy"),
            (&second_library, "Second Copy", "Second Copy"),
        ] {
            let steamapps = library.join("steamapps");
            std::fs::create_dir_all(steamapps.join("common").join(folder))
                .expect("create Steam install");
            std::fs::write(
                steamapps.join("appmanifest_570.acf"),
                format!(
                    r#""appid" "570"
"name" "{title}"
"installdir" "{folder}""#
                ),
            )
            .expect("write Steam manifest");
        }

        let forward = super::scan_steam_libraries(&[second_library.clone(), first_library.clone()]);
        let reverse = super::scan_steam_libraries(&[first_library.clone(), second_library.clone()]);
        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].name, "First Copy");
        std::fs::remove_dir_all(root).expect("cleanup");
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let manifests = root.join("Manifests");
        let installed = root.join("Installed").join("Example Game");
        let whitespace_install = root.join("Installed").join("Whitespace Game");
        let duplicate_install = root.join("Installed").join("Duplicate Example");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&installed).expect("create game folder");
        std::fs::create_dir_all(&whitespace_install).expect("create whitespace game folder");
        std::fs::create_dir_all(&duplicate_install).expect("create duplicate game folder");
        std::fs::write(
            manifests.join("installed.item"),
            format!(
                r#"{{"AppName":"ExampleApp","DisplayName":"Example Game","InstallLocation":{}}}"#,
                serde_json::to_string(&installed.to_string_lossy()).expect("encode path")
            ),
        )
        .expect("write manifest");
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
        )
        .expect("write manifest with surrounding whitespace");

        let games = scan_epic_manifests(&manifests);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].platform, "Epic Games");
        assert_eq!(
            games
                .iter()
                .filter(|game| game.platform_id.as_deref() == Some("ExampleApp"))
                .count(),
            1
        );
        let whitespace_game = games
            .iter()
            .find(|game| game.install_path == whitespace_install)
            .unwrap();
        assert_eq!(whitespace_game.name, "Example Game");
        assert_eq!(whitespace_game.platform_id, None);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn resolves_only_declared_epic_executables_inside_the_install_directory() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-launch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let manifests = root.join("Manifests");
        let install = root.join("Installed").join("Example");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(install.join("Binaries")).expect("create binaries");
        std::fs::write(install.join("Binaries").join("Game.exe"), b"test")
            .expect("create executable");
        std::fs::write(
            manifests.join("game.item"),
            format!(
                r#"{{"AppName":"ExampleApp","InstallLocation":{},"LaunchExecutable":"Binaries/Game.exe"}}"#,
                serde_json::to_string(&install.to_string_lossy()).expect("encode path")
            ),
        )
        .expect("write manifest");

        assert_eq!(
            epic_launch_executable(&manifests, "ExampleApp", &install),
            Some(PathBuf::from("Binaries/Game.exe"))
        );
        assert_eq!(
            epic_launch_executable(&manifests, "OtherApp", &install),
            None
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_epic_executables_that_escape_or_do_not_exist() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-launch-reject-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let manifests = root.join("Manifests");
        let install = root.join("Installed").join("Example");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&install).expect("create install");
        let outside = root.join("Installed").join("outside.exe");
        std::fs::write(&outside, b"outside").expect("create outside executable");
        let manifest_path = manifests.join("game.item");

        let write_manifest = |executable: &str| {
            let manifest = serde_json::json!({
                "AppName": "ExampleApp",
                "InstallLocation": install.to_string_lossy(),
                "LaunchExecutable": executable,
            });
            std::fs::write(&manifest_path, manifest.to_string()).expect("write manifest");
        };

        write_manifest("../outside.exe");
        assert_eq!(
            epic_launch_executable(&manifests, "ExampleApp", &install),
            None,
            "parent-directory traversal must be rejected"
        );

        write_manifest(&outside.to_string_lossy());
        assert_eq!(
            epic_launch_executable(&manifests, "ExampleApp", &install),
            None,
            "absolute paths outside the installation must be rejected"
        );

        write_manifest("Missing/Game.exe");
        assert_eq!(
            epic_launch_executable(&manifests, "ExampleApp", &install),
            None,
            "missing executable files must not be inferred"
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn reads_epic_manifests_with_uppercase_item_extension() {
        let root = std::env::temp_dir().join(format!(
            "amethys-epic-extension-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let manifests = root.join("Manifests");
        let install = root.join("Installed Game");
        std::fs::create_dir_all(&manifests).expect("create manifests");
        std::fs::create_dir_all(&install).expect("create install");
        let json = format!(
            r#"{{"AppName":"UppercaseExtension","DisplayName":"Uppercase Extension","InstallLocation":{}}}"#,
            serde_json::to_string(&install.to_string_lossy()).expect("encode path")
        );
        std::fs::write(manifests.join("uppercase.ITEM"), json).expect("write manifest");

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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let install = root.join("Heroes 3");
        std::fs::create_dir_all(&install).expect("create installed game");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);",
            )
            .expect("create GOG metadata tables");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                rusqlite::params!["1234567890", install.to_string_lossy()],
            )
            .expect("insert installed GOG product");
        connection
            .execute("INSERT INTO GamePieceTypes VALUES (1, 'originalTitle')", [])
            .expect("insert title type");
        connection
            .execute(
                "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                rusqlite::params![
                    "gog_1234567890",
                    r#"{"title":"Heroes of Might and Magic 3"}"#
                ],
            )
            .expect("insert GOG title");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                rusqlite::params!["duplicate-product", install.to_string_lossy()],
            )
            .expect("insert duplicate install path");
        connection
            .execute(
                "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                rusqlite::params!["gog_duplicate-product", r#"{"title":"Duplicate entry"}"#],
            )
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
        use rusqlite::{Connection, params};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_second_install = root.join("Second Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_second_install).expect("create second install");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);
             INSERT INTO GamePieceTypes VALUES (1, 'originalTitle');",
            )
            .expect("create GOG metadata tables");
        for (id, path, title) in [
            (
                "first",
                first_install.to_string_lossy().into_owned(),
                "First Game",
            ),
            (
                "second",
                first_install.to_string_lossy().into_owned(),
                "Duplicate Path",
            ),
            (
                "second",
                valid_second_install.to_string_lossy().into_owned(),
                "Second Game",
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                    params![id, path],
                )
                .expect("insert GOG product");
            connection
                .execute(
                    "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                    params![
                        format!("gog_{id}"),
                        serde_json::json!({"title": title}).to_string()
                    ],
                )
                .expect("insert GOG title");
        }
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 2);
        assert!(
            games
                .iter()
                .any(|game| game.platform_id.as_deref() == Some("second")
                    && game.install_path == valid_second_install)
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn skips_invalid_gog_titles_and_stale_install_paths() {
        use rusqlite::{Connection, params};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let installed = root.join("Valid Game");
        std::fs::create_dir_all(&installed).expect("create installed game");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create fixture database");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts (productId TEXT, installationPath TEXT);
             CREATE TABLE GamePieces (releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
             CREATE TABLE GamePieceTypes (id INTEGER, type TEXT);
             INSERT INTO GamePieceTypes VALUES (1, 'originalTitle');",
            )
            .expect("create GOG metadata tables");

        let rows = [
            (
                "valid",
                installed.to_string_lossy().into_owned(),
                r#"{"title":"Valid Game"}"#,
            ),
            (
                "bad-json",
                installed.to_string_lossy().into_owned(),
                "{not-json",
            ),
            (
                "empty-title",
                installed.to_string_lossy().into_owned(),
                r#"{"title":"  "}"#,
            ),
            (
                "stale-path",
                root.join("Removed Game").to_string_lossy().into_owned(),
                r#"{"title":"Removed Game"}"#,
            ),
        ];
        for (id, location, title) in rows {
            connection
                .execute(
                    "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                    params![id, location],
                )
                .expect("insert GOG product");
            connection
                .execute(
                    "INSERT INTO GamePieces VALUES (?1, 1, ?2)",
                    params![format!("gog_{id}"), title],
                )
                .expect("insert GOG title");
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let database = root.join("unknown.db");
        Connection::open(&database)
            .expect("create empty database")
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("Games").join("Another EA Game");
        std::fs::create_dir_all(install.join("__Installer")).expect("create EA install metadata");
        std::fs::write(
            install.join("__Installer/installerdata.xml"),
            r#"<DiPManifest><gameTitle>Another EA Game</gameTitle></DiPManifest>"#,
        )
        .expect("write EA metadata");
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_install = root.join("Valid Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_install).expect("create valid install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\first\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\target\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\target\n    InstallDir    REG_SZ    {}\n",
            first_install.display(),
            first_install.display(),
            valid_install.display(),
        );

        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 2);
        assert!(games.iter().any(|game| {
            game.platform_id.as_deref() == Some("target") && game.install_path == valid_install
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
    fn ignores_self_closing_ea_metadata_elements() {
        let metadata = "<gameTitle/><gameTitle>Actual Game</gameTitle>";
        assert_eq!(
            super::xml_element_text(metadata, "gameTitle").as_deref(),
            Some("Actual Game")
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
        assert_eq!(
            super::xml_element_text(metadata, "gameTitle").as_deref(),
            Some("Real & Valid Title")
        );
        assert_eq!(
            super::xml_element_text(metadata, "contentID").as_deref(),
            Some("real-id")
        );
    }

    #[test]
    fn xml_entity_decoding_does_not_decode_escaped_entity_text_twice() {
        assert_eq!(
            super::xml_element_text(
                "<gameTitle>Rock &amp;lt; Roll &amp;amp; More</gameTitle>",
                "gameTitle"
            )
            .as_deref(),
            Some("Rock &lt; Roll &amp; More"),
        );
    }

    #[test]
    fn ignores_ea_folders_without_installer_metadata_and_falls_back_to_folder_name() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("Mass Effect");
        std::fs::create_dir_all(install.join("__Installer")).expect("create metadata folder");
        std::fs::write(
            install.join("__Installer/installerdata.xml"),
            "<DiPManifest/>",
        )
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let installed = root.join("Blue Fire");
        let not_installed = root.join("Not Installed");
        std::fs::create_dir_all(&installed).expect("create installed game");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create fixture database");
        connection
            .execute_batch(
                "CREATE TABLE DbSet (
                Id TEXT,
                InstallDirectory TEXT,
                ProductTitle TEXT
            );",
            )
            .expect("create Amazon metadata table");
        connection
            .execute(
                "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
                rusqlite::params!["amzn1.blue-fire", installed.to_string_lossy(), "Blue Fire"],
            )
            .expect("insert installed game");
        connection
            .execute(
                "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "amzn1.other",
                    not_installed.to_string_lossy(),
                    "Not Installed"
                ],
            )
            .expect("insert uninstalled game");
        connection
            .execute(
                "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle)
             VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "amzn1.blue-fire-alias",
                    installed.to_string_lossy(),
                    "Duplicate Blue Fire"
                ],
            )
            .expect("insert duplicate installation path");
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
        use rusqlite::{Connection, params};

        let root = std::env::temp_dir().join(format!(
            "amethys-gog-malformed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let installed = root.join("Valid GOG Game");
        std::fs::create_dir_all(&installed).expect("create install");
        let database = root.join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("create database");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts (productId, installationPath);
             CREATE TABLE GamePieces (releaseKey, gamePieceTypeId, value);
             CREATE TABLE GamePieceTypes (id, type);",
            )
            .expect("create GOG metadata tables");
        connection
            .execute(
                "INSERT INTO GamePieceTypes VALUES (?1, ?2)",
                params![1_i64, "originalTitle"],
            )
            .expect("insert title type");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                params![42_i64, installed.to_string_lossy()],
            )
            .expect("insert malformed product row");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts VALUES (?1, ?2)",
                params!["valid-product", installed.to_string_lossy()],
            )
            .expect("insert valid product row");
        connection
            .execute(
                "INSERT INTO GamePieces VALUES (?1, ?2, ?3)",
                params!["gog_42", 1_i64, r#"{"title":"Malformed Game"}"#],
            )
            .expect("insert malformed title metadata");
        connection
            .execute(
                "INSERT INTO GamePieces VALUES (?1, ?2, ?3)",
                params!["gog_valid-product", 1_i64, r#"{"title":"Valid GOG Game"}"#],
            )
            .expect("insert valid title metadata");
        drop(connection);

        let games = super::scan_gog_galaxy(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("valid-product"));
        assert_eq!(games[0].name, "Valid GOG Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn skips_malformed_amazon_rows_without_losing_valid_games() {
        use rusqlite::{Connection, params};

        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-malformed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let installed = root.join("Valid Game");
        std::fs::create_dir_all(&installed).expect("create install");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create database");
        connection
            .execute_batch("CREATE TABLE DbSet (Id, InstallDirectory, ProductTitle);")
            .expect("create metadata table");
        connection
            .execute(
                "INSERT INTO DbSet VALUES (?1, ?2, ?3)",
                params![42_i64, installed.to_string_lossy(), "Malformed ID"],
            )
            .expect("insert malformed ID row");
        connection
            .execute(
                "INSERT INTO DbSet VALUES (?1, ?2, ?3)",
                params!["amzn.valid", installed.to_string_lossy(), "Valid Game"],
            )
            .expect("insert valid row");
        drop(connection);

        let games = super::scan_amazon_games(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("amzn.valid"));
        assert_eq!(games[0].name, "Valid Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn keeps_amazon_product_id_available_after_rejecting_duplicate_path() {
        use rusqlite::{Connection, params};

        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-dedup-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("First Game");
        let valid_second_install = root.join("Second Game");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&valid_second_install).expect("create second install");
        let database = root.join("GameInstallInfo.sqlite");
        let connection = Connection::open(&database).expect("create fixture database");
        connection
            .execute_batch(
                "CREATE TABLE DbSet (Id TEXT, InstallDirectory TEXT, ProductTitle TEXT);",
            )
            .expect("create Amazon metadata table");
        for (id, path, title) in [
            (
                "amzn.first",
                first_install.to_string_lossy().into_owned(),
                "First Game",
            ),
            (
                "amzn.second",
                first_install.to_string_lossy().into_owned(),
                "Duplicate Path",
            ),
            (
                "amzn.second",
                valid_second_install.to_string_lossy().into_owned(),
                "Second Game",
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO DbSet (Id, InstallDirectory, ProductTitle) VALUES (?1, ?2, ?3)",
                    params![id, path, title],
                )
                .expect("insert Amazon product");
        }
        drop(connection);

        let games = super::scan_amazon_games(&database);
        assert_eq!(games.len(), 2);
        assert!(
            games
                .iter()
                .any(|game| game.platform_id.as_deref() == Some("amzn.second")
                    && game.install_path == valid_second_install)
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_unknown_or_missing_amazon_databases() {
        let root = std::env::temp_dir().join(format!(
            "amethys-amazon-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("Game");
        let duplicate_install = root.join("Game-copy");
        for install in [&first_install, &duplicate_install] {
            std::fs::create_dir_all(install.join("__Installer"))
                .expect("create installer metadata directory");
            std::fs::write(
                install.join("__Installer/installerdata.xml"),
                "<gameTitle>Same Game</gameTitle><contentID>EA-123</contentID>",
            )
            .expect("write EA installer metadata");
        }
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\First\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\EA Games\\Second\n    Install Dir    REG_SZ    {}\n",
            first_install.display(),
            duplicate_install.display()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            )
            .expect("write EA installer metadata");
        }
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\First\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\Second\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\Second\n    Install Dir    REG_SZ    {}\n",
            first_install.display(),
            first_install.display(),
            valid_second_install.display()
        );
        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 2);
        assert!(
            games
                .iter()
                .any(|game| game.platform_id.as_deref() == Some("Second")
                    && game.install_path == valid_second_install)
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn does_not_attribute_unrelated_registry_install_values_to_previous_ea_game() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-unrelated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let ea_install = root.join("EA Game");
        let unrelated_install = root.join("Other Vendor Game");
        std::fs::create_dir_all(ea_install.join("__Installer"))
            .expect("create EA installer metadata directory");
        std::fs::write(
            ea_install.join("__Installer/installerdata.xml"),
            "<gameTitle>Valid Game</gameTitle><contentID>EA-123</contentID>",
        )
        .expect("write EA installer metadata");
        std::fs::create_dir_all(&unrelated_install).expect("create unrelated install directory");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\EA Games\\EA-123\n    Install Dir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\OtherVendor\\Installs\\99\n    Install Dir    REG_SZ    {}\n",
            ea_install.display(),
            unrelated_install.display()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("Not Ubisoft");
        std::fs::create_dir_all(&install).expect("create unrelated install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\OtherVendor\\Installs\\99\n    InstallDir    REG_SZ    {}\n",
            install.display(),
            install.display()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("Game");
        let duplicate_install = root.join("Game-copy");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&duplicate_install).expect("create duplicate install");
        let output = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\44\n    InstallDir    REG_SZ    {}\n",
            first_install.display(),
            duplicate_install.display()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("WoW");
        std::fs::create_dir_all(&install).expect("create installed game");
        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8)
            .into_iter()
            .chain(encode_varint(1))
            .collect::<Vec<_>>();
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first_install = root.join("WoW");
        let second_install = root.join("WoW-copy");
        std::fs::create_dir_all(&first_install).expect("create first install");
        std::fs::create_dir_all(&second_install).expect("create second install");

        let make_product = |code: &[u8], install: &Path| {
            let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
            let base_state = encode_varint(8)
                .into_iter()
                .chain(encode_varint(1))
                .collect::<Vec<_>>();
            let cached_state = encode_bytes_field(1, &base_state);
            let mut product = encode_bytes_field(2, code);
            product.extend(encode_bytes_field(3, &settings));
            product.extend(encode_bytes_field(4, &cached_state));
            product
        };
        let mut database_bytes = encode_bytes_field(1, &make_product(b"wow", &first_install));
        database_bytes.extend(encode_bytes_field(
            1,
            &make_product(b"WOW", &second_install),
        ));
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("WoW");
        std::fs::create_dir_all(&install).expect("create install");

        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8)
            .into_iter()
            .chain(encode_varint(1))
            .collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"WOW");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database = root.join("product.db");
        std::fs::write(&database, encode_bytes_field(1, &product)).expect("write protobuf fixture");

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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("World of Warcraft");
        std::fs::create_dir_all(&install).expect("create leftover game directory");
        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8)
            .into_iter()
            .chain(encode_varint(0))
            .collect::<Vec<_>>();
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let database = root.join("product.db");
        let file = std::fs::File::create(&database).expect("create database");
        file.set_len(16 * 1024 * 1024 + 1).expect("extend database");
        assert!(super::scan_battlenet_product_db(&database).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn folder_scan_resolves_symlink_aliases_independently_of_root_order() {
        let root = std::env::temp_dir().join(format!(
            "amethys-folder-alias-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let actual_root = root.join("a-library");
        let alias_root = root.join("z-library");
        std::fs::create_dir_all(actual_root.join("Same Game")).expect("create install");
        std::fs::create_dir_all(&alias_root).expect("create alias root");
        std::os::unix::fs::symlink(actual_root.join("Same Game"), alias_root.join("Same Game"))
            .expect("create install alias");

        let forward = scan_game_folders("EA app", &[alias_root.clone(), actual_root.clone()]);
        let reverse = scan_game_folders("EA app", &[actual_root.clone(), alias_root.clone()]);
        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].install_path, actual_root.join("Same Game"));

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
        assert!(
            games
                .iter()
                .any(|game| game.name == "Another Installed Game")
        );
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
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
        std::fs::write(&database, format!("\u{feff}{}", contents))
            .expect("write BOM-prefixed Legendary metadata");

        let games = super::scan_heroic_legendary_installed(&database);
        assert_eq!(games.len(), 2);
        assert!(games.iter().all(|game| game.platform == "Heroic"));
        let hades = games
            .iter()
            .find(|game| game.platform_id.as_deref() == Some("HadesApp"))
            .unwrap();
        assert_eq!(hades.name, "Hades");
        assert_eq!(hades.install_path, installed);
        let celeste = games
            .iter()
            .find(|game| game.platform_id.as_deref() == Some("CelesteApp"))
            .unwrap();
        assert_eq!(celeste.name, "Celeste");
        assert_eq!(celeste.install_path, whitespace_install);
        assert!(super::scan_heroic_legendary_installed(&root.join("missing.json")).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn filters_steam_runtime_components_from_game_results() {
        assert!(super::is_steam_runtime_component(
            "228980",
            "Steamworks Common Redistributables"
        ));
        assert!(super::is_steam_runtime_component(
            "1070560",
            "Steam Linux Runtime"
        ));
        assert!(super::is_steam_runtime_component(
            "123",
            "Proton Experimental"
        ));
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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("CustomGame");
        std::fs::create_dir_all(&install).expect("create install");

        let settings = encode_bytes_field(1, install.to_string_lossy().as_bytes());
        let base_state = encode_varint(8)
            .into_iter()
            .chain(encode_varint(1))
            .collect::<Vec<_>>();
        let cached_state = encode_bytes_field(1, &base_state);
        let mut product = encode_bytes_field(2, b"future_product_code");
        product.extend(encode_bytes_field(3, &settings));
        product.extend(encode_bytes_field(4, &cached_state));
        let database = root.join("product.db");
        std::fs::write(&database, encode_bytes_field(1, &product)).expect("write protobuf fixture");

        let games = super::scan_battlenet_product_db(&database);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("future_product_code"));
        assert_eq!(games[0].name, "CustomGame");
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_ubisoft_registry_keys_case_insensitively() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ubisoft-registry-case-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("Ubisoft Game");
        std::fs::create_dir_all(&install).expect("create Ubisoft install");
        let output = format!(
            "hkey_local_machine\\software\\ubisoft\\launcher\\installs\\Game123\\n    installdir    REG_SZ    {}\\n",
            install.display()
        ).replace("\\n", "\n");
        let games = super::parse_ubisoft_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("Game123"));
        assert_eq!(games[0].name, "Ubisoft Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_configured_ea_library_roots_without_accepting_relative_paths() {
        let roots = super::parse_ea_library_roots(
            r#" A:\EA ; "D:\Game Libraries\EA" ; relative\EA ; ; A:\EA "#,
        );
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|path| path.to_string_lossy() == r"A:\EA"));
        assert!(
            roots
                .iter()
                .any(|path| path.to_string_lossy() == r"D:\Game Libraries\EA")
        );
    }

    #[test]
    fn parses_ea_registry_keys_case_insensitively() {
        let root = std::env::temp_dir().join(format!(
            "amethys-ea-registry-case-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let install = root.join("EA Game");
        std::fs::create_dir_all(install.join("__Installer")).expect("create EA metadata");
        std::fs::write(
            install.join("__Installer/installerdata.xml"),
            "<DiPManifest><gameTitle>EA Game</gameTitle><contentID>EA:case-123</contentID></DiPManifest>",
        ).expect("write EA metadata");
        let output = format!(
            "hkey_local_machine\\software\\ea games\\CaseGame\\n    install dir    REG_SZ    {}\\n",
            install.display()
        )
        .replace("\\n", "\n");
        let games = super::parse_ea_registry_output(&output);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_id.as_deref(), Some("EA:case-123"));
        assert_eq!(games[0].name, "EA Game");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn discovers_riot_games_from_product_install_metadata() {
        let root = std::env::temp_dir().join(format!(
            "amethys-riot-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let valorant = root.join("VALORANT");
        let league = root.join("League of Legends");
        std::fs::create_dir_all(&valorant).expect("create VALORANT install");
        std::fs::create_dir_all(&league).expect("create League install");
        let metadata = root.join("RiotClientInstalls.json");
        let document = serde_json::json!({
            "valorant": { "product_install_root": valorant.to_string_lossy() },
            "league": { "install_path": league.to_string_lossy() },
            "riot_client": { "install_path": root.join("Riot Client").to_string_lossy() }
        });
        std::fs::write(&metadata, document.to_string()).expect("write Riot metadata");

        let games = super::scan_riot_client_installs(&metadata);
        assert_eq!(games.len(), 2);
        assert!(
            games
                .iter()
                .any(|game| game.name == "VALORANT"
                    && game.platform_id.as_deref() == Some("valorant"))
        );
        assert!(games.iter().any(|game| game.name == "League of Legends"));
        assert!(games.iter().all(|game| game.platform == "Riot Games"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_riot_yaml_path_scalars_without_corrupting_windows_escapes() {
        // YAML-escaped backslashes are decoded to normal Windows separators.
        assert_eq!(
            super::parse_yaml_scalar(r#""C:\\Riot Games\\VALORANT""#),
            Some(r"C:\Riot Games\VALORANT".to_owned())
        );
        // Riot may also emit Windows paths with unescaped backslashes.
        assert_eq!(
            super::parse_yaml_scalar(r#""C:\Riot Games\VALORANT""#),
            Some(r"C:\Riot Games\VALORANT".to_owned())
        );
        assert_eq!(
            super::parse_yaml_scalar("'C:/Riot Games/Player''s VALORANT'"),
            Some("C:/Riot Games/Player's VALORANT".to_owned())
        );
        assert_eq!(
            super::parse_yaml_scalar("C:/Riot Games/VALORANT # install root"),
            Some("C:/Riot Games/VALORANT".to_owned())
        );
        assert_eq!(
            super::parse_yaml_scalar(r#""C:\\Riot Games\\VALORANT" # install root"#),
            Some(r"C:\Riot Games\VALORANT".to_owned())
        );
        assert_eq!(
            super::parse_yaml_scalar("'C:/Riot Games/Player''s VALORANT' # install root"),
            Some("C:/Riot Games/Player's VALORANT".to_owned())
        );
        assert_eq!(super::parse_yaml_scalar(r#""C:/Game" unexpected"#), None);
    }

    #[test]
    fn discovers_riot_titles_from_product_settings_yaml() {
        let root = std::env::temp_dir().join(format!(
            "amethys-riot-yaml-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let metadata = root.join("Metadata/valorant.live");
        let install = root.join("A Games/VALORANT");
        std::fs::create_dir_all(&metadata).expect("create metadata");
        std::fs::create_dir_all(&install).expect("create game install");
        std::fs::write(
            metadata.join("valorant.live.product_settings.yaml"),
            format!("\u{feff}product_install_root: \"{}\"\n", install.display()),
        )
        .expect("write Riot product settings with UTF-8 BOM");

        let games = super::scan_riot_product_settings(&root.join("Metadata"));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "VALORANT");
        assert_eq!(games[0].platform_id.as_deref(), Some("valorant"));
        assert_eq!(games[0].install_path, install);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_riot_client_metadata_with_missing_or_relative_game_paths() {
        let root = std::env::temp_dir().join(format!(
            "amethys-riot-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture");
        let metadata = root.join("RiotClientInstalls.json");
        let document = serde_json::json!({
            "valorant": { "product_install_root": root.join("missing/VALORANT").to_string_lossy() },
            "relative": { "install_path": "C:/Games/VALORANT" }
        });
        std::fs::write(&metadata, document.to_string()).expect("write metadata");
        assert!(super::scan_riot_client_installs(&metadata).is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn equal_name_folder_results_are_ordered_by_install_path() {
        let root = std::env::temp_dir().join(format!(
            "amethys-stable-name-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let first = root.join("a-library").join("Same Game");
        let second = root.join("z-library").join("Same Game");
        std::fs::create_dir_all(&second).expect("create second install");
        std::fs::create_dir_all(&first).expect("create first install");

        let games = scan_game_folders("EA app", &[root.join("z-library"), root.join("a-library")]);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, games[1].name);
        assert_eq!(games[0].install_path, first);
        assert_eq!(games[1].install_path, second);

        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
