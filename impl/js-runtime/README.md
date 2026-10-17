# Persistent JavaScript computer-use runtime

Run `npm ci --ignore-scripts` in this directory, then configure a stdio MCP server:

```json
{"command":"node","args":["/path/to/invisible-computer-use/impl/js-runtime/server.mjs","--icu","/absolute/path/to/icu"]}
```

The runtime starts one `icu mcp` session. Native tool names and policy remain in
that server. Human elicitation replies pass from the MCP client to the native
server; evaluated JavaScript cannot approve its own requests.

```js
state.windows = (await icu.call('list_windows', {})).result;
emit(state.windows);
```

Later cells can reuse `state`. Each cell supports `await`; its local declarations
are scoped to that cell. This is an explicit persisted-state API, not byte-for-byte
compatibility with another product's JavaScript lexical environment.

QuickJS runs inside WebAssembly with a 32 MiB guest-memory limit, a bounded stack,
a deadline interrupt and no installed module loader or Node globals. Only the
JSON bridge, `emit` and `emitImage` enter the guest. Native calls remain serial.
A cell may perform at most 32 calls and produce at most 4 MiB of output. Code is
limited to 64 KiB. A timeout, cancellation or error discards guest state, and the
next cell starts fresh. An admitted native action may already have taken effect;
that action is never automatically replayed.

Cancellation cancels any waiting native approval, and late approval replies
cannot admit the stopped action. The first textual observation of a window after
a reset requests a complete tree so the new cell can recover its element IDs.

The runtime does not give scripts direct filesystem, process or network APIs.
Nine interpreter/protocol checks and actual Windows observation, decline,
approved editing, persistence and cleanup passed in the
[round-four audit](../../study/audit/parity-round4-2026-10-09/README.md).
Mac and Linux native acceptance remain separate work.
