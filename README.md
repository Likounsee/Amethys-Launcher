![Amethys Launcher](doc/asset/logo-text.png)

<p align="center">
  <a href="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml"><img src="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <img src="https://img.shields.io/badge/Rust-2024-orange?logo=rust" alt="Rust 2024">
  <img src="https://img.shields.io/badge/Platforms-Windows%20%7C%20Linux-blue" alt="Windows, Linux and macOS">
</p>

A free, community-driven, cross-platform game library and launcher client.

Amethys Launcher aims to bring game libraries, installations, updates, saves and platform integrations together without replacing the official stores, accounts, licenses, DRM or platform policies.

## Project status

The project is at **Core Foundation 0.1**. The Rust foundation is being built alongside an early, read-only multi-platform game discovery system. The graphical interface and online store/catalogue come later.

## Principles

- Windows and Linux are the supported targets for version 0.1. macOS development is paused until a Mac test machine is available.
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
4. Cross-platform IPC protocol foundation.
5. Module, task and worker management foundations.
6. SQLite storage and configuration.
7. Structured logging, error codes and diagnostics.
8. Read-only discovery of installed games from multiple PC launchers.
9. Explicit import, local library management and read-only path verification.
10. Cross-platform CI and release archive generation.

The desktop GUI, Amethys online store/catalogue, downloads, save synchronization and account system are intentionally out of scope for this foundation stage.

## Build and test

Install the stable Rust toolchain with the `rustfmt` and `clippy` components, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --all-targets
```

The CI workflow runs these quality checks on Windows and Linux.

See [the documentation index](doc/README.md) for technical details, including [builds and releases](doc/technical/RELEASES.md).

## Ownership, contributions and rewards

Amethys Launcher and the original project materials are currently owned by **Likounsee**. The project is proprietary; public repository visibility does not grant permission to reuse its code. See [LICENSE](LICENSE).

Community participation is handled through **GitHub Issues only**. Anyone can report bugs, propose features, provide technical analysis or suggest possible means of resolution. Community members must not submit code, patches or Pull Requests; the maintainer is responsible for implementing accepted Issues. People whose reports or proposals materially help the project may be credited in [community contributors](doc/community/CONTRIBUTORS.md). See [contribution policy](.github/CONTRIBUTING.md) for the full policy.

The project intends to establish a **gift-card reward programme** when Amethys Launcher is completed with its Store and integrated wallets. Rewards are intended to reflect meaningful community involvement, such as useful bug reports, technical investigations and accepted proposals. Eligibility, assessment criteria, available budget, gift-card types and distribution terms will be published before awards are determined. See [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md). This programme is a project intention, not a promise of a specific amount or payment date.

Official collaborations with Steam, Epic Games and other platform companies are handled under separate written agreements. Those companies may retain intellectual-property rights in code or materials they provide, as set out in those agreements.

Community members should use Issues rather than Pull Requests. Code implementation and repository changes are handled by the maintainer. See [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md) for details and [security policy](.github/SECURITY.md) for private vulnerability reporting.

Never publish credentials, personal data, private platform responses or confidential information from Steam, Epic Games or other services in this repository.
