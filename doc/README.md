# Amethys Launcher documentation

This directory contains the project's documentation. The repository root is
kept intentionally small: start with the root [README](../README.md) for the
project overview and [LICENSE](../LICENSE) for usage and ownership terms.

## Version 0.1.0

- [0.1.0 first public release notes](releases/0.1.0-pre-release-notes.md) —
  experimental status, included foundations, limitations and future directions.
- [Builds and releases](technical/RELEASES.md) — Windows/Linux test packages,
  first-run procedure, validation evidence and release information.

## Desktop development preview

- [Desktop preview 0.2](technical/DESKTOP-PREVIEW.md) — architecture, current capabilities, run instructions, validation and packaging. This preview is unreleased and separate from 0.1.0.

## Technical documentation

- [IPC protocol and transport](technical/IPC.md) — message format, commands,
  endpoints and security considerations.
- [Game discovery](technical/game-discovery.md) — supported launchers,
  detection methods, identifier rules and current limitations.
- [Launcher discovery coverage](discovery/LAUNCHER-COVERAGE.md) — evidence and
  real-install validation status for each adapter.

## Community

- [Community recognition](community/CONTRIBUTORS.md) — people recognised for
  useful Issues, technical investigations, proposals and validation help.
- [Contribution policy](../.github/CONTRIBUTING.md) — community participation
  through Issues only.
- [Security policy](../.github/SECURITY.md) — private vulnerability reporting.

## Assets

- `asset/` — project logos and visual assets.

## CI

GitHub Actions workflows must remain in `.github/workflows/` for GitHub to
discover and execute them. The cross-platform checks are defined in
[ci.yml](../.github/workflows/ci.yml); release packaging is defined in
[release.yml](../.github/workflows/release.yml).
