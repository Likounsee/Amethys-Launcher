# Desktop application — version 0.2.0

Amethys Launcher 0.2.0 is the first published desktop preview. The release is available for Windows x64 and Linux x86_64 GNU as portable archives. It remains experimental and is not a stable or production-ready launcher.

- Published release: [v0.2.0](https://github.com/Likounsee/Amethys-Launcher/releases/tag/v0.2.0)
- User-facing notes: [0.2.0 release notes](../releases/0.2.0-release-notes.md)
- Product plan: [ROADMAP-0.2.md](../../ROADMAP-0.2.md)

## Architecture

- The desktop UI is rendered with the vendored GhostEngine snapshot in `prototypes/ghostengine-amethys-preview` and `amethys-launcher-ui/vendor/ghostengine`.
- The desktop client communicates with Amethys Core through the existing versioned IPC protocol.
- Core remains the only owner of persistent game-library data and its SQLite database. The desktop application does not open the database directly.
- Desktop-only preferences and imported media are stored in the user's local Amethys data directory.

## Capabilities in 0.2.0

- Browse the locally registered game library, search game metadata, filter by platform and sort entries.
- Persist the selected library sort order on the current device.
- Inspect a game's launch status and configure its executable path.
- Launch a configured executable, remove a local library entry and customize or reset its cover.
- Discover supported installed games and explicitly import detected installations.
- Resize the window, use responsive layouts, navigate actionable controls with Tab / Shift+Tab / Enter, and use the documented Escape and F11 shortcuts.
- Load and refresh the Core library asynchronously so IPC requests do not block the window's UI thread.
- Run imports, executable-path updates and library removals as background Core operations, with visible completion and error feedback.
- Display explicit loading, connection, error, confirmation and notification states.

The 0.2.0 release does not provide a connected commercial store, purchases, licences, account management, game downloads or game installation. Discovery and launch readiness remain limited to the adapters and checks implemented by the project.

## Install the published portable package

Download the correct archive from the [v0.2.0 release page](https://github.com/Likounsee/Amethys-Launcher/releases/tag/v0.2.0).

- **Windows x64:** extract the ZIP and run `amethys-launcher.exe`.
- **Linux x86_64 GNU:** extract the TAR.GZ and run `./amethys-launcher`. If necessary, restore the executable bit with `chmod +x amethys-launcher`.

The archives also contain `amethys-agent`, `amethysctl`, the required fonts and cover assets, README, LICENSE, `RELEASES.md` and `LIRE-MOI.txt`. They are portable packages, not system installers. Use `amethysctl --help` (or `./amethysctl --help` on Linux) to inspect CLI commands.

Keep official launchers as the source of truth for game installation, updates, authentication and licences.

## Run from a source checkout

From the repository root, with the Rust toolchain specified by `rust-toolchain.toml`:

```sh
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bin amethys-launcher
```

For a release build:

```sh
cargo build --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bin amethys-launcher
```

## Validation

The dedicated [v0.2.0 release workflow](https://github.com/Likounsee/Amethys-Launcher/actions/runs/45235089145) completed successfully on 23 November 2026. Both Windows and Linux jobs passed:

- workspace release tests;
- desktop Clippy with warnings denied;
- desktop tests;
- workspace and desktop release builds;
- platform archive staging and smoke checks;
- packaged CLI version/help checks and packaged agent/IPC startup checks.

The publish job generated and verified the combined `SHA256SUMS` manifest and published both platform archives.

### Windows manual validation

A separate Windows x64 desktop development package was extracted and launched successfully on a real Windows machine. All 15 manual checks passed, covering startup, Core connection, library display, import, executable configuration, game launch, deletion and cancellation, sorting and persistence, refresh, keyboard navigation, window sizes/fullscreen, invalid executable paths and launching from the extracted portable ZIP.

The tested development archive was `Amethys-Windows-x64.zip`. Its SHA-256 was:

```text
d02e616d99c038d3856f6d2d55840c75b16aeec0a8bab44de8cbb0034306ae59
```

This hash identifies that exact locally generated development archive, not the published release ZIP. Rebuilding may produce a different hash.

### Linux validation

The Linux x86_64 GNU release archive passed CI build, tests, packaging and extracted-archive smoke checks. Manual testing on a physical Linux machine has not yet been completed. Do not describe Linux as real-machine validated until that testing has been performed.

Automated validation does not guarantee compatibility with every PC, desktop environment, launcher or game installation. See [launcher discovery coverage](../discovery/LAUNCHER-COVERAGE.md) for adapter-specific evidence.

## Build a local Windows development package

On Windows PowerShell, from the repository root:

```powershell
.\scripts\package-desktop-windows.ps1
```

The script runs workspace and desktop checks/tests, builds the agent, CLI and desktop executable, then stages the application and its required fonts and cover assets. It creates a development ZIP and checksum files. This local development package is distinct from the published release archives generated by the GitHub Actions release workflow.

## Known limitations

- Experimental preview; not production-ready.
- No connected commercial store, pricing, purchases, licences, account system, game downloads or installation.
- No cloud saves, cross-device synchronization or automatic updater.
- Launcher detection is metadata-based and adapter-specific.
- A detected installation does not guarantee that the game can launch.
- Linux requires real-machine validation; macOS is not included in 0.2.0.
