# Builds and releases

## Supported 0.1 release targets

The release workflow builds the CLI and core agent as native Rust executables:

| Operating system | Target | Archive |
| --- | --- | --- |
| Windows 64-bit | `x86_64-pc-windows-msvc` | ZIP |
| Linux x86_64 (GNU) | `x86_64-unknown-linux-gnu` | TAR.GZ |

Each archive contains `amethys-agent` (or `.exe`), `amethysctl` (or `.exe`), the project `README.md`, the `LICENSE`, and this release/first-run guide as `RELEASES.md`. The archives are portable binaries, not an installer or operating-system service package. The agent must be running before the CLI can communicate with it. Automatic startup, service registration, desktop shortcuts, update delivery and uninstallers are not part of this initial release package.

## Creating a draft release

Push a version tag such as `v0.1.0`:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds both targets, archives the two executables for each target, calculates SHA-256 checksums, and creates a **draft** GitHub release. The maintainer must inspect and manually publish the draft; a successful build does not automatically make a release public.

The workflow can also be started manually for an existing tag through the GitHub Actions interface. Before building, it verifies that the tag exists and exactly matches the workspace version in `Cargo.toml`; this prevents publishing binaries under a version label that differs from the compiled project version.

## First-run notes

1. Extract the archive to a directory owned by the current user.
2. Start `amethys-agent` (Windows: `amethys-agent.exe`) in a terminal.
3. In another terminal, run `amethysctl ping` and `amethysctl status`.
4. Run `amethysctl games scan` to inspect supported local launcher metadata.
5. Use `amethysctl games import-all` only when you want to add all detected installations to the Amethys database.

The agent currently runs in the foreground. Closing its terminal stops it. Do not expose the IPC endpoint over a network.

## Validation before publication

A release candidate is not ready merely because compilation succeeded. Before publishing a draft, check:

- the CI workflow is green on Windows and Linux;
- each archive contains both expected executables;
- `amethysctl --version` and `amethysctl --help` work;
- the agent starts and responds to `ping` and `status`;
- scan, import, list, search and read-only verify behave as documented;
- the archive and checksum files can be downloaded and verified on Windows and Linux.

macOS is temporarily out of scope for the 0.1 release because it cannot currently be tested on physical Apple hardware. Its development can resume when a suitable Mac test machine is available.

Windows named-pipe access uses an explicit DACL granting access to the
current process user and LocalSystem, while omitting Everyone and anonymous
users. Remote clients are rejected and client opens use identification-level
QoS. The Windows CI suite checks the named-pipe peer SID/PID and runs an
agent/CLI IPC smoke test. Unix sockets use mode `0600` and are cleaned up
using socket inode checks.

These controls restrict the IPC endpoint to the current user context; they do
not protect against malicious code already running as that user or a
compromised account. Review the [IPC security notes](IPC.md) before publishing.
