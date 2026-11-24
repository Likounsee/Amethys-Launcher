use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::{DetectedGame, read_bounded_text};

/// Reads GDK game metadata from known Xbox app library roots.
///
/// A directory is accepted only when it contains a valid MicrosoftGame.config
/// identity, a game executable declared by ExecutableList, and an existing
/// executable file. Generic XboxGames folders are never treated as proof.
pub fn scan_xbox_game_roots(roots: &[PathBuf]) -> Vec<DetectedGame> {
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();

    let mut games = Vec::new();
    let mut seen = BTreeSet::new();

    for library in roots {
        if !library.is_absolute() || !library.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&library) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let game_root = entry.path();
            if !game_root.is_dir() {
                continue;
            }

            let candidates = [
                game_root.join("Content/MicrosoftGame.config"),
                game_root.join("MicrosoftGame.config"),
            ];
            let Some(config) = candidates.iter().find(|path| path.is_file()) else {
                continue;
            };
            let Some(contents) = read_bounded_text(config) else {
                continue;
            };
            let Some((identity, title, executable)) = parse_microsoft_game_config(&contents) else {
                continue;
            };
            // Resolve both paths before accepting the installation. A declared
            // executable can be a symlink (or Windows junction) that escapes
            // the game directory even though is_file() reports true.
            let Ok(canonical_game_root) = game_root.canonicalize() else {
                continue;
            };
            if !canonical_game_root.is_dir() {
                continue;
            }
            let content_root = config.parent().unwrap_or(&game_root);
            let executable_path = content_root.join(executable);
            let Ok(canonical_executable) = executable_path.canonicalize() else {
                continue;
            };
            if !canonical_executable.starts_with(&canonical_game_root)
                || !canonical_executable.is_file()
            {
                continue;
            }
            if !seen.insert(normalize_path(&game_root)) {
                continue;
            }

            games.push(DetectedGame {
                platform_id: Some(identity),
                name: title,
                install_path: game_root,
                platform: "Xbox app",
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

/// Parses the required GDK identity and executable declaration plus the
/// optional human-readable shell title from MicrosoftGame.config.
fn parse_microsoft_game_config(contents: &str) -> Option<(String, String, String)> {
    let identity = xml_start_tag(contents, "Identity")?;
    let identity_name = xml_attribute(&identity, "Name")?;
    if identity_name.trim().is_empty() {
        return None;
    }

    let executable_list_start = find_start_tag(contents, "ExecutableList")?;
    let executable_list_end = find_end_tag(contents, "ExecutableList", executable_list_start)?;
    let executable_list = &contents[executable_list_start..executable_list_end];
    let executable_tag = xml_start_tag(executable_list, "Executable")?;
    let executable = xml_attribute(&executable_tag, "Name")?;
    let executable_path = Path::new(&executable);
    if executable.trim().is_empty()
        || executable.contains('\\')
        // A colon can introduce a drive-relative path or an NTFS alternate
        // data stream on Windows, neither of which is a valid GDK executable.
        || executable.contains(':')
        || executable.contains('\0')
        || executable_path.is_absolute()
        || executable_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    let title = xml_start_tag(contents, "ShellVisuals")
        .and_then(|tag| xml_attribute(&tag, "DefaultDisplayName"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| identity_name.clone());

    Some((identity_name, title, executable))
}

fn find_start_tag(contents: &str, element: &str) -> Option<usize> {
    super::find_xml_tag(contents, element, false, 0)
}

fn find_end_tag(contents: &str, element: &str, from: usize) -> Option<usize> {
    super::find_xml_tag(contents, element, true, from)
}

fn xml_start_tag(contents: &str, element: &str) -> Option<String> {
    let start = find_start_tag(contents, element)?;
    let end = super::find_xml_tag_end(contents, start)?;
    Some(contents.get(start..end)?.to_owned())
}

/// Reads a quoted XML attribute without accepting partial attribute names.
fn xml_attribute(tag: &str, wanted: &str) -> Option<String> {
    let mut offset = tag.find(char::is_whitespace)?;
    while offset < tag.len() {
        let remaining = &tag[offset..];
        let whitespace = remaining.len() - remaining.trim_start().len();
        offset += whitespace;
        if offset >= tag.len() || tag.as_bytes()[offset] == b'>' || tag.as_bytes()[offset] == b'/' {
            break;
        }

        let name_start = offset;
        while offset < tag.len()
            && !tag.as_bytes()[offset].is_ascii_whitespace()
            && !matches!(tag.as_bytes()[offset], b'=' | b'>' | b'/')
        {
            offset += 1;
        }
        if name_start == offset {
            offset += 1;
            continue;
        }
        let name = &tag[name_start..offset];
        while offset < tag.len() && tag.as_bytes()[offset].is_ascii_whitespace() {
            offset += 1;
        }
        if tag.as_bytes().get(offset) != Some(&b'=') {
            continue;
        }
        offset += 1;
        while offset < tag.len() && tag.as_bytes()[offset].is_ascii_whitespace() {
            offset += 1;
        }
        let quote = *tag.as_bytes().get(offset)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        offset += 1;
        let value_start = offset;
        while offset < tag.len() && tag.as_bytes()[offset] != quote {
            offset += 1;
        }
        let value = tag.get(value_start..offset)?;
        if name.eq_ignore_ascii_case(wanted) {
            return Some(super::decode_xml_entities(value));
        }
        offset += 1;
    }
    None
}

fn normalize_path(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        PathBuf::from(canonical.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        canonical
    }
}

/// Finds Xbox app libraries on Windows only when the Xbox library marker and
/// its XboxGames directory are both present on a drive root.
#[cfg(windows)]
pub fn discover_xbox_games() -> Vec<DetectedGame> {
    let mut roots = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", char::from(letter)));
        if drive.join(".GamingRoot").is_file() {
            let library = drive.join("XboxGames");
            if library.is_dir() {
                roots.push(library);
            }
        }
    }
    scan_xbox_game_roots(&roots)
}

#[cfg(not(windows))]
pub fn discover_xbox_games() -> Vec<DetectedGame> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::scan_xbox_game_roots;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "amethys-xbox-discovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    #[test]
    fn discovers_only_games_with_gdk_identity_and_existing_declared_executable() {
        let root = fixture_root();
        let content = root.join("XboxGames/Example Game/Content");
        fs::create_dir_all(&content).expect("create content");
        fs::write(content.join("ExampleGame.exe"), b"fixture").expect("write executable");
        fs::write(
            content.join("MicrosoftGame.config"),
            r#"
            <Game configVersion="1">
              <Identity Name="Publisher.ExampleGame" Publisher="CN=Example" Version="1.0.0.0"/>
              <ExecutableList><Executable Name="ExampleGame.exe" Id="Game"/></ExecutableList>
              <ShellVisuals DefaultDisplayName="Example &amp; Game"/>
            </Game>
        "#,
        )
        .expect("write config");

        let games = scan_xbox_game_roots(&[root.join("XboxGames")]);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Example & Game");
        assert_eq!(
            games[0].platform_id.as_deref(),
            Some("Publisher.ExampleGame")
        );
        assert_eq!(games[0].install_path, root.join("XboxGames/Example Game"));
        assert_eq!(games[0].platform, "Xbox app");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ignores_generic_folders_invalid_configs_and_missing_executables() {
        let root = fixture_root();
        let library = root.join("XboxGames");
        let generic = library.join("Not a game");
        fs::create_dir_all(&generic).expect("create generic folder");
        let missing = library.join("Missing executable/Content");
        fs::create_dir_all(&missing).expect("create incomplete game");
        fs::write(
            missing.join("MicrosoftGame.config"),
            r#"
            <Game><Identity Name="Publisher.Missing"/>
            <ExecutableList><Executable Name="missing.exe" Id="Game"/></ExecutableList></Game>
        "#,
        )
        .expect("write config");
        let invalid = library.join("Invalid config/Content");
        fs::create_dir_all(&invalid).expect("create invalid game");
        fs::write(
            invalid.join("MicrosoftGame.config"),
            "<Game><Identity/></Game>",
        )
        .expect("write invalid config");

        assert!(scan_xbox_game_roots(&[library]).is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn ignores_declared_executable_symlinks_that_escape_game_root() {
        use std::os::unix::fs::symlink;

        let root = fixture_root();
        let library = root.join("XboxGames");
        let game = library.join("Example Game");
        let content = game.join("Content");
        fs::create_dir_all(&content).expect("create game content");

        let outside = root.join("outside.exe");
        fs::write(&outside, b"outside executable").expect("write outside executable");
        symlink(&outside, content.join("ExampleGame.exe")).expect("create escaping symlink");
        fs::write(
            content.join("MicrosoftGame.config"),
            r#"<Game><Identity Name="Publisher.ExampleGame"/><ExecutableList><Executable Name="ExampleGame.exe"/></ExecutableList></Game>"#,
        )
        .expect("write config");

        assert!(
            scan_xbox_game_roots(&[library]).is_empty(),
            "an executable symlink outside the game root must not be discovered"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn ignores_declared_executables_reached_through_content_junctions_outside_game_root() {
        use std::process::Command;

        let root = fixture_root();
        let library = root.join("XboxGames");
        let game = library.join("Example Game");
        let outside_content = root.join("OutsideContent");
        fs::create_dir_all(&game).expect("create game root");
        fs::create_dir_all(&outside_content).expect("create external content");

        fs::write(
            outside_content.join("ExampleGame.exe"),
            b"outside executable",
        )
        .expect("write external executable");
        fs::write(
            outside_content.join("MicrosoftGame.config"),
            r#"<Game><Identity Name="Publisher.ExampleGame"/><ExecutableList><Executable Name="ExampleGame.exe"/></ExecutableList></Game>"#,
        )
        .expect("write external config");

        // Directory junctions do not require the symlink privilege on Windows.
        // The metadata appears below the game folder lexically, but resolves
        // outside its canonical root and therefore must not be accepted.
        let junction = game.join("Content");
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "New-Item -ItemType Junction -Path $env:AMETHYS_JUNCTION -Target $env:AMETHYS_TARGET | Out-Null",
            ])
            .env("AMETHYS_JUNCTION", &junction)
            .env("AMETHYS_TARGET", &outside_content)
            .output()
            .expect("run PowerShell to create content junction");
        assert!(
            output.status.success(),
            "failed to create Content junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            scan_xbox_game_roots(&[library]).is_empty(),
            "an executable reached through a Content junction outside the game root must not be discovered"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_unsafe_declared_executable_paths() {
        for executable in [
            "../game.exe",
            "C:game.exe",
            "game.exe:stream",
            "/absolute/game.exe",
            "nested\\\\game.exe",
        ] {
            let config = format!(
                r#"<Game><Identity Name="Publisher.Game"/><ExecutableList><Executable Name="{executable}"/></ExecutableList></Game>"#
            );
            assert!(
                super::parse_microsoft_game_config(&config).is_none(),
                "accepted unsafe executable path: {executable}"
            );
        }
    }
}
