# Amethys Launcher documentation

This directory contains the project's documentation. Start with the root [README](../README.md) for the project overview and [LICENSE](../LICENSE) for usage and ownership terms.

## Current release — 0.2.0

- [Amethys Launcher 0.2.0 release notes](releases/0.2.0-release-notes.md) — published downloads, installation, SHA-256 hashes, features, validation evidence and limitations.
- [Builds and releases](technical/RELEASES.md) — release history, package contents, first-run guidance and release pipeline.
- [Desktop preview 0.2](technical/DESKTOP-PREVIEW.md) — desktop architecture, capabilities, source build, validation and packaging details.

## Development roadmap — 0.3

- [0.3 development roadmap](../ROADMAP-0.3.md) — fixed order, dependencies, scope, acceptance criteria and deferred 0.4 quality campaign.
- [Lot 1 audit](technical/LOT-1-AUDIT-0.3.md) — full inventory of the existing 0.2 codebase and gaps.
- [Lot 0 architecture](technical/ARCHITECTURE-0.3.md) — target architecture, data contracts, IPC, persistence and security rules.

## Previous release — 0.1.0

- [0.1.0 first public release notes](releases/0.1.0-pre-release-notes.md) — original experimental CLI milestone, included foundations and limitations.

## Product and technical documentation

- [Product roadmap](../ROADMAP-0.2.md) — delivered 0.2 scope, remaining work and future directions.
- [IPC protocol and transport](technical/IPC.md) — message format, commands, endpoints and security considerations.
- [Game discovery](technical/game-discovery.md) — supported launchers, detection methods, identifier rules and current limitations.
- [Launcher discovery coverage](discovery/LAUNCHER-COVERAGE.md) — evidence and real-install validation status for each adapter.
- [Game compatibility](technical/GAME-COMPATIBILITY.md) — long-term compatibility architecture and security boundaries.
- [0.2 compatibility and validation report](technical/COMPATIBILITY-REPORT-0.2.md) — evidence levels, real-install limitations, anti-cheat boundaries and publisher-collaboration needs.

## Community

- [Community recognition](community/CONTRIBUTORS.md) — people recognised for useful Issues, technical investigations, proposals and validation help.
- [Contribution policy](../.github/CONTRIBUTING.md) — community participation through Issues only.
- [Security policy](../.github/SECURITY.md) — private vulnerability reporting.

## Assets

- `asset/` — project logos and visual assets.

## CI and release automation

GitHub Actions workflows must remain in `.github/workflows/` for GitHub to discover and execute them. Cross-platform checks are defined in [ci.yml](../.github/workflows/ci.yml); release packaging and publication are defined in [release.yml](../.github/workflows/release.yml).
