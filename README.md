![Amethys Launcher](doc/asset/logo-text.png)

<p align="center">
  <a href="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml"><img src="https://github.com/Likounsee/Amethys-Launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <img src="https://img.shields.io/badge/Rust-2024-orange?logo=rust" alt="Rust 2024">
  <img src="https://img.shields.io/badge/Platforms-Windows%20%7C%20Linux-blue" alt="Windows and Linux">
</p>

A free, community-driven, cross-platform game library and launcher client.

Amethys Launcher aims to bring game libraries, installations and platform integrations together without replacing official stores, accounts, licenses, DRM or platform policies.

## Project status — first public version 0.1.0

**Amethys Launcher 0.1.0 is the very first public version of the project. It is an experimental technical prototype released for testing, feedback and early evaluation only.**

**The project is not finished and is not yet a complete, reliable or production-ready launcher. It is not intended for everyday use or to manage a user's full game library.** The current release is a small first milestone: it makes a limited part of the technical foundation available so that the basic architecture and workflows can be tried on Windows and Linux.

This first version mainly provides:
- a Rust-based background agent and command-line client (there is no graphical interface);
- local communication between the agent and CLI;
- a local SQLite database and configuration;
- initial, read-only detection of game installations from selected launchers;
- explicit import and basic local library operations;
- read-only path/executable checks and launching of an explicitly configured executable;
- portable Windows and Linux test packages.

These features are partial and have important limitations. Detection coverage varies by launcher, and a detected installation is not a guarantee that a game will launch correctly. Automated builds and tests confirm only the cases covered by CI and fixtures; they do not establish broad compatibility or production readiness. See [launcher discovery coverage](doc/discovery/LAUNCHER-COVERAGE.md) for the exact evidence and pending tests.

The 0.1.0 release does **not** include a graphical interface, a store/catalogue, game downloading or installation, account management, cloud saves, backups, automatic updates, or a finished end-user experience. The portable archives are test binaries, not installers. Use them only if you are comfortable testing early software and reporting issues. Keep official launchers as the source of truth for installation, updates, authentication and licenses.

### What comes next

Development is continuing beyond this first milestone. Planned work includes consolidating and hardening the core, expanding real-world testing and launcher compatibility, improving reliability and diagnostics, and progressively designing the future user experience and larger features such as a graphical interface and store/catalogue. These are future development directions, not features promised by 0.1.0; priorities and scope may evolve as the project is tested.

## Current development — desktop preview 0.2 (unreleased)

The published 0.1.0 release remains the experimental agent/CLI milestone described above. The `main` branch is also developing a separate, unreleased desktop preview under `prototypes/ghostengine-amethys-preview`. It uses the vendored GhostEngine UI and communicates with the existing Amethys Core through IPC; Core remains the owner of persistent game-library data.

The preview currently includes a responsive game library, search and platform filters, sorting with a locally persisted preference, game details and executable configuration, launching and removal of local entries, cover customization, supported installation discovery and explicit import, keyboard navigation, and asynchronous Core library loading, refresh, imports, executable updates and removals. It remains experimental and is not included in the 0.1.0 release archives.

The Windows x64 portable package has passed all 15 manual checks, including launch from the extracted ZIP. The 0.2 desktop release target is Windows x64 and Linux x86_64; CI has been extended to build and smoke-test the Linux desktop archive, but that new check still needs a successful run. Manual Linux testing is intentionally deferred. See [Desktop preview 0.2](doc/technical/DESKTOP-PREVIEW.md) for build instructions, validation evidence, the tested Windows archive checksum and packaging details. This remains an experimental development preview, not a finished or production-ready launcher.

## Principles

- Windows and Linux are the initial supported test targets. macOS is deferred until a Mac test machine is available.
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

The desktop GUI, Amethys online store/catalogue, downloads, save synchronization and account system are intentionally outside the first 0.1.0 milestone.

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
