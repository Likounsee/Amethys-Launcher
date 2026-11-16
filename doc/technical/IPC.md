# IPC transport

Amethys Core uses a local-only, versioned IPC stream between the CLI and the
agent. The transport is independent of the graphical interface.

## Wire format

Each message is a JSON-serialized `IpcEnvelope<T>` preceded by a 32-bit
big-endian unsigned byte length. The current protocol version is `1` and the
maximum serialized payload is 8 MiB. Receivers use `read_envelope()` so the
protocol version is checked before dispatch.

Every envelope carries a UUID request ID. Responses preserve that ID so the
client can verify that a response belongs to its request.

## Supported commands

- `ping`: checks whether the agent responds.
- `get_status`: returns the core lifecycle state.
- `games`: lists games saved in the local library.
- `games list --platform <name>`: lists only saved games whose platform
  matches the supplied name, case-insensitively.
- `games search <term>`: searches saved game names, platforms, platform IDs,
  installation paths and executable paths, case-insensitively. When several
  words are supplied, every word must match somewhere in the combined metadata.
  Search is local to the Amethys library and does not query online services.
- `games add <name> <platform> <install-path> [executable]`: manually adds
  a game without a source-platform identifier.
- `games remove <id>`: removes a game from the Amethys library only.
- `games import <platform> <platform-id>`: imports one detected installation
  and stores its original platform-specific identifier.
- `games import-all`: explicitly imports every detected installation, skips
  games already in the library, and reports a summary. Each item is processed
  independently so one failed import does not stop the remaining items.
- `games scan`: scans supported launcher metadata and prints detections
  without importing them automatically.
- `games verify`: checks whether each saved installation path exists; it never
  modifies or repairs installed game files.

The storage-aware agent handles game creation, listing and removal. A detected
game's platform ID is scoped to its source platform; identical IDs from two
different platforms are not treated as the same game. The database enforces
uniqueness for a platform/ID pair. Older saved games without a platform ID
remain valid after the schema migration.

## Platform endpoints

- Windows: named pipes, for example `\\\\.\\pipe\\amethys-core`.
- Linux and macOS: Unix-domain sockets in the application data directory.

The agent and CLI resolve the same per-user data directory through
`amethys_config::data_directory()`. `AMETHYS_DATA_DIR` overrides the default.
Otherwise, Windows uses `%APPDATA%\\Amethys` (falling back to `%LOCALAPPDATA%`),
macOS uses `~/Library/Application Support/Amethys`, and Linux/other Unix systems
use `$XDG_DATA_HOME/amethys` or `~/.local/share/amethys`. The current working
directory is never used as an implicit data location.

Use `transport::IpcListener::bind()` to create a server endpoint and
`accept()` to accept clients. Use `transport::connect()` for a client.
The agent runs a persistent accept loop and handles each local request.

On Unix, the socket path is restricted to mode `0600` and is unlinked when the
listener is dropped. On Windows, remote named-pipe clients are explicitly
rejected. The pipe still uses the default security descriptor associated with
the creating process, so local-user access follows that inherited DACL; an
explicit restrictive DACL remains required before claiming isolation between
multiple local Windows users.

## Security requirements

A local endpoint is not proof that a peer is trusted. The server validates
the envelope version and applies a request timeout. Every new command must
validate its inputs, authorize the requested operation, and apply resource
limits. Do not expose these endpoints over TCP or forward them outside the
local machine.

The current protocol is intended for local communication between the Amethys
CLI and agent. Peer authentication and explicit per-command authorization
remain hardening work before treating the IPC boundary as a security boundary
against other local processes.
