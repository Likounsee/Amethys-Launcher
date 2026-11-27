//! Launch readiness of a library entry, derived from the real filesystem state.
//!
//! The interface used to ask the filesystem every time a card was drawn (and
//! on every comparison while sorting). Statuses are now computed once per
//! library load and reused by every page.

use amethys_ipc::IpcGameEntry;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    /// Folder and executable are valid: the game can be started.
    Ready,
    /// No executable has been configured yet.
    NeedsExecutable,
    /// An executable is configured but fails the launch checks.
    InvalidExecutable,
    /// The registered install folder does not exist.
    FolderMissing,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameStatus {
    pub kind: StatusKind,
    pub directory_exists: bool,
    /// Why the game cannot be launched (None when it can).
    pub block_reason: Option<String>,
}

impl StatusKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Prêt à lancer",
            Self::NeedsExecutable => "À configurer",
            Self::InvalidExecutable => "Exécutable invalide",
            Self::FolderMissing => "Dossier introuvable",
        }
    }

    /// Lower sorts first: launchable games lead, missing folders come last.
    pub fn rank(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::NeedsExecutable => 1,
            Self::InvalidExecutable => 2,
            Self::FolderMissing => 3,
        }
    }
}

impl GameStatus {
    pub fn is_ready(&self) -> bool {
        self.kind == StatusKind::Ready
    }
}

/// Pure classification, separated from the filesystem so it can be tested.
pub fn classify(directory_exists: bool, executable_configured: bool, block_reason: Option<String>) -> GameStatus {
    let kind = if !directory_exists {
        StatusKind::FolderMissing
    } else if block_reason.is_none() {
        StatusKind::Ready
    } else if executable_configured {
        StatusKind::InvalidExecutable
    } else {
        StatusKind::NeedsExecutable
    };
    GameStatus { kind, directory_exists, block_reason }
}

/// Reads the real filesystem state of one entry, using the exact launch checks.
pub fn status_of(game: &IpcGameEntry) -> GameStatus {
    classify(
        std::path::Path::new(&game.install_path).is_dir(),
        game.executable.is_some(),
        crate::core_client::launch_block_reason(game),
    )
}

/// Evaluates a known secondary installation with the same executable checks
/// used by the launch path, without changing the saved primary installation.
/// Compares installation paths using platform-appropriate path semantics.
pub fn same_install_path(left: &str, right: &str) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| std::path::PathBuf::from(left));
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| std::path::PathBuf::from(right));

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

pub fn status_for_installation(game: &IpcGameEntry, install_path: &str) -> GameStatus {
    let mut selected = game.clone();
    selected.install_path = install_path.to_owned();
    classify(
        std::path::Path::new(install_path).is_dir(),
        selected.executable.is_some(),
        crate::core_client::launch_block_reason(&selected),
    )
}

pub fn statuses_of(games: &[IpcGameEntry]) -> Vec<GameStatus> {
    games.iter().map(status_of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_folder_wins_over_every_other_state() {
        let status = classify(false, true, Some("x".into()));
        assert_eq!(status.kind, StatusKind::FolderMissing);
        assert!(!status.is_ready());
    }

    #[test]
    fn ready_requires_a_folder_and_no_block_reason() {
        assert_eq!(classify(true, true, None).kind, StatusKind::Ready);
        assert!(classify(true, true, None).is_ready());
    }

    #[test]
    fn unconfigured_and_invalid_executables_are_distinguished() {
        assert_eq!(classify(true, false, Some("none".into())).kind, StatusKind::NeedsExecutable);
        assert_eq!(classify(true, true, Some("bad".into())).kind, StatusKind::InvalidExecutable);
    }

    #[test]
    fn launchable_games_sort_before_broken_ones() {
        let mut kinds = [StatusKind::FolderMissing, StatusKind::NeedsExecutable, StatusKind::Ready, StatusKind::InvalidExecutable];
        kinds.sort_by_key(|kind| kind.rank());
        assert_eq!(kinds[0], StatusKind::Ready);
        assert_eq!(kinds[3], StatusKind::FolderMissing);
    }

    #[test]
    fn secondary_installation_status_uses_selected_path_without_mutating_primary() {
        let root = std::env::temp_dir().join(format!(
            "amethys-secondary-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let primary = root.join("primary");
        let secondary = root.join("secondary");
        std::fs::create_dir_all(&secondary).expect("create secondary installation");
        std::fs::write(secondary.join("game.exe"), b"fixture").expect("create executable");

        let game = IpcGameEntry {
            id: "game".to_owned(),
            name: "Game".to_owned(),
            platform: "Steam".to_owned(),
            platform_id: Some("123".to_owned()),
            catalogue_game_id: None,
            catalogue_metadata: None,
            install_path: primary.to_string_lossy().into_owned(),
            install_locations: vec![
                primary.to_string_lossy().into_owned(),
                secondary.to_string_lossy().into_owned(),
            ],
            executable: Some("game.exe".to_owned()),
        };

        let ready = status_for_installation(&game, &secondary.to_string_lossy());
        assert_eq!(ready.kind, StatusKind::Ready);
        assert!(ready.is_ready());
        assert_eq!(game.install_path, primary.to_string_lossy());

        let missing = status_for_installation(&game, &root.join("missing").to_string_lossy());
        assert_eq!(missing.kind, StatusKind::FolderMissing);

        std::fs::remove_dir_all(root).expect("remove installation fixture");
    }
}
