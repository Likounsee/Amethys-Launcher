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
| Steam | `libraryfolders.vdf` and `appmanifest_*.acf` | Metadata-based detection; checks install directories and filters known Steam runtimes/redistributables. |
| Epic Games | Launcher `.item` JSON manifests | Metadata-based on Windows. Linux/macOS Heroic support still needs a dedicated adapter. |
| EA app | `__Installer/installerdata.xml` in each EA Games child directory | Reads game title and content ID when present; falls back to the install folder name. Custom locations remain to do. |
| GOG | Known GOG Games directories and Galaxy `galaxy-2.0.db` | Reads installed product IDs, original titles and installation paths from Galaxy metadata when available. |
| Ubisoft Connect | Windows registry install records plus known game directory | Reads install IDs and paths from the Ubisoft registry key; custom display-name lookup and other OS coverage remain to do. |
| Xbox | Known XboxGames directory | Preliminary folder-based detection; package-aware detection remains to do. |
| Heroic | Known Heroic games directory | Preliminary folder-based detection. |
| Battle.net | `%ProgramData%/Battle.net/Agent/product.db` protobuf metadata | Windows read-only adapter; extracts product codes and install paths, validates directories, ignores launcher records and malformed/stale entries. Known product codes get friendly names; unknown products fall back to the install-folder name. |
| Amazon Games | `GameInstallInfo.sqlite` (`DbSet`) | Read-only metadata detection; returns installed rows only when the directory exists. Windows path discovery is implemented. |

## Development priorities

1. Keep discovery read-only and avoid scanning broad folders that would produce false positives.
2. Prefer launcher metadata over guessing from directory names.
3. Improve Xbox package-aware detection and expand Battle.net product-name coverage. Battle.net, EA app, GOG Galaxy, Ubisoft Connect and Amazon Games now have metadata-based adapters.
4. Discover custom install locations where reliable metadata is available.
5. Add Linux/macOS coverage for launchers available on those systems.
6. Only after detection is dependable, add explicit import/selection into the Amethys library.
7. Build the future Amethys catalogue with its own stable IDs, independent of platform IDs.

A platform is not considered fully supported merely because a conventional folder is scanned. Its detection method, operating-system coverage and limitations must be tested and documented.
