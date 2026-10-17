# Installed Codex computer-use runtime: trace, drift, and fixes

Inspected 2026-10-06. The local app contains readable JavaScript and the native
macOS service, so most of the client path is directly inspectable. A binary does
not contain the remote model, its training, or the live values of server-controlled
experiments. This is a reconstruction of the local execution path, with a targeted
native disassembly, not a claim to have recovered the entire product's source.

## Evidence and versions

| Component | Archived reference | Installed app |
|---|---|---|
| ChatGPT app | 26.901.51231 | 26.917.51856, build 10492 |
| `@oai/sky` | 0.6.26 | 0.7.1 |
| `@oai/cua` | 0.2.4 | 0.2.5 |
| `@oai/browser-desktop` | 0.1.1 | 0.1.1 |
| macOS client protocol | `CodexComputerUseIPC-5` | `CodexComputerUseIPC-5` |
| Native service SHA256 | `25e9141499b94c396f39afbdb7b19ed8f49e45dc8c61be61028ceab8f3807ce6` | `54f208b77b523f645e258ef2ef765b88460d74c87aed583ee6ab3d6ca620da7a` |

[Machine-readable inventory](../study/audit/codex-runtime-2026-10-06/runtime-inventory.json)
records file hashes, imports, export counts, and byte changes. Regenerate it with:

```sh
python3 re/inspect_codex_runtime.py \
  --output study/audit/codex-runtime-2026-10-06/runtime-inventory.json
```

The script reads package files and Mach-O metadata. It does not launch the service,
connect to its socket, read account state, or perform UI actions. The old archive
remains intact. Of the standalone Sky source files, 110 are byte-identical, 49
changed and 31 are new. Minifier names and import paths account for some byte
changes; those counts are not counts of behavior changes.

There are two bundled copies of the Sky client: one under `@oai/sky`, and one inside
`@oai/cua`. The latter is what the unified facade imports. Their current macOS
`client.js`, `native-pipe.js`, `errors.js`, and `get_app_state.js` are byte-identical.

## End-to-end local path

```text
Model tool call
  -> persistent JavaScript runtime with host-provided nodeRepl services
  -> @oai/cua tinysky_alt globals and unified facade
       -> native app branch: bundled Sky platform client
            -> app policy and host approval
            -> MacComputerUseClient request/action serialization
            -> native-pipe JSON-RPC over a local socket
            -> SkyComputerUseService
                 -> accessibility trees, stable IDs, diffs, refetch
                 -> capture, input, focus handling, settling
            <- app identity + AX text + screenshot URL + optional app instructions
       -> browser branch: browser runtime, tabs, and browser-specific APIs
  <- emitted text/images and stateful app/tab handles
Model observes the result and decides the next action
```

The model-facing tool description establishes the outer persistent REPL interface.
The exact host implementation of that tool was not decompiled in this pass.
The entries below are verified from current bundled code. Paths are relative to
`ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/cua/dist/`.

| Stage | Source | Observed behavior |
|---|---|---|
| Runtime initialization | `lib/js/oai_js_cua/src/tinysky_alt/globals.js` | Reads enabled surfaces from host configuration and installs `globalThis.cua`. |
| Unified handles | `lib/js/oai_js_cua/src/tinysky_alt/create_tinysky_alt.js` | Implements `getState`, `getApp`, browser and tab selection, and documentation emission. On macOS, `getApp` requests full initial state and returns an app-bound handle. |
| Inventory | `lib/js/oai_js_cua/src/get_state.js` | Collects native apps and browsers independently with `Promise.allSettled`; failed surfaces are reported in `errors`. |
| Native entry | `project/cua/sky_js/src/targets/mac/create_client.js` | Exposes app observation and input; audio is enabled conditionally. |
| App policy | `project/cua/sky_js/src/targets/mac/computer-use-policy.js` | Resolves policy, asks the host's elicitation service for approval, binds the approved app, and records completion/failure telemetry. Host code supplies persisted approval handling. |
| Observation | `project/cua/sky_js/src/targets/mac/get_app_state.js` | Runs the policy wrapper, requests native state, then normalizes the result. |
| Action encoding | `project/cua/sky_js/src/targets/mac/client.js` | Encodes the app, element ID or coordinates, action, options, metadata, and request type. |
| Transport | `project/cua/sky_js/src/targets/mac/native-pipe.js` | Uses 4-byte little-endian framing, an 8 MiB ceiling, a version handshake, serialized requests, absolute deadlines, and host service startup when needed. |
| Result formatting | `project/cua/sky_js/src/targets/mac/window_result.js` | Validates AX text and screenshot URLs; injects app instructions once per app, with a Numbers-specific exclusion. |

For example, `app.selectText(..., {selectionType: "cursor_after"})` becomes
`selection_type` in the Sky interface, then `selection` inside the native wire
action. Those are semantically significant options, not disposable metadata.

The [official computer-use guide](https://developers.openai.com/api/docs/guides/tools-computer-use)
describes the broader observe/act/observe loop and persistent execution state.
The [desktop Computer Use documentation](https://learn.chatgpt.com/docs/computer-use)
describes app approvals and platform behavior. Neither establishes the private IPC
implementation; the local files provide that evidence.

## What the current native binary establishes

The current service has 1,550 `LC_SYMTAB` entries and 62,041 exported symbols.
Most useful Swift names live in `LC_DYLD_EXPORTS_TRIE`. Reading only `LC_SYMTAB`
would incorrectly suggest that the accessibility machinery disappeared.
The inventory parses both structures and records the address of the ID allocator.

Exported names include 96 matches for `UIElementRenderTree`, 42 for
`UIElementRenderDifference`, 36 for `RefetchableSkyshotAXTree`, four for
`waitForUIToSettle`, and 25 for `SystemFrontmostApplicationTracker`. These include
metadata and accessors, so counts are not counts of independent features.
Imports include ApplicationServices, CoreGraphics, AppKit, and ScreenCaptureKit.
Names and linked frameworks establish compiled structure, not which route a
particular live action takes.

The [targeted disassembly](../study/audit/codex-runtime-2026-10-06/element-id-disassembly.txt)
locates `UIElementRenderTree.NextAvailableElementIDIterator.next()` at
`0x10062e030` in this exact binary. It reads state, conditionally calls an
initialization helper, adds one with an overflow check, stores the counter, clears
the initialization flag, and returns. The increment/store behavior is visible in
the current machine code. The initialization helper was not decompiled, so this
does not establish every detail of ID allocation or reuse.

Reproduce the saved instruction range only after checking the binary hash:

```sh
xcrun llvm-objdump -d \
  --start-address=0x10062e030 --stop-address=0x10062e084 \
  '/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/sky/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService'
```

`llvm-objdump` labels the range using the nearest ordinary symbol-table entry,
which is misleading here. The function identity and address come from the export
trie, as recorded in the inventory. The earlier [Ghidra findings](20-native-decompile-findings.md)
remain evidence for the archived September binary, not automatic proof of every
current implementation detail.

Follow-up: [twelve current native functions decompiled](../study/audit/native-refetch-2026-10-06/README.md)
now establish the invalidation, uniqueness and post-edit recovery paths directly.
That pass includes raw C pseudocode, assembly, binary hashes and a reusable
headless decompilation workflow.

The follow-up also resolved all eleven comparison fields and implemented
[native semantic recovery](../impl/native/RECOVERY.md) in the tracked patch. Eight
owned-app cases passed on an isolated Mac, including refusal of ambiguous and
incomplete searches. The evidence is scoped to recovery; the broader gaps below
remain separate work.

## Amendments made here

All four defects were reproduced against our fake driver before changing the
adapter. Tests exercise real serialized requests and verify that refused actions
do not reach the driver's input tools.

| Defect | Previous behavior | Change |
|---|---|---|
| Expired wire deadline | Saturated to a new 1 ms budget and still dispatched. | Reject malformed deadlines and expired requests after acquiring the dispatch lock. Recheck immediately before input, after preparation and the settle gap. |
| Lost observed window | Fell back to another window and could send a keystroke there. | Bind the observation to window ID and process ID. A missing window or changed PID requires a new observation. Observation can recover and show the replacement window. |
| Rich paste request | Dropped `format` and pasted HTML/Markdown literally. | Explicitly refuse unsupported formats before clipboard writes. Plain-text paste still works. |
| Cursor placement request | Dropped `selection` and selected text, risking replacement on the next edit. | Explicitly refuse `cursor_before` and `cursor_after`; ordinary selection preserves text, prefix, and suffix. |

Deadline admission and stale-window errors are our adapter guarantees, not newly
observed error behavior of the native service. `deadlineExceeded` is an ICU code;
the compatible wire response uses the existing generic `-10005` number. Running
synchronous input still cannot be cancelled after dispatch. PID/window binding
does not independently establish process birth identity or protect against every
possible ID-reuse race; driver-side target validation remains necessary.

The conformance client can now be selected with `CONFORMANCE_SKY_MAC_SOURCE`.
Its default stays the archived client. The installed client test uses the real
`client.js`, `native-pipe.js`, and `errors.js`, changing only the tslib import path
for the fixture harness. It connects to our service over a real local socket.
This tests the wire boundary, not the policy wrapper, host approval UI, remote
model, or real native service.

The README also incorrectly described background delivery as the default.
`Config::from_env` defaults to `attended`, and `exec::Mode::delivery` maps that to
foreground unless overridden. The README now documents those existing presets
and the `ICU_MODE=headless` setting that refuses foreground delivery. Mode behavior
was not changed by this patch.

## Remaining gaps, in useful implementation order

1. **Full app/browser facade.** ICU's `get_state` explicitly lacks browser/tab
   inventory. The current unified facade merges both and returns persistent
   app/tab handles with staged documentation. This is a larger integration feature
   than the pipe adapter. Start with a single client wrapper over existing native
   and browser APIs, plus partial-failure tests.
2. **Cursor placement and formatted clipboard support.** The adapter now refuses
   unsupported requests honestly. Implement native selection ranges and clipboard
   type preservation before advertising these options. Existing clipboard restore
   preserves plain text only, not every clipboard representation.
3. **Outcome verification on isolated desktops.** The fake driver proves routing
   and refusal, not actual app edits. Re-run the same observation/action sequences
   against owned fixtures on disposable macOS, Windows, and Linux hosts. A logged-in
   native-service trace remains the missing oracle for complete behavioral parity.
4. **Current Linux and Windows contracts.** The newer packages add Linux window,
   accessibility, app-discovery, and transport files. No Linux executable is present
   in this macOS app, but that does not establish what another platform's current
   distribution contains. September claims about global availability need a fresh
   platform-specific check. Windows native behavior was not revalidated here.
5. **Dynamic native behavior.** Settling thresholds, screenshot heuristics, cursor
   behavior, and experiment settings still need scoped measurements. The presence
   of a symbol or an old decompile cannot determine active runtime settings.

The separate [upstream cua-driver rebase analysis](../study/audit/upstream-rebase-2026-10-06/PLAN.md)
identifies token and observation-schema changes. This pass does not rebase the
driver or transfer old native acceptance claims to a new upstream revision.

## Validation

- Original implementation: all four new defect tests failed with success replies
  or misdirected input where the test required refusal.
- Updated ICU: 206 unit tests and four integration tests passed. Coverage includes
  closed-window recovery, a changed process with reused window ID, malformed and
  expired deadlines, expiry during settling, and the supported/unsupported options.
- Archived Sky client: three pipe conformance tests passed.
- Installed unified-client Sky copy: the same three tests passed. The corresponding
  standalone Sky macOS client files have identical hashes.
- Full offline conformance: 196 passed, seven skipped, three expected failures,
  in 728.08 seconds. The skipped live lanes and existing expected gaps remain
  excluded from the passing count.

Build and run these checks with the commands in [conformance/README.md](../conformance/README.md).
All input/action checks in this pass use fixtures. No app was activated and no input
was sent to the user's desktop.
