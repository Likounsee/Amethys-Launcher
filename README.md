![Amethys Launcher](doc/asset/logo-text.png)

<p align="center">
  <a href="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml"><img src="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <img src="https://img.shields.io/badge/Rust-2024-orange?logo=rust" alt="Rust 2024">
  <img src="https://img.shields.io/badge/Platforms-Windows%20%7C%20Linux%20%7C%20macOS-blue" alt="Windows, Linux and macOS">
</p>

A free, community-driven, cross-platform game library and launcher client.

Amethys Launcher aims to bring game libraries, installations, updates, saves and platform integrations together without replacing the official stores, accounts, licenses, DRM or platform policies.

## Project status

The project is at **Core Foundation 0.1**. The Rust foundation is being built alongside an early, read-only multi-platform game discovery system. The graphical interface and online store/catalogue come later.

## Principles

- Windows, Linux and macOS are first-class targets.
- The core remains independent from any graphical interface.
- Official platform APIs and permitted mechanisms only; no DRM, licensing, authentication or anti-cheat circumvention.
- SQLite is the initial local database.
- Core/worker communication uses a versioned JSON + Serde protocol.
- No `unsafe` code in Amethys-owned Rust source.
- Errors must be explicit, identifiable and diagnosable.
- Tests and CI are required on every supported operating system.

## Initial development scope

1. Rust workspace and project conventions.
2. Core runtime and lifecycle.
3. Typed internal event system.
6. Cross-platform IPC protocol foundation.
7. Module, task and worker management foundations.
8. SQLite storage and configuration.
9. Structured logging, error codes and diagnostics.
10. Read-only discovery of installed games from multiple PC launchers.

The desktop GUI, Amethys online store/catalogue, downloads, save synchronization and account system are intentionally out of scope for this foundation stage. Local game discovery is a separate feature and is part of the current development.

## Repository

This repository is currently named `Amethys-Launcher`. The project/product name is **Amethys Launcher**; the repository name can be changed separately later if desired.

## License and contributions

Amethys Launcher is proprietary software. All rights are reserved by Likounsee,
except for third-party materials governed by their own licenses. Public
visibility does not grant permission to copy, reuse, modify or redistribute
project materials. See [LICENSE](LICENSE) for the terms.

Bug reports and proposed fixes are welcome. Pull requests are reviewed before
any merge, including checks for source-code provenance, copyright, security,
privacy and platform compliance. Code contributions require an explicit
policy acknowledgement and a signed, contribution-specific rights assignment
agreement before they can be merged. See [CONTRIBUTING.md](CONTRIBUTING.md),
[CLA.md](CLA.md), and [SECURITY.md](SECURITY.md).

Never publish credentials, personal data, private platform responses or
confidential information from Steam, Epic Games or other services in this
repository.
