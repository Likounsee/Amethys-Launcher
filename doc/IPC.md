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
- `games add <name> <platform> <install-path> [executable]`: manually adds
  a game without a source-platform identifier.
- `games remove <id>`: removes a game from the Amethys library only.
- `games import <platform> <platform-id>`: imports a detected installation
  and stores its original platform-specific identifier.
- `games scan`: scans supported launcher metadata and prints detections
  without importing them automatically.

The storage-aware agent handles game creation, listing and removal. A detected
game's platform ID is scoped to its source platform; identical IDs from two
different platforms are not treated as the same game. The database enforces
uniqueness for a platform/ID pair. Older saved games without a platform ID
remain valid after the schema migration.

## Platform endpoints

- Windows: named pipes, for example `\\\\.\\pipe\\amethys-core`.
- Linux and macOS: Unix-domain sockets in the application data directory.

Use `transport::IpcListener::bind()` to create a server endpoint and
`accept()` to accept clients. Use `transport::connect()` for a client.
The agent runs a persistent accept loop and handles each local request.

On Unix, the socket path is restricted to mode `0600` and is unlinked when the
listener is dropped. On Windows, the named pipe uses the default security
descriptor associated with the creating process; production deployment must
review and, where necessary, explicitly configure its ACL.

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
