# Amethys Launcher documentation

This directory contains the project's documentation. The repository root is
kept intentionally small: start with the root [README](../README.md) for the
project overview and [LICENSE](../LICENSE) for usage and ownership terms.

## Technical documentation

- [IPC protocol and transport](technical/IPC.md) — message format, commands,
  endpoints and security considerations.
- [Game discovery](technical/game-discovery.md) — supported launchers,
  detection methods, identifier rules and current limitations.
- [Builds and releases](technical/RELEASES.md) — release targets, archive
  contents, first-run procedure and publication checklist.
- [0.1 pre-release checklist](releases/0.1.0-pre-release-checklist.md) — final
  maintainer checks before publishing the candidate.
- [0.1 draft release notes](releases/0.1.0-pre-release-notes.md) — candidate
  scope, included functionality and known limitations.

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
