# Game launcher discovery coverage

Amethys performs read-only discovery from local launcher metadata. A launcher being listed here does not mean it is installed on the current machine, and an adapter returning no games does not prove the launcher is absent.

## Implemented adapters

| Service | Detection source | Status |
|---|---|---|
| Steam | Steam library folders and app manifests | Implemented |
| Epic Games Launcher | Epic .item manifests | Implemented |
| Heroic / Legendary | installed.json | Implemented |
| GOG Galaxy | Local installed-product database, opened read-only | Implemented |
| Amazon Games | Local launcher database, opened read-only | Implemented |
| EA app | Installer metadata in both Program Files game-library roots, plus Windows registry install records for custom paths | Implemented |
| Ubisoft Connect | Windows registry install records | Implemented |
| Battle.net | Local Agent product metadata | Implemented |
| Riot Games | Riot Client JSON and per-product product_settings.yaml metadata (VALORANT, League of Legends, LoR, 2XKO) | Initial adapter; validate against real installations |

## Game-specific coverage notes

- THE FINALS is already detected through Steam AppID metadata (2073850).
- VALORANT is detected through Riot Client installation metadata and Riot product settings.
- Apex Legends is not a dedicated adapter: it should be discovered through its installing platform (Steam AppID metadata or EA app installer/registry metadata). The EA app adapter now checks both standard Program Files game-library roots as well as registry-recorded custom paths. A real installation still needs to be validated on a machine where Apex is installed.

## Planned adapters and investigations

| Service | Next work |
|---|---|
| Xbox app / Microsoft Store | Read package registration metadata using supported Windows APIs; never infer games from XboxGames folders alone |
| Rockstar Games Launcher | Find a reliable local installed-product manifest or registry source |
| itch.io | Inspect Butler/itch install metadata and validate paths |
| HoYoPlay / HoYoverse | Identify a stable local game installation manifest |
| Minecraft Launcher | Distinguish Java installations, game directories and launcher profiles |
| Standalone emulators / community launchers | Evaluate individually where local metadata is stable and useful |

## Safety and accuracy rules

- Discovery is read-only and must not start launchers or modify their data.
- Only absolute, existing installation directories are accepted.
- Launcher runtimes, anti-cheat components and shared dependencies must not be reported as games.
- Avoid guessing from generic folders. A folder name alone is not sufficient evidence of an installed game.
- Keep each adapter independently testable with synthetic metadata fixtures.
- Do not bypass DRM, account checks, licensing, authentication, anti-cheat or Windows package permissions.

## Important limitation

A successful directory check confirms only that the installation directory exists. It does not verify game-file integrity, launcher ownership, updates, or whether the game can start.
