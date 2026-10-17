# Golden: the accessibility text Sky gives the model

Source: the no-login probe of the real `SkyComputerUseService` (26.915.1001093, driven
through the real `codex app-server` → `node_repl` → `@oai/sky`, macOS 15.7.5), in
`study/audit/golden-trace-prep-2026-09-25/validation/probe/`. Citations:

- `ipc:N` is line N (1-based) of `sky-ipc.jsonl`, the service's own reply.
- `res:KEY` is `results.json[KEY]`, the text the JS returned to the caller.

**Observed** means the capture shows the rule directly. **Inferred** means it fills a gap
the capture leaves open; each one says what it rests on. `conformance/golden/sky-replay/`
replays every captured observation through our layer, and `impl/axdiff/tests/sky_replay.rs`
compares the result byte for byte.

## Full tree

| # | Rule | Status | Evidence |
| --- | --- | --- | --- |
| H1 | The first line is `Window: "<window title>", App: <app display name>.` | Observed | ipc:8, ipc:24, ipc:44 |
| H2 | An untitled window renders `Window: "", App: <app>.` | Inferred | no untitled window captured |
| T1 | One row per element. Depth-first. A row is indented with one `\t` per depth, and the root has none. | Observed | ipc:8 (window at 0, children at `\t`), ipc:44 (depth 5 under Clock's containers) |
| T2 | Every row ends with `\n`, including the last, so a full tree ends in a newline. | Observed | ipc:44 ends `...ID: com.apple.menu.help\n` |
| T3 | At most 60 levels (depth 0 to 59). Rows deeper than that are placed on the last level, in walk order, under their ancestor at depth 58. A row reached twice (a duplicate index) is shown once. | Inferred (ours) | The capture's deepest row is at depth 5. The cap keeps every tree walk bounded (a chain of 1000 overflowed the stack, review-2026-09-26 app-r3 S-D) and keeps the saved lineage loadable: serde_json refuses nesting past 128, which 64 levels reach. |
| I1 | Ids start at 0 at the first row and count up by one in row order. | Observed | ipc:8 `0 standard window`, `1 text field`, ... `17 text` |
| I2 | Across turns, a matched element keeps its id and a new one takes the next unused id, past the previous maximum. | Observed | ipc:48: containers keep 8 and 14; new rows get 36-40 after Clock's 0-35 |
| M1 | The app's menu bar is a second root after the window: `29 menu bar`, with its items at depth 1. An app with no menu bar (MiniCalc runs as an accessory app) has none. | Observed | ipc:44; ipc:8 |
| M2 | Menu bar items show no children (the closed menus under them). An open item, one the driver reports `selected` (AXSelected), keeps its menu and the menu's items. | Inferred; the open case is ours, as no open menu was captured | ipc:44 lists only the six items. The stock driver walks each item's `AXMenu` (`study/audit/review-2026-09-25/macos-evidence/mac/runs/ffbd39a-identity/turn-w0.json`, element 20). |

## Row grammar

`<id> <role>[ (settable)][ <title>]<fields>`. `<fields>` is the non-empty subset of the
list below, joined with `, `. It follows the title with `, `, or follows a bare role with one space.

1. `Description: <AXDescription>`
2. `Value: <AXValue>`
3. `ID: <AXIdentifier>`
4. `Help: <AXHelp>`
5. `Secondary Actions: <a>, <b>, ...`

| # | Rule | Status | Evidence |
| --- | --- | --- | --- |
| R1 | Title inline after the role, then `, ` before fields | Observed | `0 standard window MiniCalc, Secondary Actions: Raise` (ipc:8), `2 button 7, ID: calc-btn-7` |
| R2 | No title: one space between role and fields | Observed | `2 container Secondary Actions: Cancel`, `21 radio button Description: World Clock, Value: 1` (ipc:44) |
| R3 | `(settable)` after the role for a settable value, before the fields | Observed | `1 text field (settable) Value: 0, ID: calc-display` (ipc:8) |
| R4 | Order Description, Value, ID | Observed | `9 element Description: Daytime, Value: 3:02 PM, ...`, `8 container Description: Cupertino, 3:02 PM, ID: Cupertino, ...` (ipc:44, ipc:48), `Value: 0, ID: calc-display` |
| R5 | Help before Secondary Actions | Observed | `27 full screen button Help: this button also has an action to zoom the window, Secondary Actions: zoom the window` (ipc:44) |
| R6 | Help after ID | Inferred | no row carries both |
| R7 | A static text's content is its inline name, not a `Value:` field | Observed | `17 text MiniCalc` (the title bar text, ipc:8) |
| R8 | `(settable)` combined with a title | Inferred | not captured; rendered `text field (settable) Name, Value: ...` |
| R9 | Menu bar items have no role word: `30 Clock, ID: com.apple.menu.application` | Observed | ipc:44 |
| R10 | A row with nothing but a role: `16 zoom button`, `26 close button`, `29 menu bar` | Observed | ipc:8, ipc:44 |
| E1 | A control character in any field is escaped, so a row stays one line: newline, carriage return and tab read `\n`, `\r` and `\t`, and any other control character, and U+2028 and U+2029, read `\u{hex}`. Text without one is unchanged. | Inferred | No captured field holds a control character, quote or backslash (every row and header in ipc and appserver decoded). Without it, a value holding `\n\t7 button Delete all` rendered a row of its own (review-2026-09-26 app-r3 S-C). |
| E2 | The quoted window title in the header, the diff header and the no-change line also escapes `"` and `\` as `\"` and `\\`; the app name gets E1 only | Inferred | as E1; a title `a", App: Evil.` otherwise rewrote the header |

### Role names

Observed pairs come from MiniCalc (its AppKit classes are in
`app/src-tauri/tests/fixtures/minicalc.swift`) and Clock. The raw role is Inferred where
the capture shows only the name.

| Raw role / subrole | Name | Status |
| --- | --- | --- |
| AXWindow / AXStandardWindow | `standard window` | Observed (NSWindow) |
| AXTextField | `text field` | Observed (NSTextField) |
| AXButton | `button` | Observed (NSButton) |
| AXButton / AXZoomButton, AXCloseButton, AXMinimizeButton, AXFullScreenButton | `zoom button`, `close button`, `minimize button`, `full screen button` | Observed names, standard subroles Inferred |
| AXStaticText | `text` | Observed (title bar text) |
| AXGroup | `container` | Observed name, raw role Inferred |
| AXRadioButton, AXMenuButton, AXToolbar, AXMenuBar, AXHeading | `radio button`, `menu button`, `toolbar`, `menu bar`, `heading` | Observed names, raw roles Inferred |
| AXUnknown | `element` | Inferred (Clock's world-clock row) |
| everything else | AppKit's role description (`checkbox`, `pop up button`, `row`, ...); unknown roles lose `AX`, split camel case and go lower case | Inferred |

### Secondary actions

| # | Rule | Status | Evidence |
| --- | --- | --- | --- |
| A1 | AXRaise → `Raise`, AXCancel → `Cancel`, AXScrollDownByPage / AXScrollUpByPage → `Scroll Down` / `Scroll Up` | Observed | ipc:8, ipc:44 |
| A2 | A custom action (`Name:Remove clock\nTarget:...`) shows its name: `Remove clock` | Observed name. The raw form comes from the stock driver's Clock capture `conformance/fixtures/clock_gws.json` | ipc:44 |
| A3 | The full screen button's zoom action reads `zoom the window` | Observed | ipc:44 |
| A4 | AXPress is never listed | Observed | every `button` row in ipc:8 |
| A5 | AXShowMenu, AXConfirm and AXScrollToVisible are not listed | Inferred | On the same controls the stock driver reports `AXShowMenu, AXConfirm` (NSTextField, turn-w0.json element 1), `AXShowMenu` (Clock toolbar and full screen button, clock_gws.json). Sky lists none of them (ipc:8, ipc:44). |
| A6 | The menu bar and its items list no actions | Inferred | the driver reports `Cancel`, `Press`, `Pick` there. Sky lists none (ipc:44). |
| A7 | Other AX actions: Scroll Left/Right, Increment, Decrement, Expand, Collapse, Pick | Inferred | labels named in `tool-surface-window2.md` (`perform_secondary_action`) |

Ours: `perform_secondary_action` takes a listed label (case-insensitive) and sends the driver the
AX action that label came from (`zoom the window` → `AXZoomWindow`, `Remove clock` → the custom
action's full `Name:...` description). The driver turns an unknown word into AXPress, so
sending the label itself would press the full screen button instead of zooming.

## Diff turns

| # | Rule | Status | Evidence |
| --- | --- | --- | --- |
| D1 | Header, one line: `The following is a diff from the previous accessibility tree for Window: "<title>" with ~ and + representing changed and added elements, respectively. Removed elements are summarized by ID range.` | Observed | ipc:16, ipc:32, ipc:48 |
| D2 | If anything was removed, the next line is `Removed element IDs: ` followed by consecutive ids compacted to `a-b` and joined with `, ` | Observed | `Removed element IDs: 4-7, 9-13, 15-19` (ipc:48) |
| D3 | There are no `-` rows. Removals appear only in D2. | Observed | ipc:48 |
| D4 | A changed or added row is the marker (`~` or `+`), then the row's depth tabs, then the full row | Observed | `~\t1 text field (settable) Value: 7, ID: calc-display` (ipc:16), `+\t\t\t\t36 heading ...` (ipc:48) |
| D5 | Rows come in depth-first order of the new tree, with changed and added rows interleaved. An added subtree lists every row in it. | Observed | ipc:48: `+36, +37, +38, ~8, +39, ~14, +40` |
| D6 | A row is changed when any rendered field changes (a container that gains a Description) | Observed | ~8, ~14 in ipc:48 |
| D7 | The body has no trailing newline | Observed | ipc:48 ends on the `+40` row |
| D8 | A second read with no action in between can still be a diff (Clock fills its tree lazily) | Observed | ipc:48 (`res:clock_second_read`) |
| N1 | Nothing changed: `There has been no change in the accessibility tree for Window: "<title>".` with no trailing newline | Observed | ipc:20, ipc:40 |
| O1 | `disableDiff: true` returns the full tree with the same ids | Observed | ipc:23 request, ipc:24 |
| C1 | Cumulative variant: `The following is a cumulative diff from the initial accessibility tree for Window: "<title>"` + the D1 legend, with D2-D7 against the lineage's first turn | Inferred | the prefix is a binary string (spec 02). The capture has no cumulative turn. |
| B1 | Past a line budget the diff falls back to the full tree | Inferred | `differenceLines` returns nil (spec 02). The budget constant is not recovered, and ours is 200. |

The strings our conformance gate pins (`test_conformance.AX_DIFF_MARKERS`):

- `The following is a diff from the previous accessibility tree for Window: `
- ` with ~ and + representing changed and added elements, respectively. Removed elements are summarized by ID range.`
- `There has been no change in the accessibility tree for Window: `

## After the tree: selection and focus

| # | Rule | Status | Evidence |
| --- | --- | --- | --- |
| S1 | With a text selection, the text continues `\nSelected text: ```\n<selection>\n```\n\nNote: Pay special attention to the content selected by the user. If the user asks a question or refers to the content they are looking at on-screen, they might be referring to the selected content (but they might be referring to something else that's visible, too).` | Observed | ipc:8 (after a full tree, giving a blank line), ipc:16 (after a diff), ipc:20 (after the no-change line) |
| S2 | Without a selection, a focused element gives `\nThe focused UI element is <row without indentation>` | Observed | ipc:32, ipc:40 |
| S3 | A selection wins over the focused line. Neither → nothing. | Inferred | the selection turns (ipc:8-20) show no focused line, and the display was focused. Clock (no focus in the window) shows neither (ipc:44, ipc:48). |
| S4 | The pack wraps everything: `<app_specific_instructions>\n<pack>\n</app_specific_instructions>\n<text>`, first touch per session only | Observed | `res:clock_first_touch` vs ipc:44 (`appSpecificInstructions`), `res:clock_second_read` (none) |

## What our driver supplies, and where it cannot match

Sky reads AX itself. We render from the driver's `get_window_state` document, so a
rule can only be met when the driver reports the field. The patched macOS driver
(`impl/native/cua-driver.patch`, 2026-09-26) adds, on `elements[]` and
`passive_elements[]`:

| Field | AX source | Feeds |
| --- | --- | --- |
| `title`, `description`, `help` | AXTitle, AXDescription, AXHelp, unmerged (`label` still merges them) | R1, R2, R4, R5, R7 |
| `subrole` | AXSubrole, read for AXWindow, AXButton, AXTextField, AXCheckBox and AXRadioButton only | `standard window`, `zoom button`, ... and I2 |
| `settable` | AXValue settability, probed for the value-control roles | R3 |
| `focused` | the app's AXFocusedUIElement, compared by AX identity with the walk's actionable rows | S2 |
| `tree_order` | the row's position in the walk, on both arrays | T1 ordering when passive text sits between controls |
| `container_identifier` | AXIdentifier of the nearest collapsed AXGroup/AXScrollArea between the row and its rendered parent | I2 for rows SwiftUI rebuilds inside an identified group (below) |
| `inherited_actions` | the row's scroll actions that an ancestor, rendered or collapsed, also reports | A-rules: a scroll action is listed once, on the element that scrolls |

Without these fields (the stock driver, Linux, Windows) the renderer falls back to
`label` as the inline name, the raw role humanized, no `(settable)`, and a focused row
only when the driver's `focused_element` line names exactly one row.

Three replays pin this. `sky-replay/golden-trace-prep-2026-09-25.json` gives every Sky row
as an element, so it tests the renderer and lineage on Sky's own data (8 turns, all byte
for byte). `sky-replay/golden-trace-prep-2026-09-25.driver-shape.json` is MiniCalc's tree
rewritten into what the patched driver emits
(`study/audit/sky-format-2026-09-26/driver_shape.py`): its 6 turns match byte for byte.
`sky-replay/clock-live-2026-09-26.driver.json` is Clock as the patched driver emitted it
live (mac-review3-fixes-2026-09-26); its 2 turns are pinned known gaps. It replaced a
Clock document that `driver_shape.py` had synthesized from Sky's rows.

Rules the live Clock capture added, each Inferred (Sky's own rows for the same controls,
not its code):
- `AXGenericElement` renders `element` (Sky's world-map points).
- A scroll action (`Scroll Down`, `Scroll Up`, ...) is listed on the element that scrolls,
  not on the rows inside it that answer with the same action: Sky lists them on one
  container and on none of its 16 descendants.
- A nameless leaf with its parent's role, inside its parent's frame, is the same control
  twice (SwiftUI wraps a toolbar menu button in another) and renders as one row.
- A row inside a collapsed container that has an AXIdentifier is matched across turns by
  that identifier, its role, its position among the container's rows of that role, their
  count and its subtree's shape. This is what keeps Clock's city rows (`ID: Cupertino`) on
  one id while their time changes each minute, as Sky keeps the container's id.

| # | Gap | Why | Status |
| --- | --- | --- | --- |
| G1 | Clock's `container` rows are missing, with their `ID:` and `Secondary Actions:`, and every later id shifts | the driver collapses AXGroup and AXScrollArea (`platform-macos/src/ax/tree.rs`). Changing that renumbers `element_index` for every consumer of the driver, so it stays. | pinned in `sky-replay/known-gaps/` |
| G2 | Rows under a node that has no action keep the nearest actionable ancestor as parent, so they can render shallower than Sky's depth | `parent_index` points at actionable rows only | pinned with G1 |
| G3 | Subroles of roles outside the five above render the plain role name | one AX read per row is spent only where a captured row needed it | Inferred to be rare |
| G4 | Clock's world-map points get a new id each minute | they sit under a heading with no identifier, and their position cannot tell a changed time from a removed city and an added one. Sky's instructions for Clock say to ignore that list | pinned in `impl/axdiff/tests/clock_live.rs` |

## Where this lands in our window2 surface

- `accessibility.tree` carries the whole text above: pack, tree or diff, and tail. This
  placement is Inferred. The capture is of `get_app_state` (`AppState.text`). window2's
  `tree` is documented as the "existing formatted accessibility tree text, including
  element indexes and tab hierarchy".
- `accessibility.selected_text` is the raw selection. `accessibility.focused_element` is the
  S2 row, and `accessibility.selected_elements` lists rows in the same form.

The legacy `[id] AXRole "label" = "value"` format survives only in the patched driver's
structured `diff` field (`impl/native/cua-driver.patch`). The driver builds against
`axdiff`'s legacy renderer, and `icu` does not read that field.
