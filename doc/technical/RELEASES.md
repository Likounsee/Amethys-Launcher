# Builds and releases

## Amethys Launcher 0.2.0 — published desktop preview

**Status:** published  
**Publication date:** 23 November 2026  
**Tag:** `v0.2.0`  
**Release page:** https://github.com/Likounsee/Amethys-Launcher/releases/tag/v0.2.0  
**Release workflow:** https://github.com/Likounsee/Amethys-Launcher/actions/runs/45235089145

The 0.2.0 release is the first published desktop preview. It packages the graphical launcher, Core agent, CLI, required fonts and cover assets for Windows x64 and Linux x86_64 GNU.

### Published packages

| Operating system | Target | Archive | Download |
| --- | --- | --- | --- |
| Windows 64-bit | `x86_64-pc-windows-msvc` | ZIP | [Download](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/amethys-v0.2.0-x86_64-pc-windows-msvc.zip) |
| Linux x86_64 (GNU) | `x86_64-unknown-linux-gnu` | TAR.GZ | [Download](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/amethys-v0.2.0-x86_64-unknown-linux-gnu.tar.gz) |

The release also publishes a combined [SHA256SUMS](https://github.com/Likounsee/Amethys-Launcher/releases/download/v0.2.0/SHA256SUMS) file.

Each archive contains:

- `amethys-launcher` — graphical desktop application;
- `amethys-agent` — local Core agent;
- `amethysctl` — command-line client;
- `assets/fonts/` and `assets/covers/`;
- `README.md`, `LICENSE`, `RELEASES.md` and `LIRE-MOI.txt`.

The archives are portable packages, not installers or operating-system service packages. They do not create shortcuts, register a background service or install an automatic updater.

### Published archive checksums

The following SHA-256 values match the assets attached to the published GitHub release and are repeated in its `SHA256SUMS` manifest.

| File | SHA-256 |
| --- | --- |
| `amethys-v0.2.0-x86_64-pc-windows-msvc.zip` | `4ccb3c90f248ca89b8be2d42537bb145763ee6ab67e3b47e3377b327fea955e5` |
| `amethys-v0.2.0-x86_64-unknown-linux-gnu.tar.gz` | `376aef5bd6249315dae46dbc83c21997681cd93af9daff54696c752c1327c7fe` |

Verify the archive before extracting it:

- **Windows PowerShell:** `Get-FileHash .\amethys-v0.2.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256`
- **Linux:** download both the archive and `SHA256SUMS` into the same directory, then run `sha256sum -c SHA256SUMS`.

### Release validation evidence

The dedicated release workflow completed successfully (run `45235089145`) on 23 November 2026. Both platform build jobs and the publish job passed.

Each platform job ran workspace release tests, desktop Clippy with warnings denied, desktop tests, release builds, archive staging and extracted-package smoke checks. The smoke checks verify the packaged CLI version and help output, then start the packaged agent and confirm IPC readiness. The publish job generated the two-entry SHA-256 manifest, verified it and uploaded both archives.

- **Windows:** automated release build and packaged archive smoke checks passed. A separate Windows desktop development package had previously passed 15 manual checks on a real Windows machine. That manually tested development archive is not the same file as the published release ZIP.
- **Linux:** automated release build and packaged archive smoke checks passed. Manual testing on a physical Linux machine remains outstanding.
- **macOS:** not included in 0.2.0.

Automated CI results confirm the tested build and packaging paths, not universal compatibility with all PCs, desktop environments, launchers or games.

### First launch

1. Download the archive for your operating system from the [v0.2.0 release page](https://github.com/Likounsee/Amethys-Launcher/releases/tag/v0.2.0).
2. Extract it to a directory where your user account has read/write access.
3. Launch the graphical application:
   - **Windows:** run `amethys-launcher.exe`.
   - **Linux:** run `./amethys-launcher`. If needed, run `chmod +x amethys-launcher` first.
4. The package also includes the Core agent and CLI. Run `amethysctl.exe --help` on Windows or `./amethysctl --help` on Linux to inspect the available commands.

Keep official platform launchers as the source of truth for game installation, updates, authentication and licences. Amethys does not bypass DRM, licensing, account checks, anti-cheat or platform restrictions.

### Scope and limitations

Version 0.2.0 remains an experimental development preview, not a production-ready launcher. It provides a graphical local library, search, filters, sorting, game details, executable configuration and launching, supported launcher discovery, explicit import, custom covers and asynchronous Core operations.

It does not provide a connected commercial store, prices, promotions, purchases, account management, game downloads or game installation. Cloud saves, cross-device synchronization and automatic updates are also not included. Discovery coverage is adapter-specific, and detecting a game does not guarantee that it will launch. See [launcher discovery coverage](../discovery/LAUNCHER-COVERAGE.md) and [desktop documentation](DESKTOP-PREVIEW.md).

## Amethys Launcher 0.1.0 — first public test milestone

The first public version, `v0.1.0`, was an experimental agent/CLI prototype for Windows and Linux. It provided a Rust background agent, CLI, local IPC, SQLite-backed storage, initial read-only launcher discovery, explicit import and basic local library operations. It did not include a graphical interface.

The 0.1.0 archives were portable test binaries, not installers. They contained the agent, CLI, project README, LICENSE and this release guide. The agent ran in the foreground and had to be started before the CLI could communicate with it.

### First run for 0.1.0 CLI packages

1. Extract the archive to a directory owned by the current user.
2. Open a terminal in that directory and start the agent:
   - **Windows PowerShell:** `./amethys-agent.exe`
   - **Linux:** `./amethys-agent`
3. Keep that terminal open. In a second terminal, check the connection:
   - **Windows PowerShell:** `./amethysctl.exe ping`, then `./amethysctl.exe status`
   - **Linux:** `./amethysctl ping`, then `./amethysctl status`
4. Inspect supported local launcher metadata with `games scan`.
5. Use `games import-all` only when you explicitly want to add all detected installations to the Amethys database.

## Security and privacy

Discovery reads launcher metadata and does not modify game installations or launcher databases. IPC is local-only and must not be exposed over a network.

Windows named-pipe access uses an explicit DACL granting access to the current process user and LocalSystem, while omitting Everyone and anonymous users. Remote clients are rejected and client opens use identification-level QoS. Unix sockets use mode `0600` and are cleaned up using socket inode checks.

These controls restrict the IPC endpoint to the current user context; they do not protect against malicious code already running as that user or a compromised account. Review the [IPC security notes](IPC.md) before testing.

## CI and release automation

The cross-platform quality checks are defined in [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml). Versioned release builds and publication are defined in [`.github/workflows/release.yml`](../../.github/workflows/release.yml). A release tag must match the workspace version in `Cargo.toml`; the workflow validates the tag, tests and builds both targets, checks the extracted packages and publishes the archives with SHA-256 checksums.
