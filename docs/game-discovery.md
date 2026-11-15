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
| Steam | `libraryfolders.vdf` and `appmanifest_*.acf` | Metadata-based detection; checks that the install directory exists. |
| Epic Games | Launcher `.item` JSON manifests | Metadata-based on Windows. Linux/macOS Heroic support still needs a dedicated adapter. |
| EA app | Known EA Games directory | Folder-based preliminary detection; custom locations and metadata need improvement. |
| GOG | Known GOG Games directories | Folder-based preliminary detection; GOG Galaxy database support remains to do. |
| Ubisoft Connect | Known Ubisoft game directory | Folder-based preliminary detection; registry/custom locations remain to do. |
| Xbox | Known XboxGames directory | Preliminary folder-based detection; package-aware detection remains to do. |
| Heroic | Known Heroic games directory | Preliminary folder-based detection. |
| Battle.net | Not implemented yet | Needs a dedicated metadata adapter and tests. |
| Amazon Games | Not implemented yet | Needs a dedicated metadata adapter and tests. |

## Development priorities

1. Keep discovery read-only and avoid scanning broad folders that would produce false positives.
2. Prefer launcher metadata over guessing from directory names.
3. Add dedicated adapters and fixture-based tests for Battle.net, GOG Galaxy, EA app, Ubisoft Connect, Xbox and Amazon Games.
4. Discover custom install locations where reliable metadata is available.
5. Add Linux/macOS coverage for launchers available on those systems.
6. Only after detection is dependable, add explicit import/selection into the Amethys library.
7. Build the future Amethys catalogue with its own stable IDs, independent of platform IDs.

A platform is not considered fully supported merely because a conventional folder is scanned. Its detection method, operating-system coverage and limitations must be tested and documented.
