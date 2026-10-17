# Messages MCP service

Standalone GPL-3.0-or-later component. The core driver and desktop app do not link
it. See [NOTICE.md](NOTICE.md) for the typedstream decoder dependency.

The stdio service implements `find_chats`, `read_messages`, `search_messages`,
`count_message_activity`, `read_image` and reviewed `send_message`. Native message
delivery uses a separate macOS AppleEvents helper. The desktop app's own Messages
UI remains separate work. The native event path has passed synthetic scripting
tests; sending through a signed-in Messages account is still unverified. The
reference's six tool schemas are retained for compatibility.

## Run

```sh
cargo build --manifest-path impl/messages/Cargo.toml --release --locked
```

Configure a form-capable MCP host with this server entry:

```json
{"command":"/absolute/path/to/icu-messages","args":["messages","mcp"]}
```

On macOS, the default database is `~/Library/Messages/chat.db`. Access remains
subject to macOS Full Disk Access. `--database`, `--state-dir` and `--time-zone`
override the database, private approval directory and IANA calendar zone. Outside
macOS, an explicit exported fixture database is required. No database is opened
by initialization or `tools/list`.

For the default Mac database, the native helper is loaded from the sibling
`Copilot Contacts.app` package. Run `icu-messages contacts status` to inspect its
permission and `icu-messages contacts authorize` to open the explicit macOS
permission request. Normal queries do not request OS permission or activate an
app. The helper is launched through LaunchServices so its own approved identity
applies when the parent is an SSH or background process. Calls use private files,
nonce-bound responses and a lifetime file that stops a canceled helper. The
helper has an eight-second query watchdog and a memory ceiling. Authorization
allows up to 130 seconds and can bring the permission prompt forward.

An exported `--database` keeps address-only resolution unless
`--contacts-helper /absolute/path/to/Copilot Contacts.app/Contents/MacOS/icu-messages-contacts`
is provided explicitly. This prevents fixture tests from accessing personal
Contacts accidentally. Read filters may combine matches for a partial name;
send planning refuses an ambiguous person. Contact names are refreshed for each
request. Limited Contacts permission only exposes the selected contacts.

For native sending, `Copilot Messages.app` sits beside the MCP executable. Run
`icu-messages delivery status` for preflight and `icu-messages delivery authorize`
to request Automation permission explicitly. Normal sends require that Messages
is already running with a connected account. They do not silently request a new
macOS grant. An exported database has native delivery disabled unless the host
explicitly supplies `--delivery-helper`; this keeps fixture reads from reaching
the real Messages app.

Text and paths are passed as AppleEvent data, never interpolated into executable
scripts. The helper rechecks the native chat's service, account and participants.
New conversations prefer iMessage; phone-only recipients may use a connected
RCS or SMS account. An accepted result means the Messages AppleEvent commands
were acknowledged, not that another device received the message.

Chat discovery and activity counts return metadata. Message bodies and image
contents are released only after the host approves the resolved chat, or a
matching existing grant authorizes it. Candidate bodies can be read privately
inside the worker before the chat approval is presented. A host without MCP
elicitation forms cannot grant new read permissions. The MCP client is the
approval authority; this protocol is not an OS security boundary against other
processes running as the same user.

Once grants last for one request. Session grants last for the MCP connection.
Always grants bind to the chat, service and normalized membership and survive
restarts. Cancellation is terminal for reads during the active turn. Turn scope
is offered only to hosts that declare `capabilities.experimental.icuTurnLifecycle`
and send `notifications/icu/turn_started` with a new `turnId`, followed by
`notifications/icu/turn_ended` when appropriate. These are host notifications,
not model tools. Conversation-aware host integration remains required where one
MCP connection is shared across conversations.

Review and revoke persistent permissions:

```sh
icu-messages permissions list
icu-messages permissions revoke --chat-guid CHAT_GUID --operation read
```

Both commands accept `--state-dir`. Revocation is observed by other live service
processes. Deleting and recreating the permission store invalidates old in-memory
grants and requires the old service instance to restart. Corrupt or non-private
storage does not grant access.

## Data and images

Queries use read-only SQLite snapshots, append watermarks, opaque query-bound
cursors and resource-limited workers. Search pages can omit denied chats and still
return `next_cursor`, including an empty page. Membership is rechecked after the
approval wait, before content is released. Late replies cannot authorize a
canceled request. The public search schema's nonempty text rule is preserved;
there is no separate 4096-byte search-text cap.

`read_image` accepts only an attachment id previously returned by an approved
message read in the current connection. It revalidates the original message/chat
association and confines file access to that database's `Attachments` directory.
Component-wise `openat` calls reject symlinks, traversal and non-regular files.
Old cached references may expire; every reference in a newly returned page stays
available. Reading the message again obtains a current reference.

macOS uses ImageIO for raster formats, including HEIC and EXIF orientation.
Linux fixture support uses the Rust image codecs. Results are PNG image blocks,
resized to at most 1536 pixels on either axis and capped at 2 MiB. Source files
are capped at 20 MiB. The query worker's eight-second default deadline, 512 MiB
RSS watchdog, CPU limit and bounded pipes contain malformed or oversized work.
The RSS watchdog is sampled and is not a hard OS memory sandbox.

## Validation and packaging

```sh
cargo test --manifest-path impl/messages/Cargo.toml --locked
cargo clippy --manifest-path impl/messages/Cargo.toml --all-targets --locked -- -D warnings
python3 impl/messages/scripts/package.py --output /absolute/new/package/path
```

The package script produces a developer executable, native Contacts and Messages helpers,
a server configuration and source with vendored dependencies. It does not install
the server into a host, notarize or publish it. The default helper is ad-hoc signed;
`--contacts-bundle` and `--delivery-bundle` can supply separately signed helpers. All tests use synthetic databases and images, with
MCP host decisions supplied by the test client. Native host-form UI acceptance
and the desktop application's integration remain separate work.

The [round-eight audit](../../study/audit/parity-round8-2026-10-09/README.md)
records the current service checks. The unmodified reference's native query
startup failed in round five, so schema and fixture checks do not prove full
behavioral equivalence. Signed-in native sending, exact Unicode search and unread/count
selection semantics still need implementation or native comparison.

The send planner resolves an exact existing conversation or a reviewed new
recipient list. It copies files into a private staging directory and hashes them,
then rechecks files, Contacts records, chat membership and the approval lease
before dispatch. Approval can edit text. Plans are consumed once and canceled
reviews cannot re-prompt in the same turn.

Before native dispatch, a private journal records the attempt. Active or uncertain
attempts block the same payload across service restarts. Renaming the chat,
reordering recipients or changing an attachment path does not bypass that check.
After checking the Messages conversation, the host can reconcile an outcome:

```sh
icu-messages sends pending
icu-messages sends reconcile --id ATTEMPT_ID --outcome sent
icu-messages sends reconcile --id ATTEMPT_ID --outcome not_sent
```

These administrative commands are not MCP tools. A still-running attempt cannot
be reconciled. Journals record destination metadata and content fingerprints,
not the message body. Accepted or uncertain file sends retain private attachment
copies because Messages may read them asynchronously. After confirming Messages
has finished with an accepted file, remove those copies explicitly:

```sh
icu-messages sends forget-attachments --id ATTEMPT_ID
```

Unknown outcomes must be reconciled before files can be removed. No automatic
expiry clears uncertain records. A native helper stops when its parent process
identity changes or its request lifetime ends. Successful, separately requested
messages remain distinct; this is not a claim of exactly-once delivery after a
client loses a success response.
