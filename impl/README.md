# Implementation

The build of the invisible-computer-use layer. cua-driver already provides the core focus-free mechanism on all three OSes (verified in `win-eval2/`, `build/verify/`, `linux-eval/`); this tree adds the Codex/Sky parity delta the specs enumerate, as controlled components we own and can bundle.

## The product: `icu`

One workspace (`impl/Cargo.toml`), one crate that is the entry point (`impl/icu`), one install command:

```
impl/install.sh [--prefix ~/.local/bin] [--no-native] [--docker]   # icu + the patched cua-driver
icu mcp            # stdio MCP server: the window2 tool surface (verbatim names) + approval gate + memory + host state
icu call <tool> '<json>'   # the same code path, one call
icu doctor | icu tools | icu mcp-config
icu window2|confirmations|skysight|app-instructions ...   # the crates' dev verbs
```

Configure with `ICU_DRIVER`, `ICU_SOCKET`, `ICU_HOME` (state, approvals), `ICU_MEMORY_ROOT`, `ICU_DENIED_APPS`,
`ICU_FORBIDDEN_APPS`, `ICU_REQUIRE_APP_APPROVAL`, `ICU_ALLOW_ALWAYS`, `ICU_RISK_LEVEL` (see `icu/src/lib.rs`).
The library API is `icu::Layer::connect(Config)` then `layer.dispatch(tool, &args)`.

## Status

Verify everything with `impl/verify.sh` (add `--native` to rebuild the patched driver, `--live` for the live suites). The conformance `our` target runs through the `icu` binary. Cross-OS drives: `impl/native/linux-drive/` (Docker) and `impl/native/windows-drive/` (EC2).

Current audit fixes: [FIX-STATUS](../study/audit/FIX-STATUS.md). Offline verification passes 109 workspace and 1,001 native tests; latest cursor/capture/physical-input acceptance remains live-pending. The table below records historical slice evidence, not a new full-product acceptance claim. Required binaries now come from `impl/target/debug` and verification fails if absent.

| Slice | Spec | State | Test |
|---|---|---|---|
| `axdiff` — AX-tree diff engine + observation layer | [02](../specs/02-ax-diffing.md) | DONE; stateful layer (`axdiff::layer`) renders Sky's observed text (`axdiff::sky`), 8 captured Sky observations replay byte for byte, cross-OS adapter | `cargo test --manifest-path impl/axdiff/Cargo.toml` |
| `app-instructions` — per-app instruction packs | [04](../specs/04-per-app-instructions.md) | DONE; verbatim packs + first-touch injection wired into `axdiff-layer`, live-verified on Clock | `cargo test --manifest-path impl/app-instructions/Cargo.toml` |
| `window2` — observation surface facade | [01](../specs/01-observation-model.md), [19](../specs/19-observation-error-skysight-addendum.md) | DONE; golden WindowState/Window/App shapes over cua + the layer, gaps reported in `_gaps`; live 36/36 | `cargo test --manifest-path impl/window2/Cargo.toml` |
| `confirmations` — Confirmations Policy + app approval | [14](../specs/14-approval-confirmations.md) | DONE; verbatim policy texts, 26-tag taxonomy checked against the text, elicitation shape, session/always store format and roundtrip guards (no captured reference store for byte comparison) | `cargo test --manifest-path impl/confirmations/Cargo.toml` |
| `native/` — cua-driver patches: occlusion clip (08) + cross-platform AX diff lineage (02) | [02](../specs/02-ax-diffing.md), [08](../specs/08-cursor-overlay.md) | DONE; driver rebuilt (mac native, Linux Docker, Windows xwin), 10 native unit tests (core 6, mac revision 2, occlusion 2), lineage driven live on all three OSes, occlusion clip live PASS on macOS (`native/macos-drive-occlusion-evidence.png`) | `impl/native/apply-and-build.sh`, `impl/verify.sh --native` (or, inside `vendor-cua/libs/cua-driver/rust`, `cargo test -p platform-macos --lib`) |
| `icu` — one API / MCP entry point / install over all of the above | [13](../specs/13-protocol-tools-browser.md), golden `tool-surface-window2.md` | DONE; window2 surface verbatim, approval gate with MCP elicitation, stable-id routing, memory + host tools; `impl/install.sh` | `cargo test --manifest-path impl/Cargo.toml -p icu` |
| `skysight` — Skysight-style memory | [03](../specs/03-memory-skysight-vs-history.md) | DONE; verbatim prompts, summary contract + validator, 10min/6h windows, resources store, sticky-taint + descriptive-not-directive rules; model call/ambient capture/phase2 stubbed | `cargo test --manifest-path impl/skysight/Cargo.toml` |

## `axdiff`

Recreates Codex's `UIElementRenderDifference` / `UIElementTreeRevision.differenceLines`: stable cross-turn element identity (matched nodes inherit their id, new nodes draw the next id from a monotonic +1 counter) and the diff renderer that emits only changed/added/removed rows.

**Sky's text (2026-09-26).** The layer's model-facing text is `axdiff::sky`: the
`Window: "X", App: X.` header, ids from 0, tab-indented rows such as
`1 text field (settable) Value: 0, ID: calc-display` with `Secondary Actions:`, the menu bar,
the one-line diff header with `Removed element IDs:` and marker-before-tabs rows, the
no-change line, and the `Selected text:` / focused-element tail. Every rule and its evidence
from the real service are in `conformance/golden/ax-diff-format.md`. `tests/sky_replay.rs`
replays each captured observation (`conformance/golden/sky-replay/`) byte for byte, and
[the evidence](../study/audit/sky-format-2026-09-26/README.md) covers the rest. The rows need
fields the stock driver merges into `label`; the patched macOS driver reports them
(`title`, `description`, `help`, `subrole`, `settable`, `focused`, `tree_order`), and the
golden lists what still differs (Clock's collapsed containers). The legacy
renderer described below (`[id] AXRole "label"`, `-` rows) remains only because the native
driver's structured `diff` field builds against it.

What the legacy renderer does:
- Verbatim headers + legends (incremental and cumulative variants) — the exact strings the model must see.
- Per-line `~` changed / `+` added / `-` removed, in depth-first order; unchanged nodes are omitted.
- Stable element ids across a `Revision` lineage (`Revision::initial` then `Revision::next` per turn).
- Removed-as-ID-range compaction (`feature/axTreeDiffingRemovedElementIDRanges`): `Removed elements: 2-4, 6`.
- Two diff baselines, chosen by function (a bug found + fixed 2026-09-10): `render_diff` = incremental, vs the PREVIOUS turn (`-` lines for removed); `render_cumulative` = vs the INITIAL turn ("cumulative diff from the initial accessibility tree", removed as ID ranges). An earlier version wrongly used a flag that swapped the header but kept the previous-turn baseline; a multi-turn regression guard (`cumulative_is_from_initial_not_previous`) now pins it.
- Line-budget guard: past the budget it returns `FullFallback` so the caller renders the full tree (Codex's `differenceLines` returning nil).
- No-change short-circuit and per-call `disableDiff` bypass.

Run it:
```
cargo test --manifest-path impl/axdiff/Cargo.toml                 # 15 engine + 5 layer + 2 adapter tests; identity + cumulative-baseline guards fail-on-purpose-verified
cargo run --manifest-path impl/axdiff/Cargo.toml --example demo   # prints a realistic three-turn session diff
```

Grounding and honest limits: headers/legends/no-change strings are verbatim from the golden (themselves from the binary's rendered strings); the monotonic id and inherit-by-path model are from the decompile (`specs/20`). INFERRED and documented in the source: the exact line-budget constant (a parameter here) and the per-row text beyond the marker (the golden pins only the marker, not the row rendering). The node-match key is `(role, label)` scoped to the matched parent — robust for value changes and leaf add/remove, degrading to remove+add on a label change; the exact Swift matching heuristic is not recoverable.

Validated end-to-end on REAL cua-driver output (2026-09-10): the `cua` adapter (`src/cua.rs`, `axdiff-cua` binary) parses a real `get_window_state` `elements` array and diffs two live snapshots. Driven against Activity Monitor (self-updating), two snapshots 4s apart produced a correct incremental diff — 101 added / 15 removed, ids continuing monotonically from 141, and the 115 elements that "differed" by raw positional index collapsed to the true structural delta because content-keyed matching saw them as reindexed, not changed (the whole point of stable identity, confirmed on real data). Cumulative mode compacted a real `26-40` removed range; budget fallback and `disable_diff` both returned the full tree. Real-data finding, now documented in `src/cua.rs` and covered by `tests/cua_realshape.rs`: cua's `parent_index` and `depth` disagree for ~40% of nodes (rows carry `parent_index=0` but `depth=2` — cua flattens intermediate containers in `parent_index` while `depth` keeps true nesting); the adapter treats `parent_index` as authoritative.

Run the real adapter: `cargo run --manifest-path impl/axdiff/Cargo.toml --bin axdiff-cua -- turn1.json turn2.json --app "<name>" [--cumulative] [--budget N] [--disable-diff]`.

### Stateful layer over cua-driver (`axdiff-layer`)

The practical integration: rather than fork cua-driver's Rust internals, `axdiff-layer` wraps its `get_window_state`. It keeps a per-`(pid,window)` revision store on disk, so the FIRST observation of a window returns the full tree and each SUBSEQUENT one returns a diff with stable ids carried across turns — Codex's behaviour, layered on top. It honours cua's own `degraded`/empty contract (no diffing a mis-grounded tree).

```
axdiff-layer --pid P --window W [--cumulative] [--disable-diff] [--reset] [--state DIR] [--driver PATH]
```

Driven LIVE (2026-09-10) against Activity Monitor across three real turns: turn 1 `FULL(first)` established the lineage, turn 2 returned a real incremental diff (menu items appeared, ids continuing 76+), turn 3 the cumulative-vs-initial variant; state persisted to `<dir>/<pid>_<window>.{prev,initial}.json`. The persist->restore->next round-trip is guarded by a test so ids stay stable across the process boundary.

### Conformance loop closed

`conformance/`'s `OurLayerAdapter` now routes `get_window_state` through `axdiff-layer` and presents the Codex window2 tool names, so the golden contract gates the real code. `CONFORMANCE_TARGET=our CONFORMANCE_LIVE=1` passes 18/18 (incl. a deterministic golden-format gate feeding axdiff two controlled snapshots); the same suite against raw `cua` xfails exactly the 4 tests our layer adds (the 3 window2 names cua folds + the diff renderer), so the checks are meaningful, not vacuous.

### Cross-OS adapter

The `cua` adapter is validated on real output from TWO OSes: macOS (Activity Monitor) and Linux/aarch64 (a GTK fixture on the cloud box). Both expose the same adapter-relevant element keys (`element_index, role, label, depth`; macOS adds `parent_index/frame/selected`), and the local `axdiff-cua` parses the real Linux JSON cleanly. Windows' `get_window_state` uses the same `element_index/role` shape (win-eval); a live Windows tree wasn't re-captured after the instance was torn down.

Native integration (spec 02 steps 5-6) is DONE in `impl/native/`: the same engine runs inside cua-driver-core's `element_revision` lineage store and is wired into the macOS, Windows and Linux `get_window_state` tools (`disable_diff`/`cumulative_diff`, `elements[].stable_id`, structured `diff`), driven live on all three OSes.

## `app-instructions`

Spec 04. The seven shipped playbooks (`packs/*.md`, byte-identical to the corpus and to the live bundle's sha256s) keyed by bundle id, and the first-touch `<app_specific_instructions>` prefix. `src/lib.rs` carries a line-for-line port of `sky_js`'s `window_result.js` wrapper (`window_result_text`: bundle-id keyed seen-Set, app-name fallback, the Numbers exclusion Set) and the window2/native-MCP delivery model (`DeliveryState::deliver`, mirroring `bundleIdentifiersWithDeliveredInstructions`). `axdiff-layer` resolves the pid through cua's `list_apps`, delivers once per session (session = state dir), and logs `APP_INSTRUCTIONS(first:<file>|seen:<file>|none)` on stderr.

Driven LIVE on Clock (2026-09-10): turn 1 returned the verbatim `Clock.md` prefix before the full tree, turn 2 returned the plain no-change line, and Activity Monitor (no pack) never got the tag. Conformance: `our` 27/27 live; raw `cua` xfails the 9 spec-04 checks on top of the 4 it already lacked.

Honest limits: Notion's bundle id is the standard `notion.id` (unverified locally); the Spotify runtime supplement (spec 04 step 5, optional) is not built; whether native strips the pack's trailing newline is unknown (we pass the bytes verbatim).

## Audit behavior changes

UI readiness now uses the shared `ui-readiness` policy. After a delivered action,
the app waits for consecutive quiet, non-busy observations and uses the last
snapshot for its next decision. Initial idle reads keep the transport deadline;
post-action polling has a two-second budget, a 120 ms quiet interval and 60 ms
polls. These are local policy values, not a claim of timing equivalence. An
authorized, validated screenshot can support a canvas whose accessibility tree
is empty. The policy polls available observations; it does not subscribe to native
accessibility notifications. Slow synchronous reads cannot be preempted.

The CLI's optional `settle` uses a private observation baseline, preserving the
next public diff and app instructions. Polling invalidates cached action targets;
read a fresh public state before further input. Mixed file/UI verification uses
one budget and brackets UI samples with matching file/process observations. This
provides bounded freshness, not an atomic snapshot of the desktop and filesystem.

Automatic screenshot omission requires requested text, prior image delivery to
the same session/PID/window, explicit complete accessibility coverage, and no
identified visual risk. Partial or unknown coverage retains images, including
current native bounded projections. Automatic mode still captures once; it only
controls which image is returned. Explicit screenshot requests take precedence.

Driver initialization and health/discovery each have a 1.5-second wall-clock cap; ordinary exchanges default to 15 seconds. `ICU_DRIVER_TIMEOUT_MS` can lower probe budgets or change the ordinary budget (positive, at most 300000 ms). This is independent of the advisory action `deadline_ms`. Transport failures are terminal and typed; input is never replayed implicitly. Capabilities remain locally available after setup failure.

AXIdentifier-supported relabels preserve IDs. Snapshot tokens are per-observation; ambiguous replacements use fresh IDs. Native and facade cumulative diffs include label changes. Every macOS animation route uses the shared detach policy; actual input completion and existing keyboard-focus restoration remain awaited.

Native screenshot dimensions and origins are logical points, with separate pixel dimensions/scales. Transient captures use explicit visible screen rectangles and are labeled as composites; they are not isolated hidden textures. Layer/list selection and decoded geometry have offline guards, while actual capture/ghost-cursor/hardware input acceptance remains gated on the prepared bounded harness.

Pixel actions in the ICU layer require a target capture with valid returned-image geometry. Call `get_window_state` with `include_screenshot: true` before coordinate input or simulated element clicks, including after a text-only observation or a cache written by an older build. The layer uses returned-image pixels per logical point, accounting for screenshot resizing. Missing geometry is refused instead of using the primary display scale. The driver reads an unbound pixel only against the calling session's own capture ([c9-unify](../study/audit/c9-unify-2026-09-27/README.md)). One icu process is one driver session: its `get_window_state`, its settle polls and its input all carry the label `icu-<ICU_SESSION>-<pid>`, so one `icu mcp` lands its own unbound pixels on macOS, Linux and Windows. A one-shot `icu call` is a new process and so a new session: pass that `get_window_state`'s `screenshotId` with the coordinates, or keep one `icu mcp` session. The driver's refusals reach icu callers with icu's names (`screenshotId`, `icu call`, `icu mcp`). On Linux and Windows the driver sends the capture only as an MCP image part; icu merges it into the payload as `screenshot_png_b64`, as `cua-driver call` does ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md)). See [coordinate evidence and remaining limits](../study/audit/layer-coordinates-2026-09-23/README.md).

September13 follow-up: `McpChild` delegates construction, requests, kill/reap and Tokio teardown to one OS transport worker. It can be embedded synchronously inside an existing Tokio task without nested-runtime panic; async callers may still use `spawn_blocking` to avoid blocking their executor. Deadlines and terminal/no-replay behavior remain enforced. `get_window_state.ax_diagnostics` now preserves raw AXWindows errors/counts and the observing driver's own trust state. Independent observer comparison is prepared but blocked by the locked host; no AX cause is asserted.

## Current scoped macOS acceptance (September13)

[Final bounded evidence](../study/audit/finish-acceptance/README.md) supersedes earlier pending/failed live notes above. Corrected run113414Z passes exact pixel[1], double[1,2], middle one pair, wheel two−1 ticks/20-point scroll, background AppKit drag selection, three immediate menu captures and fresh-token/native/window2 `quick` readback. Actual mint-cursor/menu PNGs are inspected; final physical typing remains pending. Background mouse paths use one public PID transport, no automatic replay. Native focus cleanup preserves changed pointer/HID activity/current owner while retaining the2/1 inverse. The harness observes actual text tracking events and exact handler completion; it preserves uncertain desktop state through cleanup. Browser/foreground variants and other physical scales have no new live acceptance claim.
