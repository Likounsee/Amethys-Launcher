# IPC transport

Amethys Core uses a local-only, versioned IPC stream between the agent and its
workers. The transport is deliberately independent of the GUI.

## Wire format

Each message is a JSON-serialized `IpcEnvelope<T>` preceded by a 32-bit
big-endian unsigned byte length. The current protocol version is `1` and the
maximum serialized payload is 8 MiB. Receivers should use
`read_envelope()`, not `read_message()`, for protocol envelopes so the
version is checked before dispatch.

Every envelope carries a UUID request ID. Handlers should preserve that ID in
their response so callers can correlate concurrent requests.

## Request and response messages

The shared protocol types currently define two requests:

- `ping`: checks whether the agent can answer.
- `get_status`: asks for the core's current state.

A response contains the original request ID and either a result (a pong or the
core state) or a structured error code and message. These are message formats
only: the agent does not yet execute these commands. A dispatcher and the
connection between the agent and this protocol are still to be implemented.

## Platform endpoints

- Windows: named pipes, for example `\\\\.\\pipe\\amethys-core`.
- Linux and macOS: Unix-domain sockets at an application-owned filesystem path.

Use `transport::IpcListener::bind()` to create a server endpoint and
`accept()` to accept one client. Use `transport::connect()` for a client.
The current listener abstraction accepts one connection per instance; create
another listener instance when the server is ready to accept another client.

On Unix, the socket path is restricted to mode `0600` and is unlinked when the
listener is dropped. On Windows, the named pipe uses the default security
descriptor associated with the creating process; production deployment must
review and, where necessary, explicitly configure its ACL.

## Security requirements

A local endpoint is not proof that a peer is trusted. Before executing a
command, the server must validate the envelope version, validate the payload,
authorize the requested operation, and apply resource limits. Do not expose
these endpoints over TCP or forward them outside the local machine.

The transport currently provides connection and framing primitives only. It
does not yet implement peer authentication, command authorization, a request
dispatcher, reconnection, or a persistent multi-client server loop.
