# Amethys Launcher — GhostEngine integration prototype

> Archived integration prototype copied from GhostEngine. This is a UI/rendering experiment, not the production launcher and not part of the main Cargo workspace.

This prototype is intentionally isolated from Amethys' agent, CLI, storage and orchestration crates. It demonstrates what the current GhostEngine revision can render and handle in an interactive desktop UI.

## Local development

The manifest uses a local path dependency and expects the two repositories to be checked out side by side:

```text
workspace-parent/
├── Amethys-Launcher/
│   └── prototypes/ghostengine-amethys-preview/
└── GhostEngine/
    └── ghostengine/crates/ghostengine/
```

From the Amethys-Launcher repository root:

```sh
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml -- --snapshot target/amethys-preview
```

This local path dependency is only for the isolated development prototype. It does **not** change the production dependency policy: the actual Amethys application must consume only an official public GhostEngine release, pinned to a release tag/version and committed `Cargo.lock`. GhostEngine currently has no published GitHub releases, so there is no stable release to pin yet.

## What the prototype demonstrates

- Four mock pages: Store, Library, Game detail and Settings.
- Scrolling, hover/click navigation, filters and breadcrumb navigation.
- GhostEngine text input, keyboard input, IME and clipboard integration.
- PNG/JPEG/BMP image decoding, texture cache and mipmaps.
- Inter fonts (OFL licence included in `assets/fonts/LICENSE-Inter.txt`).
- Live rendering measurements and an on-demand benchmark.
- A headless scripted session that saves screenshots.

The prices, recommendations, library entries, playtime and store information are sample data. The prototype does not connect to Amethys' real agent, database, discovery, IPC or game-launching logic. It is not a production-ready launcher UI.

## Known limitations

- The prototype uses a fixed 1600×900 logical layout; native framebuffer resizing does not yet provide responsive page layout.
- The window/input integration currently relies on GhostEngine's `native-window` feature and its Winit/softbuffer/arboard dependencies.
- The renderer is CPU-based. No GPU backend is included or required.
- The current GhostEngine API is a single crate and is not yet the final cross-platform modular API.
- The prototype has not been validated here by building/running it on Windows, Linux and macOS.

