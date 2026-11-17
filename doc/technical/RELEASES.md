# Builds and releases

## Supported 0.1 release targets

The release workflow builds the CLI and core agent as native Rust executables:

| Operating system | Target | Archive |
| --- | --- | --- |
| Windows 64-bit | `x86_64-pc-windows-msvc` | ZIP |
| Linux x86_64 (GNU) | `x86_64-unknown-linux-gnu` | TAR.GZ |
| macOS Apple Silicon | `aarch64-apple-darwin` | TAR.GZ |

Each archive contains `amethys-agent` (or `.exe`) and `amethysctl` (or `.exe`). These are portable binaries, not an installer or operating-system service package. The agent must be running before the CLI can communicate with it. Automatic startup, service registration, desktop shortcuts, update delivery and uninstallers are not part of this initial release package.

## Creating a draft release

Push a version tag such as `v0.1.0`:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds all three targets, archives the two executables for each target, calculates SHA-256 checksums, and creates a **draft** GitHub release. The maintainer must inspect and manually publish the draft; a successful build does not automatically make a release public.

The workflow can also be started manually for an existing tag through the GitHub Actions interface.

## First-run notes

1. Extract the archive to a directory owned by the current user.
2. Start `amethys-agent` (Windows: `amethys-agent.exe`) in a terminal.
3. In another terminal, run `amethysctl ping` and `amethysctl status`.
4. Run `amethysctl games scan` to inspect supported local launcher metadata.
5. Use `amethysctl games import-all` only when you want to add all detected installations to the Amethys database.

The agent currently runs in the foreground. Closing its terminal stops it. Do not expose the IPC endpoint over a network.

## Validation before publication

A release candidate is not ready merely because compilation succeeded. Before publishing a draft, check:

- the CI workflow is green on Windows, Linux and macOS;
- each archive contains both expected executables;
- `amethysctl --version` and `amethysctl --help` work;
- the agent starts and responds to `ping` and `status`;
- scan, import, list, search and read-only verify behave as documented;
- the archive and checksum files can be downloaded and verified on each target OS.

Windows named-pipe access currently uses the creating process' default security descriptor. Do not describe the 0.1 IPC endpoint as isolated from other local Windows users until an explicit restrictive DACL and the corresponding security tests are implemented.
