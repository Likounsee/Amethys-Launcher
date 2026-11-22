# Amethys Launcher — GhostEngine desktop integration (development)

> First working integration milestone. This application uses the current experimental GhostEngine window/rendering prototype. It is not a stable GhostEngine integration or a production release.

This folder is isolated from the main Cargo workspace. It uses a local path dependency on the sibling GhostEngine checkout and connects to the existing Amethys Core through the real local IPC protocol.

## Repository layout

The two repositories must be checked out side by side:

```text
workspace-parent/
├── Amethys-Launcher/
│   └── prototypes/ghostengine-amethys-preview/
└── GhostEngine/
    └── ghostengine/crates/ghostengine/
```

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
- The search field filters real saved metadata (name, platform, platform ID, install path and executable); platform filters are generated from the platforms actually present in Core.
- The game details page can update the executable path through the Core `SetGameExecutable` IPC command; launch remains disabled until the configured path resolves to a file inside the registered game directory.
- Configured executables can be launched from the UI after canonical-path validation confines them to the registered game directory.
- Library entries can be removed through the Core-owned SQLite service; this removes only the Amethys entry, never the game files.
- Core connection state is shown in Settings.
- The Store page explicitly reports that no catalogue provider is connected yet. It does not show sample games, prices, discounts or fake purchase links.
- Inactive navigation entries and fake recently played games have been removed/hidden.

## Not implemented yet

Game launching is currently performed by the desktop client after it retrieves the saved executable metadata; it is not yet a Core IPC command. The current Core protocol does not expose playtime tracking, download/install management, store catalogue queries, wishlists, platform account linking or settings mutation. Those features need real services before they can be exposed as functional controls.

The library is loaded at startup and can be refreshed manually. Add/import flows inside the GUI, playtime tracking and live synchronization remain to be implemented. Games can still be imported through the existing Amethys CLI.

## Temporary GhostEngine boundary

The window currently relies on the prototype's `native-window` feature (Winit/Softbuffer) and CPU framebuffer. This is deliberately temporary and must be replaced when GhostEngine's public API and platform backends are redesigned. Amethys page appearance and interaction design should be preserved across that migration; internal rendering and native-window integration are not contractual.

The production launcher must eventually depend on an official GhostEngine release, pinned to its release tag/version and committed `Cargo.lock`. This prototype's local path dependency is only for development.

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

The script builds the desktop launcher, Core agent and `amethysctl` CLI, stages the fonts/covers beside the launcher and creates `dist/Amethys-Windows-x64.zip`. The CLI can be used to import/manage games until the GUI import flow is implemented. This is a portable ZIP, not an MSI installer. The prototype has not yet been built or run on the user's PC as part of this change. Its fixed 1600×900 page layout is still a limitation; responsive layout is a follow-up task.
