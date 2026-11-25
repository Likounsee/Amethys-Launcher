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
| Xbox app (GDK) | Xbox library marker, `XboxGames`, and each title's `MicrosoftGame.config` | Initial Windows adapter; requires a valid GDK identity and an existing declared executable. It does not enumerate every Microsoft Store/UWP package. |
| Heroic | Heroic/Legendary `installed.json` | Reads installed app names, titles and paths from Legendary metadata; validates that each install directory exists. Checks Heroic's platform-specific application-data location (including Linux Flatpak) and standalone Legendary's XDG config location. |
| Battle.net | `%ProgramData%/Battle.net/Agent/product.db` protobuf metadata | Windows read-only adapter; extracts product codes, install paths and the launcher's explicit installed flag; it validates directories and ignores launcher records, uninstalled entries and malformed/stale data. Known product codes get friendly names; unknown products fall back to the install-folder name. |
| Amazon Games | `GameInstallInfo.sqlite` (`DbSet`) | Read-only metadata detection; returns installed rows only when the directory exists. Windows path discovery is implemented. |
| Riot Games | Riot Client `RiotClientInstalls.json` and per-product `product_settings.yaml` | Initial metadata adapter for installed Riot products; validates absolute existing install roots and ignores Riot Client itself. Validate additional product formats against real installations. |

## Importing into the Amethys library

Detected games are imported only when the user explicitly requests
`amethysctl games import <platform> <platform-id>` or
`amethysctl games import-all`. The bulk command imports each detected game
sequentially, skips entries already present using the platform-scoped ID (or the
canonical installation path within the same platform when no ID is available), and reports added, skipped and failed
counts. A failed item does not prevent later detected games from being tried.
The import stores the
source platform's identifier separately from Amethys' own local library ID.
This preserves the identifier namespace: the same platform ID may identify
different products on different stores.

The local database schema migration adds an optional `platform_id` field and
a uniqueness constraint on the case-insensitive platform name plus the
platform-specific ID. Existing library entries are preserved and initially
receive a null platform ID. If a later scan identifies one of these legacy
entries by the same platform and canonical installation path, import attaches
the newly discovered platform ID to that existing row without changing its
Amethys library ID. Future imports can then find it by stable platform identity,
even if its installation path changes. When a later explicit import finds an
existing entry by that identity, Amethys refreshes its display name and detected
installation path in place, preserving the local library ID and any manually
configured executable when the adapter cannot resolve a replacement. This
prevents a moved game from remaining attached to a stale path without creating
a duplicate library entry. Detection remains read-only: importing changes only
the Amethys database and never modifies the original launcher or game
installation.

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
directory; paths containing traversal components (`.` or `..`), rooted relative
paths, and drive-relative paths are rejected as `invalid_path`. Absolute paths
are accepted only when their canonical target remains inside the saved
installation directory. This also rejects symlinks and Windows junctions that
redirect an executable outside that directory. Executable states use
`available`, `missing`, `wrong_type`, `inaccessible`, `invalid_path`, or `error`.
Entries without a configured executable report `not_configured` and are not
penalized.

Use `amethysctl games verify --json` for the versioned JSON report
(`schema_version: 3`). It includes `directory_status`,
`executable_status`, per-entry `healthy` and `launch_ready`, plus
aggregate `available`, `missing`, `healthy`, `launch_ready`, and
`issues` counts. `healthy` means the installation directory exists and
any configured executable passes path validation; an unconfigured executable
does not make the installation unhealthy. `launch_ready` is stricter: it is
true only when the installation directory exists and a configured executable
is present inside it. A directory being available does not prove that all game
files are intact or that the game can start; this check validates only path
metadata and any explicitly configured executable.

## Development priorities

1. Keep discovery read-only and require launcher metadata or another reliable installation record; do not infer games from generic folder names.
2. Expand Xbox support beyond GDK titles through permission-respecting Microsoft Store package metadata, with reliable game-vs-app classification.
3. Expand Battle.net product-name coverage and validate Riot/Xbox adapters against more real installations.
4. Expand custom install-location discovery and Linux/macOS coverage where reliable launcher metadata is available.
5. Improve explicit import/selection and read-only verification in the Amethys library.
6. Build the future Amethys catalogue with its own stable IDs, independent of platform IDs.

A platform is not considered fully supported merely because a conventional folder is scanned. Its detection method, operating-system coverage and limitations must be tested and documented.
