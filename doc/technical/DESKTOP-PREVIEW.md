# Desktop preview — development 0.2

The desktop application is an **unreleased development preview**. It is not part of the published 0.1.0 package and must not be presented as a stable or production-ready release.

## Architecture

- The desktop UI is rendered with the vendored GhostEngine snapshot in `vendor/ghostengine`.
- The desktop client communicates with Amethys Core through the existing versioned IPC protocol.
- Core remains the only owner of persistent game-library data and its SQLite database. The desktop application does not open the database directly.
- Desktop-only preferences and imported media are stored in the user's local Amethys data directory.

## Current preview capabilities

- Browse the locally registered game library, search game metadata, filter by platform and sort entries.
- Persist the selected library sort order on the current device.
- Inspect a game's launch status and configure its executable path.
- Launch a configured executable, remove a local library entry and customize or reset its cover.
- Discover supported installed games and explicitly import detected installations.
- Resize the window, use responsive layouts, navigate actionable controls with Tab / Shift+Tab / Enter, and use the documented Escape and F11 shortcuts.
- Load and refresh the Core library asynchronously so IPC requests do not block the window's UI thread.
- Run imports, executable-path updates and library removals as background Core operations, with visible completion and error feedback.
- Display explicit loading, connection, error, confirmation and notification states.

The preview does not download or install games, replace official launchers, manage platform accounts or licenses, bypass DRM, or provide a finished store/catalogue. Discovery and launch readiness remain limited to the adapters and checks implemented by the project.

## Run from a source checkout

From the repository root, with the stable Rust toolchain installed:

```powershell
$env:CARGO_TARGET_DIR = "$PWD\target"
cargo run --release --manifest-path .\prototypes\ghostengine-amethys-preview\Cargo.toml --bin amethys-launcher
```

The application uses the existing local Amethys Core service and its data directory. Keep official launchers as the source of truth for installation, updates, authentication and licenses.

## Validation

Run the desktop library tests:

```powershell
cargo test --no-default-features --manifest-path .\prototypes\ghostengine-amethys-preview\Cargo.toml --lib
```

Run the desktop integration checks:

```powershell
cargo check --manifest-path .\prototypes\ghostengine-amethys-preview\Cargo.toml
cargo test --manifest-path .\prototypes\ghostengine-amethys-preview\Cargo.toml --lib
```

The CI workflow also builds and smoke-tests the portable Windows desktop package and captures the library and settings pages at compact, standard and 4K window sizes. A passing automated check only validates the scenarios exercised by that workflow; it is not proof of compatibility with every Windows machine, launcher or game installation.

## Packaging

On Windows PowerShell, run:

```powershell
.\scripts\package-desktop-windows.ps1
```

The script runs formatting, Clippy, tests and release builds before staging the portable package. It writes per-file SHA-256 checksums and a checksum for the final ZIP archive. The output is a development package, not an installer or a public stable release.
