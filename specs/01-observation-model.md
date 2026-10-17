## Observation model: indexed AX tree + screenshot

### 1. What it is

Each turn the agent SEES a window as a text rendering of its accessibility (AX) tree, where every actionable element carries an integer handle, plus a paired screenshot for grounding. The agent reads the tree, picks an element by its handle, and issues an action against that handle. The two systems agree on this shape but disagree sharply on element identity across turns (Sky keeps a monotonic `elementID` and diffs the tree; cua mints a fresh per-snapshot `element_token` and never diffs) and on payload shape (Sky is text-first, cua is structured-first).

### 2. Codex/Sky

Sky captures a "Skyshot": one operation that bundles the AX tree render and the screenshot. Element handles are allocated by a monotonic iterator with a `maxElementID` high-water mark, and the same element keeps its ID across captures so the tree can be sent as an incremental diff (added / removed / changed) rather than re-sent whole. Removed IDs are summarized as consolidated ranges, and rendering has a line budget. The public JS surface calls the handle `element_index` but the native IPC transmits it as a stringified `elementID`.

- `/Users/nathanzhao/dev/invisible-computer-use/codex-corpus/mac/docs/sky-window-api.md` — window1/mac surface. `get_app_state` returns `{ screenshot, text }` where `text` is the rendered AX tree; `GetAppStateInput.disableDiff` opts out of the diff to get a full tree. Actions (`click`, `scroll`, `set_value`, `select_text`, `perform_secondary_action`) address by `element_index`.
- `/Users/nathanzhao/dev/invisible-computer-use/codex-corpus/mac/skills/macos-SKILL.md` — the operative contract: "call `get_app_state(...)` before deciding what to do next ... forces you to re-derive fresh `element_index` values"; and "the accessibility tree will be returned as a diff from the most previous accessibility tree, listing only the elements that were removed, added, or changed. Prefer this default diff output; pass true for `disableDiff` only when you need a fresh full accessibility tree."
- `/Users/nathanzhao/dev/invisible-computer-use/codex-corpus/mac/docs/sky-window2-api.md` — window2 surface. `AccessibilityState` is richer than a bare tree string: `tree`, plus `focused_element`, `selected_elements`, `selected_text`, `document_text`. `Screenshot` is an array item with `id`, `zIndex`, `originX/originY`, `width/height`; `WindowState.screenshots` is a bounded array capturing transient UI (popovers/menus) with z-order, and `screenshotId` can be passed back to `click`/`scroll`/`drag`.
- Native binary `.../@oai/sky/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService` — symbols (via `nm | swift demangle` and `strings`):
  - `ComputerUseCore.UIElementRenderTree.NextAvailableElementIDIterator` with a `maxElementID` field and `.next() -> Swift.Int` — the monotonic elementID allocator. CONFIRMED.
  - `RefetchableSkyshotAXTree`, `RefetchableUIElement`, and error strings `elementNoLongerValidAfterRefetch`, `elementAmbiguousAfterRefetch`, `elementAmbiguousBeforeRefetch`, `invalidElementID` — elements are re-matched across refetches, so an ID has cross-turn identity. CONFIRMED.
  - Diffing: `ComputerUseAppController.isAXTreeDiffingEnabled`, `SkyshotOperation.init(... enableAXDiffing:)`, `updateSkyshot(treeCache: TransformedUIElement.TreeCache?, disableAXDiffing:, skipScreenshot:) -> SkyshotCapture`, `UIElementRenderDifference`, `SystemSelection.DifferenceBaseline`, string `AccessibilityDifferenceLineBudgetExceeded`, feature flag `feature/axTreeDiffingRemovedElementIDRanges`, strings "Removed element IDs: " and "Summarizes removed AX tree elements as consolidated ID ranges in diff rendering." CONFIRMED. `TreeCache` is the prior tree that serves as the diff baseline.
  - `eventStreamDescription(_:lineOptions: UIElementRenderLineOptions, ignoreDifferenceLineBudget:)` renders the tree lines. `EventStreamAXElement` fields: role, subrole, title, description, value, placeholder, identifier.
- `.../sky_js/src/targets/mac/client.js` — IPC mapping. `x(e)` requires an integer and returns `String(e)`, then addressing is `{ elementID: { _0: String(idx) } }` vs `{ coordinate: { _0: [x,y] } }`; `setValue`/`selectText`/`performSecondaryAction` send `elementID: String(...)`. `getAppState` passes `disableDiff` through. So the public `element_index` int is a display alias for the native string `elementID`.

### 3. cua-driver

cua walks the AX tree depth-first, assigns a 0-based `element_index` in DFS order, and returns BOTH a structured `elements[]` array and a `tree_markdown` string plus a paired screenshot, in a single `get_window_state` call. Element identity is deliberately per-snapshot: alongside each `element_index` it emits an opaque `element_token` of the form `s<8hex snapshot>:<index>`, valid only until the next snapshot for the same window supersedes it. There is no diffing and no cross-turn stable ID.

- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/platform-macos/src/ax/tree.rs` — `walk_tree_bounded` and `AXNode`. `index_counter = 0usize` then `let idx = *counter; *counter += 1` at line 486: 0-based DFS indices (the module doc comment in `element_token.rs` calling them "1-based" is inaccurate). `is_addressable` gates which nodes get an index (>=1 AX action name, or a writable value control, and not `AXEnabled==false`). `format_node_line` produces `- [N] AXRole "Title" = "value" (description) [id=... help="..." actions=[press,showmenu]]`; non-actionable rows render `- AXRole = "value"` with no index. Layout containers (`AXScrollArea`/`AXGroup`) collapse. Caps `DEFAULT_MAX_ELEMENTS = 2_000`, `DEFAULT_MAX_DEPTH = 25`. Unions `AXChildren + AXWindows` to see background windows.
- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/cua-driver-core/src/element_token.rs` — `TokenRegistry` keyed by `(runtime_scope, pid)`, `LRU_CAP_PER_PID = 8`. `register_snapshot` mints a process-global `AtomicU32` snapshot id; a new snapshot for the same `window_id` calls `lane.retain(|e| e.window_id != window_id)`, so the previous token goes stale immediately. `resolve` returns `(window_id, element_index)` or the sentinel `STALE_TOKEN_ERROR = "element_token is stale; call get_window_state again to refresh"`. `resolve_element_args` refuses a bare `element_index` with `snapshot_id_required` and refuses conflicting targets. Format is `format_token` -> `s{snapshot_id:08x}:{element_index}`. This is the direct counterpart to Sky's `elementID`, but per-snapshot instead of monotonic.
- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs` — the tool. Default `include_screenshot: true` and `include_accessibility_tree: true`, so tree + screenshot come paired (both-false is an error). Calls `register_snapshot` and emits `snapshot_id` plus a per-row `element_token` via `build_elements_array_with_token`. Structured fields per element: `element_index`, `element_token`, `role`, `label`, `value`, `value_description`, `min`/`max`, `enabled`, `selected`, `actions`, `in_web_content`, `frame:{x,y,w,h}`, `parent_index`, `depth`. No `focused_element`/`selected_elements`/`document_text` (grep found none). Emits `degraded_reason`/`escalation` hints and a `background_input` capability report.
- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/platform-macos/src/ax/window_scope.rs` — `decide_window_scope` and `WindowScope { Matched, NotFound, OwnerPidMismatch, AxUnresolved }`. Guarantees a snapshot never returns another surface's elements under the requested `window_id`; failures refuse with `window_id_not_found` / `window_owner_pid_mismatch` or degrade with `ax_window_unresolved`. Sky has no visible equivalent.
- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/cua-driver-core/src/element_query.rs` — `project_elements_for_query`, the ONLY payload-reduction mechanism cua has. It is a substring filter that keeps matching rows plus their ancestor chain with original indices intact. It is not a diff against a prior snapshot.
- `/Users/nathanzhao/dev/invisible-computer-use/vendor-cua/libs/cua-driver/rust/crates/cua-driver-core/src/window_inspection.rs` — `mark_browser_chrome_capture_coverage`, a coverage/recovery marker for browser chrome. Not a diff; noted only because it is the nearest "what changed / what to do next" signal cua carries.

### 4. Delta

| Aspect | Sky/Codex | cua-driver |
| --- | --- | --- |
| Element handle | monotonic `elementID` (Int), stable across turns | per-snapshot `element_token` `s<hex>:<idx>` + 0-based `element_index` |
| Cross-turn identity | yes, refetch re-matches; errors on ambiguous/invalid | none by design; old token goes stale on next snapshot |
| Tree delivery | diff by default (added/removed/changed, removed-ID ranges, line budget); `disableDiff` for full | always full `tree_markdown`; only a `query` substring projection |
| Payload shape | text-first (`text` / `tree` string) | structured `elements[]` first + `tree_markdown` for back-compat |
| Focus/selection/document | `focused_element`, `selected_elements`, `selected_text`, `document_text` | not exposed |
| Screenshots | window2: array of bounded screenshots with `zIndex`/origin for transient UI | one window screenshot per call |
| Window scoping safety | not observed in symbols | explicit refusals + degraded reasons |
| Per-element metadata | role/subrole/title/desc/value/placeholder/id | richer: frame, parent, depth, enabled, selected, min/max, actions, value_description, in_web_content |

- Matches: per-element integer addressing, one call pairs tree + screenshot, markdown-style tree text, "re-snapshot every turn before acting."
- cua ahead: structured `elements[]` with geometry and control state, opaque token with an explicit validity contract and a stale-error sentinel, strict window scoping, walk caps for huge Electron trees.
- Codex ahead: monotonic stable `elementID` (enables cross-turn reference and refetch matching), AX-tree diffing with removed-ID-range summarization and a line budget, focus/selection/document text, multi-screenshot z-ordered capture for transient UI.

### 5. Recreate/port plan

To reproduce Codex's observation behavior on cua:

1. AX-tree diffing (the large piece). cua has none. Add a per-`(pid, window_id)` previous-tree baseline cache (mirror the existing `element_cache` keying in `get_window_state.rs`) and a diff renderer in `rust/crates/platform-macos/src/ax/tree.rs` that emits added/removed/changed lines, with removed handles collapsed to ID ranges and a line budget. Add a `disable_diff` arg to the tool schema in `get_window_state.rs`. Effort: large-build. Copy the `disableDiff` wording and "prefer diff output" guidance verbatim from `codex-corpus/mac/skills/macos-SKILL.md` and `docs/sky-window-api.md` into the tool description.

2. Monotonic, cross-turn element identity. Sky's diff only works because IDs persist. Patch `rust/crates/cua-driver-core/src/element_token.rs` and `tree.rs` to add a stable-ID layer: match `AXNode`s across snapshots by AX identifier / role+frame / DFS path, reuse the prior ID on a match, and mint a new one only for genuinely new nodes. Surface `elementNoLongerValid`-style errors like Sky. This is a prerequisite for step 1's removed/changed lines to be meaningful. Effort: native-patch (large if the refetch matcher must be robust across virtualized lists).

3. Focus / selection / document text. Read `AXFocusedUIElement`, `AXSelectedText`, and the focused document text during the walk in `tree.rs`, and add `focused_element` / `selected_elements` / `selected_text` / `document_text` to the structured output in `get_window_state.rs`. Effort: thin-port.

4. Multi-screenshot transient-UI capture with z-order (window2 behavior). Extend the capture path and `get_window_state.rs` to return an array of bounded screenshots with `zIndex`/origin for popovers and menus. Effort: native-patch.

5. Docs/skill copy. cua already ships an indexed-tree tool; adapt the "re-snapshot each turn, re-derive indices, prefer diff, fall back to screenshot" guidance from `macos-SKILL.md` into cua's tool description and skills. Effort: copy-verbatim (adapted wording).

The observation foundation (indexed tree + paired screenshot + structured elements) is already present and ahead of Sky, so the net work to reach Codex-like behavior is concentrated in steps 1 and 2.
