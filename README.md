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

## Ownership, contributions and rewards

Amethys Launcher and the original project materials are currently owned by **Likounsee**. The project is proprietary; public repository visibility does not grant permission to reuse its code. See [LICENSE](LICENSE).

Individual contributors are welcome. Before proposing code, read [CONTRIBUTING.md](CONTRIBUTING.md), identify the files you intend to add or change, and open an Issue if you need specifications or further information. Code is not merged until its origin and rights have been checked, the contribution-specific written assignment has been signed, and the maintainer has accepted it. Contributors whose work is accepted are credited in [CONTRIBUTORS.md](CONTRIBUTORS.md), including the files or features they worked on where practical.

The project intends to establish a **gift-card reward programme** when Amethys Launcher is completed with its Store and integrated wallets. Rewards are intended to reflect each contributor's involvement and accepted work. Eligibility, assessment criteria, available budget, gift-card types and distribution terms will be published before awards are determined. See [CONTRIBUTING.md](CONTRIBUTING.md). This programme is a project intention, not a promise of a specific amount or payment date.

Official collaborations with Steam, Epic Games and other platform companies are handled under separate written agreements. Those companies may retain intellectual-property rights in code or materials they provide, as set out in those agreements.

Pull requests are reviewed before any merge, including checks for source-code provenance, copyright, security, privacy and platform compliance. See [CLA.md](CLA.md) for the individual contribution agreement and [SECURITY.md](SECURITY.md) for private vulnerability reporting.

Never publish credentials, personal data, private platform responses or confidential information from Steam, Epic Games or other services in this repository.
