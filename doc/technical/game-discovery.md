# Game discovery

Amethys discovery is read-only. A scan reports installed games; it does not add them to the user's Amethys library, start a launcher, or modify launcher metadata.

## Identifier rules

- A platform ID belongs to its platform. For example, Steam AppID `570` identifies Dota 2 only within Steam.
- Other platforms have their own identifier formats and namespaces.
- The future Amethys catalogue will assign a separate Amethys ID to each game. A free-to-play game can have an Amethys ID just like a paid game.
- Never treat a platform ID as a universal game ID, and never merge two games solely because their platform IDs happen to match.

## Current discovery coverage

| Platform | Current method | Notes |
| --- | --- | --- |
| Steam | `libraryfolders.vdf` and `appmanifest_*.acf`; Windows `HKCU\\Software\\Valve\\Steam\\SteamPath` | Metadata-based detection; checks install directories, filters known Steam runtimes/redistributables, validates numeric AppIDs against manifest filenames, parses escaped quotes in VDF values, and discovers custom Windows Steam roots from the current user's registry without elevation. |
| Epic Games | Launcher `.item` JSON manifests | Metadata-based on Windows. Heroic/Legendary is handled by its own adapter on supported OSes. |
| EA app | `__Installer/installerdata.xml` plus Windows `HKLM\\SOFTWARE\\EA Games` install records | Reads title and content ID when present; registry paths allow custom game locations outside `Program Files/EA Games`. Quoted paths and known `%NAME%` environment references are normalized without invoking a shell; unknown variables remain unresolved. A registry record is accepted only when the install directory and EA installer metadata exist. |
| GOG | Galaxy `galaxy-2.0.db` | Reads installed product IDs, original titles and installation paths from Galaxy metadata; opens the database read-only, skips malformed titles and stale paths, and de-duplicates canonical install paths without folding case on case-sensitive filesystems. Generic `GOG Games` folders are deliberately not scanned because folder names alone can produce false positives. |
| Ubisoft Connect | Windows registry install records | Reads install IDs and paths from the Ubisoft registry key. Generic Ubisoft game folders are not scanned, because leftover or manually-created directories do not prove an installed game. Custom display-name lookup and other OS coverage remain to do. |
| Xbox | Not currently scanned | Folder-name guessing was removed because a directory alone does not prove that a game package is installed. Package-aware, permission-respecting detection remains to do. |
| Heroic | Heroic/Legendary `installed.json` | Reads installed app names, titles and paths from Legendary metadata; validates that each install directory exists. Checks Heroic's platform-specific application-data location (including Linux Flatpak) and standalone Legendary's XDG config location. |
| Battle.net | `%ProgramData%/Battle.net/Agent/product.db` protobuf metadata | Windows read-only adapter; extracts product codes, install paths and the launcher's explicit installed flag; it validates directories and ignores launcher records, uninstalled entries and malformed/stale data. Known product codes get friendly names; unknown products fall back to the install-folder name. |
| Amazon Games | `GameInstallInfo.sqlite` (`DbSet`) | Read-only metadata detection; returns installed rows only when the directory exists. Windows path discovery is implemented. |

## Importing into the Amethys library

Detected games are imported only when the user explicitly requests
`amethysctl games import <platform> <platform-id>` or
`amethysctl games import-all`. The bulk command imports each detected game
sequentially, skips entries already present using the platform-scoped ID (or
installation path for legacy entries), and reports added, skipped and failed
counts. A failed item does not prevent later detected games from being tried.
The import stores the
source platform's identifier separately from Amethys' own local library ID.
This preserves the identifier namespace: the same platform ID may identify
different products on different stores.

The local database schema migration adds an optional `platform_id` field and
a uniqueness constraint on the case-insensitive platform name plus the
platform-specific ID. Existing library entries are preserved and receive a
null platform ID; they remain addressable by their Amethys library ID and
installation path. Detection remains read-only: importing changes only the
Amethys database and never modifies the original launcher or game installation.

## Read-only installation verification

Run `amethysctl games verify` to inspect every game already saved in the
Amethys library. The command does not launch games, hash or read game contents,
repair installations, or modify game/launcher files.

For each entry it reports the installation-directory state:

- `available`: the path resolves to a directory.
- `missing`: the path does not exist.
- `wrong_type`: the path exists but is not the expected filesystem object.
- `inaccessible`: the operating system denied metadata access.
- `error`: another filesystem metadata error occurred.

When an executable path is configured on a library entry, verification also
checks it. Relative executable paths are resolved beneath the saved installation
directory; absolute paths are checked as supplied. Executable states use
`available`, `missing`, `wrong_type`, `inaccessible`, or `error`.
Entries without a configured executable report `not_configured` and are not
penalized.

Use `amethysctl games verify --json` for the versioned JSON report
(`schema_version: 2`). It includes `directory_status`,
`executable_status`, per-entry `healthy`, and aggregate `available`,
`missing`, `healthy`, and `issues` counts. A directory being available
does not prove that all game files are intact or that the game can start; this
check validates only path metadata and any explicitly configured executable.

## Development priorities

1. Keep discovery read-only and require launcher metadata or another reliable installation record; do not infer games from generic folder names.
2. Implement Xbox package-aware detection and expand Battle.net product-name coverage. Battle.net, EA app, GOG Galaxy, Ubisoft Connect, Amazon Games and Heroic/Legendary have metadata-based adapters.
3. Expand custom install-location discovery for launchers where reliable metadata is available; Steam's per-user Windows registry root and EA registry locations are now covered.
4. Add Linux/macOS coverage for launchers available on those systems.
5. Only after detection is dependable, add explicit import/selection into the Amethys library.
6. Build the future Amethys catalogue with its own stable IDs, independent of platform IDs.

A platform is not considered fully supported merely because a conventional folder is scanned. Its detection method, operating-system coverage and limitations must be tested and documented.
