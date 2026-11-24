![Amethys Launcher](doc/asset/logo-text.png)

<p align="center">
  <a href="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml"><img src="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <img src="https://img.shields.io/badge/Rust-2024-orange?logo=rust" alt="Rust 2024">
  <img src="https://img.shields.io/badge/Platforms-Windows%20%7C%20Linux-blue" alt="Windows and Linux">
  <img src="https://img.shields.io/badge/Version-0.2.0-blueviolet" alt="Version 0.2.0">
</p>

A free, community-driven, cross-platform game library and launcher client.

Amethys Launcher aims to bring game libraries, installations and platform integrations together without replacing official stores, accounts, licenses, DRM or platform policies.

## Latest release — Amethys Launcher 0.2.0

<https://github.com/Likounsee/Amethys-Launcher/releases/tag/v0.2.0>

**Version 0.2.0 was published on 23 November 2026.** It is the first published desktop preview of Amethys Launcher, available as portable packages for Windows x64 and Linux x86_64.

| Platform | Package | Download |
| --- | --- | --- |
| Windows x64 | ZIP | [Download](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/amethys-v0.2.0-x86_64-pc-windows-msvc.zip) |
| Linux x86_64 (GNU) | TAR.GZ | [Download](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/amethys-v0.2.0-x86_64-unknown-linux-gnu.tar.gz) |
| Checksums | SHA-256 | [SHA256SUMS](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/SHA256SUMS) |

### What is included

- Graphical desktop launcher connected to Amethys Core through the versioned IPC protocol.
- Local game library with search, platform filters, sorting and game details.
- Executable configuration and launching of configured local games.
- Supported launcher discovery and explicit import of detected installations.
- Custom game covers, responsive layout, keyboard navigation and asynchronous loading/operation feedback.
- Core agent, CLI, fonts and cover assets in both portable release archives.

See the [0.2.0 release notes](doc/releases/0.2.0-release-notes.md) for installation instructions, archive checksums, validation evidence and known limitations.

**Important:** 0.2.0 is an experimental development preview, not a finished or production-ready launcher. The archives are portable packages, not installers. Linux passed automated release validation but has not yet been manually tested on a physical Linux machine. macOS is not supported by this release.

## Previous release — 0.1.0

Version 0.1.0 was the first public technical milestone. It provided the Rust background agent and command-line client, local IPC, SQLite-backed storage, initial read-only launcher discovery, explicit import and basic local library operations. It did not include the graphical desktop application.

The 0.1.0 packages and their original limitations are documented in [the 0.1.0 release notes](doc/releases/0.1.0-pre-release-notes.md).

## Global game catalogue

The public master game catalogue is maintained separately in [Amethys-Catalog](https://github.com/Likounsee/Amethys-Catalog). Its static catalogue endpoint is designed to distribute a compact search index and on-demand detail shards; per-PC installations remain private in the launcher database.

## Development roadmap — 0.3

The 0.2 release is frozen. The next development cycle is Amethys Launcher 0.3: a real catalogue, authorized distribution sources, persistent download/install operations, managed installations, updates and repairs. The [0.3 roadmap](ROADMAP-0.3.md), [complete baseline audit](doc/technical/LOT-1-AUDIT-0.3.md) and [target architecture](doc/technical/ARCHITECTURE-0.3.md) define the implementation order and boundaries. The global Quality Check campaign is deferred to 0.4.

## Project status and scope

The 0.2.0 desktop application is built on the existing Rust workspace and the vendored GhostEngine UI preview. The desktop client communicates with Amethys Core through IPC; Core remains the owner of persistent game-library data and its SQLite database.

The 0.2 release does **not** include a connected commercial store, prices, promotions, purchases, account management, game downloads or game installation. Cloud saves, synchronization, automatic updates and a finished end-user experience are also outside this release. Future plans are tracked in [ROADMAP-0.2.md](ROADMAP-0.2.md).

Detection coverage varies by launcher, and a detected installation is not proof that a game can launch. See [launcher discovery coverage](doc/discovery/LAUNCHER-COVERAGE.md) for adapter evidence and the [0.2 compatibility report](doc/technical/COMPATIBILITY-REPORT-0.2.md) for unresolved executable, launcher and anti-cheat cases. Keep official launchers as the source of truth for installation, updates, authentication and licences.

## Principles

- Windows and Linux are the initial supported test targets. macOS is deferred until a Mac test machine is available.
- The core remains independent from any graphical interface.
- Official platform APIs and permitted mechanisms only; no DRM, licensing, authentication or anti-cheat circumvention.
- SQLite is the initial local database.
- Core/worker communication uses a versioned JSON + Serde protocol.
- No `unsafe` code in Amethys-owned Rust source.
- Errors must be explicit, identifiable and diagnosable.
- Tests and CI are required on every supported operating system.

## Build and test

Install the Rust toolchain specified by `rust-toolchain.toml`, including the `rustfmt` and `clippy` components, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --all-targets
```

The desktop preview can be run from a source checkout with:

```sh
cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bin amethys-launcher
```

See the [documentation index](doc/README.md) for technical details, including [builds and releases](doc/technical/RELEASES.md), [desktop preview architecture](doc/technical/DESKTOP-PREVIEW.md), [IPC](doc/technical/IPC.md) and [game discovery](doc/technical/game-discovery.md).

## Ownership, contributions and rewards

Amethys Launcher and the original project materials are currently owned by **Likounsee**. The project is proprietary; public repository visibility does not grant permission to reuse its code. See [LICENSE](LICENSE).

Community participation is handled through **GitHub Issues only**. Anyone can report bugs, propose features, provide technical analysis or suggest possible means of resolution. Community members must not submit code, patches or Pull Requests; the maintainer is responsible for implementing accepted Issues. People whose reports or proposals materially help the project may be credited in [community contributors](doc/community/CONTRIBUTORS.md). See [contribution policy](.github/CONTRIBUTING.md) for the full policy.

The project intends to establish a **gift-card reward programme** when Amethys Launcher is completed with its Store and integrated wallets. Rewards are intended to reflect meaningful community involvement, such as useful bug reports, technical investigations and accepted proposals. Eligibility, assessment criteria, available budget, gift-card types and distribution terms will be published before awards are determined. See [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md). This programme is a project intention, not a promise of a specific amount or payment date.

Official collaborations with Steam, Epic Games and other platform companies are handled under separate written agreements. Those companies may retain intellectual-property rights in code or materials they provide, as set out in those agreements.

Community members should use Issues rather than Pull Requests. Code implementation and repository changes are handled by the maintainer. See [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md) for details and [security policy](.github/SECURITY.md) for private vulnerability reporting.

Never publish credentials, personal data, private platform responses or confidential information from Steam, Epic Games or other services in this repository.
