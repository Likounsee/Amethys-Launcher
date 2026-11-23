# Builds and releases

## Amethys 0.1.0 — first public test milestone

The first public version, `v0.1.0`, has been built and published for Windows and Linux. It is an **experimental prototype intended for testing and feedback only**. It is not a finished launcher, not production-ready and not recommended for everyday use.

The release workflow successfully built both targets, ran the release tests, packaged the binaries and generated and verified SHA-256 checksums. This confirms the automated release pipeline; it does not establish broad compatibility with every launcher, game, machine or installation.

## Supported 0.1.0 test packages

| Operating system | Target | Archive |
| --- | --- | --- |
| Windows 64-bit | `x86_64-pc-windows-msvc` | ZIP |
| Linux x86_64 (GNU) | `x86_64-unknown-linux-gnu` | TAR.GZ |

Each archive contains `amethys-agent` (or `.exe`), `amethysctl` (or `.exe`), the project `README.md`, the `LICENSE`, and this guide as `RELEASES.md`. The archives are portable test binaries, not an installer or operating-system service package. The agent must be running before the CLI can communicate with it. Automatic startup, service registration, desktop shortcuts, update delivery and uninstallers are not included.

## Experimental Windows desktop development package

The GhostEngine desktop preview has a separate Windows x64 packaging script. From the repository root, run:

```powershell
./scripts/package-desktop-windows.ps1
```

The script runs workspace and desktop checks/tests, builds the agent, CLI and desktop executable, then stages the application and its required fonts and cover assets under `dist/Amethys/`. It creates `dist/Amethys-Windows-x64.zip`, a per-file `SHA256SUMS.txt` manifest and a `.sha256` checksum for the archive. CI verifies both checksum layers and launches the packaged application in snapshot mode at compact, standard and 4K window sizes, checking that the library and settings pages are captured.

**Windows 0.2 validation:** the portable ZIP was built locally, extracted and launched successfully on a real Windows machine. All 15 manual checks passed. The exact tested archive had SHA-256 `d02e616d99c038d3856f6d2d55840c75b16aeec0a8bab44de8cbb0034306ae59`; a rebuild may produce a different checksum. The successful [CI run 45204643241](https://github.com/Likounsee/Amethys-Launcher/actions/runs/45204643241) also verified the Windows package checksums and desktop snapshots. Linux CI quality and desktop library tests passed, but manual testing of a Linux desktop package is deferred and remains outstanding.

This remains an experimental development preview, separate from the public 0.1.0 CLI archive. It is not an installer or a production-ready release.

**Versioning gate:** “desktop preview 0.2” is currently a development milestone, not yet the repository's official Cargo release version. The workspace package version remains `0.1.0`, and `release.yml` rejects a `v0.2.0` tag until the workspace version is deliberately updated. Do not create or publish a `v0.2.0` release tag until the intended release scope, version bump and release artifacts are agreed and checked.

## First-run notes

1. Extract the archive to a directory owned by the current user.
2. Open a terminal in that extracted directory and start the agent:
   - **Windows PowerShell:** `./amethys-agent.exe`
   - **Linux:** `./amethys-agent`
3. Keep that terminal open. Open a second terminal in the same directory and check the connection:
   - **Windows PowerShell:** `./amethysctl.exe ping`, then `./amethysctl.exe status`
   - **Linux:** `./amethysctl ping`, then `./amethysctl status`
4. Inspect supported local launcher metadata:
   - **Windows PowerShell:** `./amethysctl.exe games scan`
   - **Linux:** `./amethysctl games scan`
5. Use `games import-all` only when you explicitly want to add all detected installations to the Amethys database:
   - **Windows PowerShell:** `./amethysctl.exe games import-all`
   - **Linux:** `./amethysctl games import-all`

The agent currently runs in the foreground. Closing its terminal stops it. The CLI commands must be run from the extracted directory (or with the full executable path). Do not expose the IPC endpoint over a network.

## Known scope and limitations

Version 0.1.0 is a very small first milestone. It provides a Rust agent, a CLI, local IPC, SQLite-backed storage, initial read-only launcher discovery, explicit import and basic local library operations. These foundations are incomplete and should be treated as experimental.

There is no graphical interface, store/catalogue, game download or installation, account system, cloud save synchronization, backup feature, automatic updater, service installer or polished end-user experience. Discovery is metadata-based and does not guarantee that a game is launchable. The project is still being developed; do not present this release as a complete or production-ready product.

## Validation and test evidence

Keep three evidence levels distinct in release notes and bug reports:

1. **Automated CI / fixture-tested:** parser, command, IPC, storage, packaging or security behavior exercised by automated tests. A fixture does not prove compatibility with every installed launcher version.
2. **Real-install validated:** a reproducible scan or operation was checked against an actual game installation and launcher on a real machine. Record only the platforms and cases that were genuinely exercised.
3. **Not yet tested:** no suitable physical machine, launcher account or installed game was available. State this plainly; do not imply that the case passed.

The release pipeline completed successfully for both Windows and Linux: build and release tests passed, platform archives were generated, and SHA-256 checksums were generated and verified. These automated results do not replace real-world testing. Real-install validation is recorded per adapter in [launcher discovery coverage](../discovery/LAUNCHER-COVERAGE.md); untested adapters must remain labelled as pending. macOS is outside the 0.1.0 target and is deferred until a suitable Mac test machine is available.

## Future development

Development continues after this first test milestone. Planned directions include stabilizing the core, expanding automated and real-machine testing, improving launcher compatibility and diagnostics, and progressively designing a more complete user experience. A graphical interface and larger features such as a store/catalogue, downloads, accounts, backups and save synchronization are future possibilities, not part of 0.1.0. Their scope and timing are not guaranteed and may change as the project evolves.

## Security and privacy

Amethys does not bypass DRM, licensing, authentication, anti-cheat or platform restrictions. Discovery reads launcher metadata and does not modify game installations or launcher databases. IPC is local-only and must not be exposed over a network.

Windows named-pipe access uses an explicit DACL granting access to the current process user and LocalSystem, while omitting Everyone and anonymous users. Remote clients are rejected and client opens use identification-level QoS. The Windows CI suite checks the named-pipe peer SID/PID and runs an agent/CLI IPC smoke test. Unix sockets use mode `0600` and are cleaned up using socket inode checks.

These controls restrict the IPC endpoint to the current user context; they do not protect against malicious code already running as that user or a compromised account. Review the [IPC security notes](IPC.md) before testing.
