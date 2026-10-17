# Security posture

## IPC socket caller-identity authentication

The cua-driver daemon listens on a unix-domain socket (default per-user path, or the
`--socket` path). Callers reach it either directly or through the `icu` layer, which spawns
one `cua-driver mcp` proxy child.

### What is enforced today

The daemon authenticates the connecting process at the socket layer, not only at the tool
layer:

- The socket file is forced to mode `0o600` (owner read/write only) at bind time
  (`secure_local_socket`, guarded by `every_local_service_socket_is_forced_private`).
- On every `accept`, the daemon reads the peer's credentials over `SO_PEERCRED`
  (tokio `UnixStream::peer_cred`) and compares the peer uid to the runtime owner's
  effective uid (`authenticate_unix_peer` / `authenticate_unix_uid` in `serve.rs`). A
  connection from a process owned by a different uid is refused before any request is
  parsed. This is the connection-layer equivalent of Codex's
  `senderProcessNotAuthenticated` (-10000).
- An embedded or trusted-host session additionally requires the peer PID to equal the
  launcher's PID (`authenticate_embedded_host_connection`).
- The sensitive `history_control` / `history_relaunch_state` methods additionally resolve
  and check the peer process's executable path per request
  (`history_cli_executable_path`), so those specific methods are vetted per request, not
  just per connection.

Guards: `unix_peer_auth_admits_the_owner_and_rejects_other_uids` and
`every_local_service_socket_is_forced_private` (both in `serve.rs`).

### Threat model and what is deliberately not done

The socket is a single-tenant, per-user private endpoint. The uid check defends against a
different local user connecting to your daemon on a shared machine. It deliberately does
not authenticate each same-uid process beyond the embedded-host PID check and the
history-control executable check: on a single-tenant machine any process running as you is
already inside the trust boundary (it can read your files, attach a debugger to your
processes, and reach the socket by owner permission anyway), so per-process auth for
same-uid callers would add cost without changing what an attacker at that privilege can do.

### When to build more (the trigger)

Add per-request caller authentication for every method, and drop the "any same-uid process
is trusted" assumption, if any of these becomes true:

- the driver is run as a multi-client service with several distinct, mutually distrusting
  callers;
- it runs on a shared or multi-user host where more than the owner must be served; or
- the socket is ever exposed over a network transport rather than a local unix socket.

In that case, gate each request on an allowlist of caller executable paths or a signed
per-request token, matching Codex's per-request process vetting for all methods, not only
`history_control`.
