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
}
