# Game launcher discovery coverage

Amethys performs read-only discovery from local launcher metadata. A launcher being listed here does not mean it is installed on the current machine, and an adapter returning no games does not prove the launcher is absent.

## Implemented adapters

| Service | Detection source | Implementation / validation |
|---|---|---|
| Steam | Steam library folders and app manifests; native Windows/Linux roots, Linux Flatpak root and custom XDG data root | Implemented; locally exercised on Windows with real installations; Linux is covered by CI fixtures but still needs a real-machine check |
| Epic Games Launcher | Epic .item manifests; Windows ProgramData root | Implemented on Windows; locally exercised with real installations; Linux uses Heroic/Legendary metadata |
| Heroic / Legendary | installed.json | Implemented; fixture-tested in CI; local validation depends on having a Heroic/Legendary game installed |
| GOG Galaxy | Local installed-product database, opened read-only | Implemented; fixture-tested in CI; local validation depends on having a GOG game installed |
| Amazon Games | Local launcher database, opened read-only | Implemented; fixture-tested in CI; local validation depends on having an Amazon Games title installed |
| EA app | Installer metadata in both Program Files game-library roots, optional custom roots via `AMETHYS_EA_LIBRARY_ROOTS`, plus Windows registry install records | Implemented; locally exercised with Apex Legends using a custom library root |
| Ubisoft Connect | Windows registry install records | Implemented; fixture-tested in CI; local validation depends on having a Ubisoft game installed |
| Battle.net | Local Agent product metadata | Implemented and fixture-tested in CI; **not locally validated with a real Battle.net game yet**. Community testing is needed |
| Riot Games | Riot Client JSON and per-product product_settings.yaml metadata (VALORANT, League of Legends, LoR, 2XKO) | Implemented; locally exercised with VALORANT; other Riot titles still need real-install validation |
| Xbox app (GDK titles) | Xbox library marker, `XboxGames` directory, and each game's `MicrosoftGame.config` identity plus declared executable | Initial adapter; local detection of Xbox app entries is not proof that the GDK-specific path has been validated; real GDK installs still need testing |

## Game-specific coverage notes

- THE FINALS is already detected through Steam AppID metadata (2073850).
- VALORANT is detected through Riot Client installation metadata and Riot product settings.
- Apex Legends is not a dedicated adapter: it should be discovered through its installing platform (Steam AppID metadata or EA app installer/registry metadata). The EA app adapter now checks both standard Program Files game-library roots as well as registry-recorded custom paths. A real installation still needs to be validated on a machine where Apex is installed.

## Planned adapters and investigations

| Service | Next work |
|---|---|
| Microsoft Store / non-GDK packaged games | Investigate per-user package registration and a reliable way to distinguish games from ordinary apps; do not enumerate all apps as games |
| Rockstar Games Launcher | Find a reliable local installed-product manifest or registry source |
| itch.io | Inspect Butler/itch install metadata and validate paths |
| HoYoPlay / HoYoverse | Identify a stable local game installation manifest |
| Minecraft Launcher | Distinguish Java installations, game directories and launcher profiles |
| Standalone emulators / community launchers | Evaluate individually where local metadata is stable and useful |

## Safety and accuracy rules

- Discovery is read-only and must not start launchers or modify their data.
- Steam discovery on Linux checks conventional native roots, the Flatpak per-user root, and an absolute `XDG_DATA_HOME` root when configured. macOS support is paused for the 0.1 release until it can be tested on physical Mac hardware.
- Only absolute, existing installation directories are accepted.
- Launcher runtimes, anti-cheat components and shared dependencies must not be reported as games.
- Avoid guessing from generic folders. A folder name alone is not sufficient evidence of an installed game.
- The Xbox adapter currently targets GDK installs with a valid `MicrosoftGame.config` and an existing declared executable. It does not yet cover every Microsoft Store/UWP title.
- Keep each adapter independently testable with synthetic metadata fixtures.
- Do not bypass DRM, account checks, licensing, authentication, anti-cheat or Windows package permissions.

## Important limitation

A successful directory check confirms only that the installation directory exists. It does not verify game-file integrity, launcher ownership, updates, or whether the game can start.

## Real-install validation checklist

Synthetic fixtures validate parsers and edge cases, but they cannot prove that a launcher version stores metadata at the expected path on a real machine. Keep these two kinds of evidence separate in reviews and release notes.

When a maintainer does not own a game on a platform (for example, Battle.net), do not mark that adapter as locally validated and do not install or purchase a game solely for this test. Ask community testers to opt in and report:

1. Windows version and launcher version (if known).
2. Whether at least one game is installed through that launcher.
3. Whether `games scan --json` detects it, including the platform name and install path (redact personal usernames and private directory names before sharing).
4. Whether a second scan returns the same result and whether an uninstalled/stale entry is correctly ignored.
5. Any relevant Amethys error output. Never request account credentials, tokens, launcher databases, or other private metadata files.

A platform should be labelled **real-install validated** only after a reproducible report from a real installation. Otherwise label it **fixture/CI tested; real-install validation pending**. Missing local games are a testing limitation, not a reason to remove an otherwise useful adapter.
