use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// A game found in Steam's local installation metadata.
/// Discovery is read-only: it never changes platform files or the game library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGame {
    pub app_id: String,
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
        if key == "path" && !value.is_empty() {
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
            if !seen.insert(app_id.clone()) {
                continue;
            }

            let install_path = manifests.join("common").join(install_dir);
            if install_path.is_dir() {
                games.push(DetectedGame {
                    app_id,
                    name,
                    install_path,
                    platform: "Steam",
                });
            }
        }
    }

    games.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
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
    use super::{parse_app_manifest, parse_library_folders};
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
