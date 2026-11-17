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
- `games set-executable <id> <path>`: configures the executable path for an imported game. The path is stored as metadata only; it is canonicalized and constrained to the registered installation directory when launch is requested.
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
- `games launch <id>`: starts the configured executable directly, without a shell. Launch is refused if the executable is missing, cannot be resolved, or resolves outside the registered installation directory.

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

On Unix, the socket path is restricted to mode `0600`. Stale-socket recovery
checks the socket type and inode before unlinking, and listener cleanup removes
only the socket inode it originally created.

On Windows, the named pipe uses an explicit DACL built for the current user.
Access is granted to that user and LocalSystem; access is not granted to
Everyone or anonymous users. Remote clients are rejected, the first pipe
instance is exclusive, and the listener creates subsequent instances with the
same DACL. Client opens use identification-level QoS. The Windows transport
test checks that the connected client and server report the expected SID and
process identity. The agent limits concurrent IPC connections to 32 and
applies a 15-second request timeout.

## Security requirements

A local endpoint is not proof that a peer is trusted. The operating-system
endpoint ACL is the first access-control boundary; the server also validates
the envelope version, bounds frames to 8 MiB, applies a request timeout and
limits concurrent connections. Every new command must validate its inputs,
authorize the requested operation and apply resource limits. Do not expose
these endpoints over TCP or forward them outside the local machine.

The current protocol is intended for local communication between the Amethys
CLI and agent. The agent rejects library mutations unless its lifecycle state
is `running`; storage validates metadata lengths, empty values and NUL bytes.
The Windows ACL and Unix socket permissions restrict access to the current
user context, but CI success is not a guarantee against compromise of that
user account or malicious code already running as that user.
