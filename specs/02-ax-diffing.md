## AX-tree diffing between turns

### 1. What it is

Sky's per-turn accessibility payload is not the whole tree every time. After the first snapshot for a chat, `get_app_state` returns a DIFF against the previous turn's tree: lines marked `~` changed, `+` added, `-` removed, or removed elements collapsed into element-ID ranges. This is the token-efficiency lever for long sessions, toggled off per-call by `disableDiff`. cua-driver has no equivalent: `get_window_state` walks the AX tree and emits the full Markdown plus a full structured `elements` array on every call, and each snapshot fully invalidates the previous index map.

### 2. Codex/Sky

The `disableDiff` flag is a thin passthrough in JS; the entire diff engine lives in the native `SkyComputerUseService` (Swift, `ComputerUseCore` module). CONFIRMED by demangled symbols and rendered help strings.

- `codex-corpus/mac/docs/sky-window-api.md` — `GetAppStateInput.disableDiff?: boolean`, documented as "Return a full accessibility tree instead of a diff from the previous tree." This is the only diff surface in the public API.
- `sky_js/.../targets/mac/get_app_state.js` — forwards verbatim: `e.getAppState({app: i.app, disableDiff: i.disableDiff})`. No diffing in JS.
- `sky_js/.../targets/mac/client.js` — marshals to the IPC request `ComputerUseIPCAppGetSkyshotRequest` carrying `{app, disableDiff}`. Request type string `"ComputerUseIPCAppGetSkyshotRequest"`.
- `sky_js/.../types/window/GetAppState.d.ts` — the typed `Input.disableDiff?: boolean`.
- Native binary `SkyComputerUseService` (strings + `nm | swift demangle`), key symbols:
  - `ComputerUseIPCAppGetSkyshotRequest.disableDiff.getter : Bool` — the flag lands here.
  - `ComputerUseAppController.updateSkyshot(treeCache:, disableAXDiffing:, skipScreenshot:)` and `updateSkyshotSettlingIfNeeded(disableAXDiffing:)` — `disableDiff` becomes `disableAXDiffing`.
  - `SystemSelectionExtractor.extract(from:, ..., continuingFrom: UIElementTreeRevision<TransformedUIElement>?, ..., enableAXDiffing: Bool)` — the diff is computed by carrying the PREVIOUS turn's `UIElementTreeRevision` in `continuingFrom`. Element identity is preserved across a `lineageID` / `lineageRootRevision` chain of revisions, backed by a persistent `TransformedUIElement.TreeCache`.
  - `UIElementTreeRevision.differenceLines(from: UIElementTreeRevision, options:, summarizesRemovedElementIDs: Bool, ignoreDifferenceLineBudget: Bool, onLineBudgetExceeded: (AccessibilityDifferenceLineBudgetExceeded) -> ()) -> [String]?` — THE diff emitter. Returns `nil` when the change set blows the line budget, which forces a full re-render. `summarizesRemovedElementIDs` is the removed-as-ID-range mode.
  - `UIElementRenderDifference.init(oldRender:, newRender:)`, `UIElementRenderDifferenceBuffer.changes : [Change]`, `Change.inheritElementID()`, `Change.path : IndexPath`, `Change.depthFirstOrder` — the structural diff pairs old vs new render arrays by tree path in depth-first order; unchanged nodes inherit their prior element ID, and new nodes draw the next ID from `UIElementRenderTree.NextAvailableElementIDIterator` (tracks `maxElementID`). Element indices therefore stay STABLE across turns.
  - Feature flags `feature/axTreeDiffing` -> `ComputerUseAppController.isAXTreeDiffingEnabled`, and `feature/axTreeDiffingRemovedElementIDRanges` -> the `removedElementSummary` / `summarizesRemovedElementIDs` path. Flag help: "Enables AX tree difference rendering." and "Summarizes removed AX tree elements as consolidated ID ranges in diff rendering."
  - Rendered header strings the model sees: "The following is a diff from the previous accessibility tree" + "with ~, +, and - representing changed, added, and removed elements, respectively."; the cumulative variant "The following is a cumulative diff from the initial accessibility tree" + "with ~ and + representing changed and added elements, respectively. Removed elements are summarized by ID range."; and the no-op short-circuit "There has been no change in the accessibility tree for ".

Mechanism summary (CONFIRMED for structure, INFERRED for exact budget values, which live in stripped Swift constants): a chat holds a revision lineage. Each turn extracts a new `UIElementTreeRevision` continuing the prior one, so element IDs are inherited by tree path. `differenceLines(from:)` renders only the changed/added/removed rows; if the diff exceeds a line budget it returns nil and Sky falls back to a full render; `disableDiff` bypasses the whole path and always renders full.

### 3. cua-driver

cua has NO cross-turn diff. It has per-snapshot positional indices plus an opaque snapshot token, and every snapshot supersedes the last. The nearest modules are the token registry and the element cache, neither of which compares trees.

- `rust/crates/platform-macos/src/ax/tree.rs` — `walk_element(...)` assigns a fresh 0-based `counter` (`element_index`, `index_counter = 0usize` at line 192, `*counter += 1` at line 487; the field doc at line 50 says "0-based index"), `parent_index`, and `depth` on every walk; `render_lines(&lines)` builds `tree_markdown` from scratch. `AXNode` carries no persistent identity across walks. `TreeWalkResult { tree_markdown, nodes, ... }` is rebuilt each call.
- `rust/crates/platform-macos/src/tools/get_window_state.rs` — the tool doc states the contract explicitly: "call get_window_state once per turn ... The index map is replaced by the next snapshot." Full tree + full `structuredContent.elements` returned every time; the only reduction knobs are `query`, `max_elements`, `max_depth`, `include_screenshot`, `include_accessibility_tree`. No `disableDiff`, no prior-tree input.
- `rust/crates/cua-driver-core/src/element_token.rs` — `TokenRegistry.register_snapshot*` mints a process-global `snapshot_id` per walk; `format_token` -> `s{snapshot_id:08x}:{index}`. The doc is emphatic that a new snapshot INVALIDATES the old (`STALE_TOKEN_ERROR`), so tokens deliberately do NOT survive across turns. Opposite of Sky's stable inherited IDs.
- `rust/crates/cua-driver-core/src/element_cache.rs` — `ElementCacheCore<K,S>`, a `Mutex<HashMap>` that stores one snapshot per `(pid, window)` and drops the prior on replace. It is a lookup cache for resolving an index to a native handle, not a diff baseline; it keeps no history and does no comparison.
- `rust/crates/cua-driver-core/src/element_query.rs` — `project_elements_for_query(elements, query, filtered_markdown)` filters the current snapshot to query matches plus ancestors. Row reduction, not turn-over-turn diffing.

So cua's token/index machinery is per-snapshot ephemeral by design; there is no lineage, no old-vs-new render comparison, and no persistent element identity to hang a diff on.

### 4. Delta

| Aspect | Sky | cua-driver |
|---|---|---|
| Cross-turn diff of AX tree | Yes: `differenceLines(from:)` with `~ + -` markers | None; full tree every turn |
| Element identity across turns | Stable via revision lineage + `inheritElementID()` | None; fresh positional index each walk |
| Removed-element compaction | ID-range summary mode | n/a |
| Oversize-diff safety | Line budget -> nil -> full re-render fallback | n/a (always full) |
| No-change short circuit | "There has been no change ..." | n/a |
| Per-call opt-out | `disableDiff` | n/a (already always full) |
| Token/index validity contract | IDs persist across the lineage | Explicit stale-on-next-snapshot token (`element_token.rs`) |
| Query/row projection | present | present (`element_query.rs`), and arguably cleaner as a structured field |

Where cua is ahead: an explicit, tested stale-token failure contract and a structured `elements` array with `role/label/value/frame/parent_index/depth`, which is a better substrate to diff over than Markdown lines. Where Codex is ahead: the entire diff subsystem, stable element identity, budget fallback, and removed-ID-range compaction.

### 5. Recreate/port plan

This is a new subsystem for cua, not a flag flip. The blocker is identity: a diff needs stable element IDs across snapshots, and cua deliberately has none. Steps, in dependency order:

1. Copy the API surface and header strings verbatim from `codex-corpus/mac/docs/sky-window-api.md` and the binary's rendered strings ("The following is a diff from the previous accessibility tree", the `~ + -` legend, the cumulative/ID-range legend, the no-change line). copy-verbatim.
2. Add cross-snapshot element identity in `rust/crates/platform-macos/src/ax/tree.rs`. Give each `AXNode` a stable key from `role + parent path + label` (fall back to AX path when label is empty), matching Sky's path-plus-inherit model. This is the hard part and is native-patch bordering large: the AX walk currently assigns identity positionally, so it must gain a matcher against the prior snapshot's keyed nodes. Consider promoting the key into `cua-driver-core` so Windows/Linux can share it. large.
3. Add a per-(pid, window) revision store that keeps the PREVIOUS snapshot's keyed render, analogous to Sky's `UIElementTreeRevision` lineage. Natural home is `rust/crates/cua-driver-core/src/element_cache.rs` (extend the stored `S` to hold the last render for diffing) or a sibling `element_revision.rs`. native-patch/large.
4. Write the diff emitter: compare prior keyed render to current, classify each node changed/added/removed, and render `~ + -` lines plus a removed-ID-range summary. Mirror `UIElementRenderDifference` / `differenceLines`. Include a line-budget guard that abandons the diff and returns the full tree when the change set is too large. New module in `cua-driver-core` (e.g. `element_diff.rs`), producing both diff Markdown and a diffed structured array. large.
5. Thread a `disableDiff` (or `disable_diff`) arg through `rust/crates/platform-macos/src/tools/get_window_state.rs`, defaulting to diff-on after the first snapshot per (pid, window), full when set. Wire the budget-exceeded and no-prior-snapshot cases to fall back to the existing full render. native-patch.
6. Reconcile with the token contract in `rust/crates/cua-driver-core/src/element_token.rs`: today a new snapshot invalidates old tokens. Diffing wants indices STABLE across turns, so either keep inherited element IDs valid across the lineage or emit a fresh full snapshot id each turn while presenting stable indices. Decide and document; this changes the stale-token semantics. native-patch.

Recommended first cut: implement the diff over the structured `elements` array only (skip Markdown-line diffing) and keep `disableDiff` returning today's full path unchanged. That isolates risk to steps 2 through 4 and leaves every existing full-tree consumer working.
