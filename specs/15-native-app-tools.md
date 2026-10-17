## Native app tools: Messages/iMessage two-phase send

Beyond generic screen/keyboard/mouse control, Codex ships app-specific "native" tools that bypass the UI entirely and talk to an app's data store directly. The flagship is a Messages/iMessage tool suite: read tools query the local `~/Library/Messages/chat.db` SQLite database, and sending goes through a two-phase PrepareSend/CommitSend flow gated by a user-editable approval. This is exposed to the model as an MCP server ("Runs the Messages client as an MCP server") rather than as computer-use actions. None of this exists in cua-driver, and most of it is out of scope for a generic driver; the send-approval pattern is the one reusable idea.

### Codex/Sky

All symbols/strings below are CONFIRMED from `strings` and Swift demangling on the native service binary (captured at `.../@oai/sky/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService`; the durable copy is `reference/openai-sky/bin/SkyComputerUseService`). The umbrella grep (`Messages|PrepareSend|CommitSend|MessagesSendApproval|FindChats|ReadMessages|SearchMessages|CountActivity|chat.db|ComputerUseIPCMessages`) returns on the order of 200+ hits against the archived binary (`strings -a … | grep -Eo … | wc -l` = 222), confirming a large native Messages surface. (An earlier "~300" figure was against the shipped app-bundle build, which is not archived; do not treat the exact number as stable across builds.)

- Six MCP tool ids, each with a long model-facing description string in the binary: `find_chats`, `read_messages`, `search_messages`, `count_message_activity`, `read_image`, `send_message`. CONFIRMED (each appears as an exact lowercase tool id plus a paragraph-length description).
  - `find_chats`: "Find recent Messages chats by participant, chat name, date range, or unread status ... Returns each chat's unread_count and stable chat_guid for reuse with read_messages, search_messages, send_message, and count_message_activity."
  - `read_messages`: "Read messages from one exact chat, newest first" with cursor pagination (`next_cursor`), `unread_only`, and per-page `local_ref` sender/chat mappings.
  - `search_messages`: "Search message-body text, newest first" narrowed by `chat_guids`, `participants`, ISO-8601 `from`/`to`; paginated.
  - `count_message_activity`: "Count Messages activity over time, either overall or per chat ... total message counts split into sent and received," `breakdown` overall|chat, `counts_by_bucket` aligned with `buckets`.
  - `read_image`: "Read an image attachment returned by read_messages or search_messages. Returns the image, which may be resized or compressed to limit transfer size." (Read tools return attachment metadata but not contents; `read_image` is the explicit fetch.)
- Data source is chat.db SQLite, CONFIRMED: the literal `Library/Messages/chat.db`; SQLite wrapper classes `_TtC6SQLite10Connection`, `SQLiteSchema`, `PreparedStatement`; and raw SQL fragments including `), selected_messages AS (`, `FROM selected_messages`, `.message message ON message.ROWID = selected_messages.row_id`, and `ORDER BY selected_messages.sort_date DESC, selected_messages.row_id DESC, selected_messages.chat_row_id DESC`. Supporting types: `MessagesDatabaseAdapter`, `MessagesDatabaseSession(Factory)`, `MessagesContactDatabase`, `MessagesHandleFrecency`, `MessagesPaginationCursorWithState`, `messagesQueryTimedOut` / "The Messages database query exceeded its time limit."
- Two-phase send, CONFIRMED via a dedicated IPC message pair: `ComputerUseIPCMessagesPrepareSendRequest` -> `ComputerUseIPCMessagesPreparedSend`, then `ComputerUseIPCMessagesCommitSendRequest` -> `ComputerUseIPCMessagesSendResult`. Phase 1 resolves and plans the send (destination resolution: `MessagesSendDestination`, `messagesAmbiguousDestination`, `MessagesSendDestinationKind`, `SendPlanUnavailableReason`); phase 2 actually delivers. Outcome states include `messageSent`, `messagesSendOutcomeUncertain`, `messagesSendRateLimited`, `messagesSendPlanUnavailable`.
- User-editable approval store, CONFIRMED: class `MessagesSendApprovalStore` (demangled `_TtC17ComputerUseClient25MessagesSendApprovalStore`), alongside `MessagesReadDecisionStore`. Approval taxonomy from the analytics enums: operations `CODEX_MESSAGES_APPROVAL_OPERATION_{SEND_MESSAGE,READ_MESSAGES,SEARCH_MESSAGES,READ_IMAGE,UNSPECIFIED}`; persistence scopes `CODEX_MESSAGES_APPROVAL_PERSISTENCE_{ONCE,TURN,SESSION,ALWAYS,UNSPECIFIED}`; results `CODEX_MESSAGES_APPROVAL_RESULT_{ACCEPTED,DECLINED,CANCELED,ERROR,UNSPECIFIED}`. Cancellation is explicit and non-retryable: "Message was not sent because the send approval was canceled." and, for reads, "Content was not returned because the read approval was canceled. Do not ask the user to approve access or retry." The "user-editable" part is the approval UI showing the resolved message and recipients before commit, per the entitlement copy below.
- OS permission tie-in, CONFIRMED and now archived (`reference/openai-sky/plists/`): the Sky app's own plist (`.../@oai/sky/Codex Computer Use.app/Contents/Info.plist`, archived as `sky-Codex-Computer-Use-Info.plist`) carries `NSContactsUsageDescription` = "ChatGPT uses Contacts to show names and find phone numbers or email addresses for people you ask to message." and `NSAppleEventsUsageDescription` = "By default, ChatGPT sends messages only after you approve the message and its recipients." (both verified verbatim via `plutil -extract`). The top-level `ChatGPT.app/Contents/Info.plist` (archived as `ChatGPT-Info.plist`) `NSAppleEventsUsageDescription` = "ChatGPT uses Apple Events to control Mac apps on your behalf." The corresponding gates in the binary ARE archived and CONFIRMED: `MessagesAppleEventsPermission`, `MessagesSystemPermissionGate`, `messagesPermissionsPending` / `messagesPermissionsDenied`, "Required Messages permissions were not granted." (Reads hit chat.db + Contacts; sends use AppleEvents/scripting, cf. `MessagesScriptingType`, `MessagesDraft`, `MessagesSaveOptions`.)

### cua-driver

- Absent. `grep -ril "chat.db\|imessage\|Messages" .../cua-driver/rust` returns only false positives: CHANGELOG.md, test files, `updater.rs`, overlay/input source, `server.rs`/`tool.rs`, `platform-macos/src/permissions/gate.rs` (line 372, the word "messages" in a code comment about error messages), and SKILL.md. None reference iMessage, chat.db, or a Messages tool.
- The broader grep for `send_message|find_chats|read_messages|PrepareSend` finds only generic transport/RPC uses of `send_message` (contract manifest, `_native.py`, `cua_driver_sdk.ts`, testkit, `desktop.rs`) and the prose "send messages" in SKILL.md line 1104. No iMessage semantics, no two-phase send, no approval store.
- There is no native-app-tool layer at all in cua-driver: the contract (`rust/crates/cua-driver-contract/src/desktop.rs`) is screen/input/window primitives. No Contacts or AppleEvents integration; macOS permissions handling in `platform-macos/src/permissions/gate.rs` covers input/accessibility/screen-recording gates, not Contacts/Automation-per-app.

### Delta

- The standalone [Messages component](../impl/messages/README.md) now runs five
  read/image tools through MCP, with bounded workers, pagination, activity counts,
  permission-filtered pages, host forms, revocation and cancellation. The
  [round-six audit](../study/audit/parity-round6-2026-10-09/README.md) checks the
  packaged executable and an offline source rebuild against synthetic fixtures.
  All six interface schemas match the captured reference. This is partial
  implementation, not full Messages parity.
- Round seven adds native Contacts resolution with a signed helper and prepares
  reviewed sends with immutable file copies, editable bodies, revalidation and
  uncertain-outcome handling. Its send tests use capture-only delivery.
- Round eight connects an AppleEvents transport and persistent uncertainty
  journal. A self-addressed scripting fixture verifies the complete MCP path,
  group creation and restart refusal; it does not test real-app Automation consent
  or signed-in delivery. Those remain open with native host UI and
  conversation-aware lifecycle integration.
  Reference native queries timed out during bootstrap on the test Mac, leaving
  data-selection and approval equivalence unverified.
- cua-ahead: none (cua does not attempt this surface).

### Recreate/port plan and whether to build it

The user's expanded scope includes the complete Messages feature set. It belongs
in a separate app-tools component above the generic driver. That component owns
the private database schema, Contacts and AppleEvents maintenance; the driver
continues to provide general screen and input operations. Earlier statements
calling Messages a project non-goal are superseded.

What is worth stealing is the two-phase-send approval pattern, which is reusable for any irreversible action a generic driver might expose (a paste-and-submit, a form submit, a destructive click):

- Phase 1 "prepare": the tool resolves and returns a fully-materialized plan (resolved recipients, final message body, destination kind) without side effects. Hook point in cua would be a new contract action shape returning a `PreparedAction` handle rather than executing, mirroring `PrepareSendRequest -> PreparedSend`.
- Approval gate: surface that plan to the user with an editable payload and a persistence scope (once/turn/session/always), storing the decision keyed by operation+destination. Mirror `MessagesSendApprovalStore` + the `PERSISTENCE_*` and `RESULT_*` enums. Canceled means abort-and-do-not-retry, communicated back to the model in-band (Codex's exact wording: "Do not ask the user to approve access or retry"). This is the genuinely portable, model-safety-relevant piece.
- Phase 2 "commit": takes the prepared handle and executes, returning a rich outcome enum (sent / uncertain / rate-limited / plan-unavailable) rather than a bare boolean, so the model can reason about partial failure. Mirror `CommitSendRequest -> SendResult`.

The implementation boundary remains a separate MCP server alongside the driver,
with the desktop host providing consent and lifecycle management. The driver is
available for any explicitly authorized UI fallback.
