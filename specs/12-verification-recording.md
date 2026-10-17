## Verification (verify_state) and trajectory recording/replay

### What it is

Two subsystems that together form our verifier foundation. `verify_state` is a deterministic, bounded, tri-state predicate checker: it polls one explicitly authorized window and answers satisfied / unsatisfied / unknown, where unknown never implies success. Trajectory recording writes a per-action `turn-NNNNN/` folder with before/after AX state and screenshots plus an `action.json` that carries the delivery truth, and `replay_trajectory` re-invokes those turns. Codex/Sky ships neither of these; its "Record & Replay" is an unrelated UI-driven skill generator, and its verification model is to re-capture `get_window_state`.

### Codex/Sky

Codex has no `verify_state` tool and no before/after trajectory recorder. Confirmed by exhausting the tool surface and the corpus:

- `.../sky_js/src/sky.js`, `service.js` — the shipped MCP tool names are `get_window_state`, `get_app_state`, `list_windows`, `click`, `type_text`. No `verify_state`, no `start_recording`, no `replay_trajectory`. The only `*record*` symbols are `StartAudioRecording` / `StopAudioRecording` (audio, not trajectory). CONFIRMED.
- `codex-corpus/mac/skills/macos-SKILL.md:104` ("try fetching the latest `get_app_state(...)` to make sure you have the latest context") and `skills/windows-SKILL.md:90` ("capture a new state when you need to verify progress"). Codex's entire verification story is re-capturing the AX snapshot and letting the model judge it. There is no structured predicate, no tri-state, no stability gate. CONFIRMED.
- `SkyComputerUseService` binary, `strings | grep -i verify` — only `CodexSecurityNativeVerifyFix`, and Messages "could not verify whether the send completed". No verification engine. CONFIRMED.
- `SkyComputerUseService` binary, `strings | grep -i "RecordAndReplay"` — `RecordAndReplayOverlayController`, `RecordAndReplayRecordingControlsController`, analytics `CodexRecordAndReplayRecordingStarted/Ended`. This is a floating UI overlay that records the *user's* actions into an event stream, then an LLM "Draft a concise replay plan... prefer an available connector or dedicated tool... use Computer Use for unsupported UI interactions or visually dependent verification" writing `{{EVENT_STREAM_DIRECTORY}}/record-and-replay-skill.md`. It is semantic skill authoring, not a state verifier or a per-turn evidence recorder. CONFIRMED.
- `codex-corpus/mac/ComputerUse/SkysightSummarizer.md` — Skysight is a passive event-stream memory summarizer ("records the user's activity through an event stream to provide passive context"). It emits memory prose, not verification outcomes, and it treats all observed content as untrusted. Adjacent but not a verifier. CONFIRMED.

Net: there is nothing in Codex to copy for either verification or before/after recording. The only recording-shaped asset is the Record & Replay skill-generation prompt, which is orthogonal to our verifier.

### cua-driver

cua-driver has a full contract-defined verifier plus a reserve/dispatch/write recorder and a renderer.

- `cua-driver-contract/src/verification.rs` — the wire contract. `VerifyStateInput { pid, window_id, expect: Vec<StatePredicate> (1..=8, logical AND), session, timeout_ms (0..=10000, default 5000), stable_samples (1..=5, default 2), include_screenshot }`. Predicates are flat: `WindowPredicate { exists, bounds: BoundsExpectation{ x,y,width,height,tolerance_px } }` and `ElementPredicate { selector:{ role, label_contains }, exists (schema `enum:[true]`, absence unprovable), value_equals, enabled, selected }`. Output `VerifyStateOutput { status, stable, elapsed_ms, samples, predicates:[PredicateOutcome{ index, status, unknown_reason, observed_json }] }`. Enums `VerificationStatus { satisfied, unsatisfied, unknown }` and `UnknownReason { invalid_predicate, unsupported_predicate, untrusted_source, multi_match, target_missing, observation_unavailable, stability_unproven }`.
- `cua-driver-core/src/expectation.rs` — the engine. `VerifyStateTool::invoke` validates bounds, then polls at `POLL_INTERVAL_MS=100` until `stable_samples` consecutive satisfied samples or the deadline. `ToolObservationProvider::observe` reuses the real `list_windows` + `get_window_state` tools with an internal `_observation_only:true` flag so verification never mutates the shared element/token cache and one admission owns the whole window. `evaluate_element` is deliberately conservative: web-area / `in_web_content` readback returns `untrusted_source`; a property check against >1 match returns `multi_match`; absence is only `unsatisfied` when `elements_complete`, else `target_missing`/`observation_unavailable`; `normalized_role` maps AXCheckBox/checkbox/"check box", push button->button, page tab->tab. `aggregate_status` = unsatisfied > unknown > satisfied. A last-sample-satisfied result that never proved stability is downgraded to `stability_unproven`.
- `cua-driver-core/src/recording.rs` — the recorder. `RecordingSession`/`RecordingInner` own live state; a `PendingTurn` is reserved before dispatch so both phases share one `turn-NNNNN/` even under out-of-order completion. `write_turn` emits `action.json` (`tool`, `arguments`, `result_summary`, `result_error`, timestamps, `click_point`, and `action_truth` from `ActionExecutionRecord::debug_json`), `before/after_state.json`, `before/after.png`, `click.png` + `click_source.png`, and legacy `app_state.json`/`screenshot.png` aliases. `write_evidence_manifest` writes `evidence.json` schema `cua-turn-evidence/v1` with per-artifact `captured` / `unavailable(classification)` / `not_applicable` status. `strip_internal_keys` drops `_`-prefixed args; `owner` (=`_session_id`) gates teardown so one session's disconnect can't stop another's recording.
- `cua-driver-core/src/recording_tools.rs` — MCP surface: `start_recording`, `stop_recording`, `get_recording_state`, and `replay_trajectory` (re-invokes each `turn-NNNNN/action.json` in lexical order via the normal dispatch path; documents that element-indexed clicks don't survive across sessions but pixel clicks and keyboard replay cleanly, and that recording a replay against a new build and diffing the two trajectories is the regression workflow).
- `cua-driver-core/src/recording_loader.rs`, `recording_render.rs`, `recording_zoom.rs` — post-hoc render pipeline: `load()` reads `session.json` + `cursor.jsonl` + click turns into `LoadedTrajectory`; `recording_render.rs` builds an ffmpeg `sendcmd` filter graph; `recording_zoom.rs` is pure zoom-on-click math (cubic bezier). Cosmetic, not part of verification.
- `platform-macos/src/recording_hooks.rs` — `app_state_json_for` walks the AX tree for (pid, window_id) to produce `app_state.json`; `element_window_local_xy` resolves `element_index` to window-local pixels so `click.png` gets a marker on AX-indexed clicks. macOS video is native ScreenCaptureKit (no extra TCC prompt), per `start_recording` describe output.

### Delta

| Capability | Codex/Sky | cua-driver |
|---|---|---|
| Structured state predicates | none (re-capture `get_app_state`, model judges) | `verify_state` window+element predicates, `deny_unknown_fields` schema |
| Tri-state / unknown-never-success | none | `VerificationStatus` + 7 `UnknownReason` codes |
| Stability gating | none | `stable_samples` consecutive-satisfied poll loop |
| Untrusted-source / web-area guard | none | `target_is_in_web_area`, `element_source_trusted` |
| Per-action before/after evidence | none | `turn-NNNNN/` + `evidence.json` `cua-turn-evidence/v1` |
| Delivery truth on each action | none | `action_truth` (transport, route, effect, delivered_count, attempts) |
| Deterministic replay + diff | none | `replay_trajectory` |
| Record user workflow -> skill | Record & Replay overlay + `record-and-replay-skill.md` | none |
| Passive activity memory | Skysight summarizer | none |

Where cua is ahead: everything the verifier board needs already exists and is contract-tested. Where Codex is ahead: nothing for verification; its only relevant asset is the Record & Replay semantic-skill generator, which is a different product surface we do not need to reach OSWorld-style verification.

### OSWorld-style verifier board on cua

Map the seven-axis taxonomy onto what cua already emits; only the last axis needs new code.

1. **delivery** — did input actually reach the app. Read `action.json.action_truth` (`route`, `transport`, `effect: confirmed|unverifiable`, `actual_delivery`, `delivered_count`, `attempts`). Already recorded, no patch. Board rule: fail delivery if `result_error` or `effect != confirmed`.
2. **focus** — right window/element active. `verify_state` `window.exists` plus `element.selected`/`enabled`; corroborate with `before_state.json` vs `after_state.json`. No patch.
3. **cursor** — pointer/click landed where intended. `cursor.jsonl` sampler + `action.json.click_point` + `click.png` marker. No patch.
4. **capture fidelity** — was the observation real. `evidence.json` `captured|unavailable|not_applicable` with `classification` (`capture_failed`, `privacy_suppressed`, `no_target_pid`). Board rule: any `unavailable` on an expected artifact degrades that turn to unknown. No patch.
5. **semantic state** — element value/role/label is correct. `verify_state` `ElementPredicate` (`value_equals`, `enabled`, `selected`), honoring `untrusted_source`/`multi_match` as unknown. No patch.
6. **persistent state** — filesystem/DB/network side effects. GAP: `verify_state` is window-scoped only (`expectation.rs` reads AX/window, nothing else). Nearest module is none. Needs a new out-of-window predicate source.
7. **end-to-end** — whole task succeeded. Compose `replay_trajectory` + trajectory diff, gated by the six axes above per turn. No new tool, needs a harness.

Concrete steps:

- **Board harness (native-patch, new crate/bin):** write an OSWorld runner that drives `start_recording` -> action tools -> `verify_state` per checkpoint -> `stop_recording`, then scores each `turn-NNNNN/` against the seven axes from `evidence.json` + `action.json.action_truth` + the `verify_state` outcome. Pure consumer of existing artifacts; touches no core file. Effort: native-patch.
- **Persistent-state predicates (native-patch):** extend `cua-driver-contract/src/verification.rs` with a non-window predicate variant (e.g. file-exists / file-content-hash / process-exists) and a matching evaluator branch in `cua-driver-core/src/expectation.rs` guarded behind an explicit capability, since these escape the "one authorized window" invariant. Add a new `ObservationProvider`-style source. Effort: native-patch, contract change.
- **Golden-trajectory regression (thin-port of existing workflow):** use the documented `replay_trajectory` + diff pattern (`recording_tools.rs`) as the board's E2E lane; add a diff step comparing two run directories. Effort: thin-port.
- **Optional Record & Replay skill-gen (large-build, only if wanted):** copy `codex-corpus/mac` Record & Replay / Skysight prompt text verbatim as reference, but the event-stream capture, overlay, and LLM skill authoring are a separate product surface with no cua counterpart. Not required for the verifier board. Effort: large-build; defer.

No Codex file is copied for verification. The verifier board is built on cua's own contract and recorder.
