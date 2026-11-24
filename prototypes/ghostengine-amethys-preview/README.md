# Amethys Launcher — GhostEngine desktop integration (development)

> First working integration milestone. This application uses the current experimental GhostEngine window/rendering prototype. It is not a stable GhostEngine integration or a production release.

This folder is isolated from the main Cargo workspace and uses the GhostEngine source snapshot vendored in this repository at `vendor/ghostengine`. A separate GhostEngine checkout is not required. Amethys connects to the existing Core through the real local IPC protocol.

## Repository layout

Only this repository is needed:

```text
Amethys-Launcher/
├── crates/
├── prototypes/ghostengine-amethys-preview/
└── vendor/ghostengine/
    ├── Cargo.toml
    ├── examples/
    └── src/
```

The vendored engine snapshot was imported from `Likounsee/GhostEngine` branch `GhostEngine`, commit `dd402cd44b53b8a8182910d7efdf4b1c51ed751b`. Its MIT and Apache-2.0 license texts are included beside the crate manifest. This is a source snapshot, not a Git submodule or a live link: later GhostEngine development does not silently change Amethys builds. Updating the snapshot is an explicit maintenance task.

Cargo still downloads the engine's third-party crates from crates.io on the first build; no second Git repository is needed.

## Run on Windows

From a PowerShell terminal in the Amethys-Launcher repository, build the agent and desktop app into the same target directory:

```powershell
$env:CARGO_TARGET_DIR = "$PWD/target"
cargo build --release -p amethys-agent
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bin amethys-launcher
```

The desktop application first tries the existing Core IPC endpoint. If it cannot connect, it starts the bundled `amethys-agent.exe` beside itself and waits for the agent to become ready.

The desktop application reads the library through the Amethys Core named pipe `\\.\pipe\amethys-core`. On Unix, it uses the socket `amethys.sock` in the Amethys data directory. It does not open the SQLite database itself.

If the agent is not running or the IPC connection fails, the UI reports the connection error instead of showing invented library entries.

## Implemented against real Amethys services

- The Library page requests `ListGames` through the existing versioned IPC protocol and opens a real details page for each saved entry.
- Game names, source platform, IDs and install paths come from the real local SQLite-backed library.
- Installation-directory presence is checked against the actual filesystem.
- The Refresh button re-queries the real library over IPC.
- The `Détecter les jeux` action runs the existing read-only platform discovery service on a background thread, so scanning does not freeze the UI, then displays installations that are not already in the library.
- Each detected installation can be imported individually through the Core `AddDetectedGame` IPC command; Core retains ownership of persistence and deduplicates repeated imports.
- The search field filters real saved metadata (name, platform, platform ID, install path and executable); platform filters are generated from the platforms actually present in Core.
- The game details page can update the executable path through the Core `SetGameExecutable` IPC command; launch remains disabled until the configured path resolves to a file inside the registered game directory.
- Configured executables can be launched from the UI after canonical-path validation confines them to the registered game directory.
- Library entries can be removed through the Core-owned SQLite service; this removes only the Amethys entry, never the game files.
- Core connection state is shown in Settings.
- The Store page explicitly reports that no catalogue provider is connected yet. It does not show sample games, prices, discounts or fake purchase links.
- Inactive navigation entries and fake recently played games have been removed/hidden.

## Not implemented yet

Game launching is currently performed by the desktop client after it retrieves the saved executable metadata; it is not yet a Core IPC command. The current Core protocol does not expose playtime tracking, download/install management, store catalogue queries, wishlists, platform account linking or settings mutation. Those features need real services before they can be exposed as functional controls.

The library is loaded at startup and can be refreshed manually. GUI import is now available for platforms covered by `amethys-discovery`, and discovery runs outside the UI thread. Playtime tracking, live synchronization, and broader library management remain to be implemented. The existing CLI remains available.

## Temporary GhostEngine boundary

The window currently relies on the prototype's `native-window` feature (Winit/Softbuffer) and CPU framebuffer. This is deliberately temporary and must be replaced when GhostEngine's public API and platform backends are redesigned. Amethys page appearance and interaction design should be preserved across that migration; internal rendering and native-window integration are not contractual.

The vendored snapshot is intentionally pinned inside this repository so the launcher can be built from a single checkout. When GhostEngine's architecture changes, the integration can be updated by importing a reviewed new snapshot while keeping Amethys page and Core logic separate.

## Build and validation

From the Amethys-Launcher repository root:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo check --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
cargo test --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --lib
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
```

Create a portable Windows x64 package from the repository root with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package-desktop-windows.ps1
```

The script builds the desktop launcher, Core agent and `amethysctl` CLI, stages the fonts/covers beside the launcher and creates `dist/Amethys-Windows-x64.zip`. The GUI can detect and import installations covered by the discovery service. This is a portable ZIP, not an MSI installer. The prototype has not yet been built or run on the user's PC. Its fixed 1600×900 page layout is still a limitation; responsive layout is a follow-up task.
