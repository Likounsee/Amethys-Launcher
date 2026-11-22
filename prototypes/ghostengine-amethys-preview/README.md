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

Open a first PowerShell terminal in the Amethys-Launcher repository and start the real core agent:

```powershell
cargo run --release -p amethys-agent
```

Keep that terminal running. In a second terminal, launch the desktop application:

```powershell
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
```

The desktop application reads the library through the Amethys Core named pipe `\\.\pipe\amethys-core`. On Unix, it uses the socket `amethys.sock` in the Amethys data directory. It does not open the SQLite database itself.

If the agent is not running or the IPC connection fails, the UI reports the connection error instead of showing invented library entries.

## Implemented against real Amethys services

- The Library page requests `ListGames` through the existing versioned IPC protocol.
- Game names, source platform, IDs and install paths come from the real local SQLite-backed library.
- Installation-directory presence is checked against the actual filesystem.
- Core connection state is shown in Settings.
- The Store page explicitly reports that no catalogue provider is connected yet. It does not show sample games, prices, discounts or fake purchase links.
- Inactive navigation entries, fake recently played games and the mock search field have been removed/hidden.

## Not implemented yet

The current Amethys Core IPC protocol does not expose game launching, playtime tracking, download/install management, store catalogue queries, wishlists, platform account linking or settings mutation. The UI does not pretend those features work. They need real Core services and IPC commands before they can be exposed as functional controls.

The current first milestone also loads the library once at application startup. Live refresh, add/remove actions, real search, selected-game detail pages and launch actions remain to be implemented.

## Temporary GhostEngine boundary

The window currently relies on the prototype's `native-window` feature (Winit/Softbuffer) and CPU framebuffer. This is deliberately temporary and must be replaced when GhostEngine's public API and platform backends are redesigned. Amethys page appearance and interaction design should be preserved across that migration; internal rendering and native-window integration are not contractual.

The production launcher must eventually depend on an official GhostEngine release, pinned to its release tag/version and committed `Cargo.lock`. This prototype's local path dependency is only for development.

## Build and validation

From the Amethys-Launcher repository root:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo check --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
```

The prototype has not yet been built or run on the user's PC as part of this change. Its fixed 1600×900 page layout is also still a limitation; responsive layout is a follow-up task.
