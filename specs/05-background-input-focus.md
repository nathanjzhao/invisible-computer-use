## Background input ladder, focus preservation, off-Space

### 1. What it is

The logic that lets the agent act on a window without stealing the user's focus, moving the real pointer, changing Space, or mutating a sibling window of the same process. It is a ladder of delivery routes ordered from safest to most invasive, a reactive focus-steal suppressor that restores the user's frontmost app if a target self-activates, and a hard limit: a window that is off the active Space or otherwise absent from the process's live `AXWindows` is observation-only and refuses all input.

### 2. Codex/Sky

Codex hides the entire ladder inside the native service. The model-facing API is window-addressed and exposes no delivery mode, no refusal codes, and no off-Space concept. Input auto-activates its target, and `activate_window` is offered as an optional escape hatch. The reactive focus-steal preventer, the frontmost tracker, and the Space and deminiaturize observers all live in the compiled `SkyComputerUseService` binary.

- `codex-corpus/mac/docs/sky-window2-api.md` — `activate_window(input): Promise<void>` documented as "Optional escape hatch to bring an open window to the foreground; input methods activate their target window automatically." No `delivery_mode`, no background/foreground rung, no off-Space refusal in the type surface.
- `codex-corpus/mac/skills/macos-SKILL.md` — the input verbs (`click`, `type_text`, `press_key`, `set_value`) take only `app` plus element/coords. No focus or Space parameters. `get_app_state` "transparently launches the app in the background."
- `SkyComputerUseService` (strings) — `SystemFocusStealPreventer` (Swift `_TtC20AccessibilitySupport25SystemFocusStealPreventer`), `SystemFrontmostApplicationTracker`, `FocusStealSuppression`, `frontmostApplicationObserver`, `lastViewBridgeFocusStealWasSuppressed`. Confirms the reactive suppressor plus a live frontmost tracker and explicit handling of out-of-process ViewBridge focus steals.
- `SkyComputerUseService` (strings) — `SetFrontProcessOptions`, `kCPSNotifyNewFront`, `kCPSNotifyKeyFocusChanged`, `kCPSNotifyKeyFocusTaken`, `kCPSNotifyLostKeyFocus`, `orderFrontRegardless`, `makeKeyAndOrderFront:`. The CPS focus-event constants are the same Carbon PSN key-focus machinery cua uses for focus-without-raise.
- `SkyComputerUseService` (strings) — `activeSpaceDidChange:`, `_workspaceID`, `isOnScreen`, `AXWindowDeminiaturized`, `windowDidDeminiaturize:`, `AXManualAccessibility`, `AXEnhancedUserInterface`. A live Space and window-state model plus the AX enablement attributes.
- `SkyComputerUseService` (`nm -u`) — the SLPS/SLS/CGS SPIs are absent from the undefined-import table, so Codex resolves them at runtime via `dlsym`, exactly as cua does. CONFIRMED they are not statically imported; runtime resolution is INFERRED from that plus the `strings` hits.

Net: Codex's default posture is activate-target-then-act, cushioned by `SystemFocusStealPreventer` restoring the user's prior frontmost. Because activation makes the target follow to the current Space, Codex effectively performs an implicit off-Space gather where cua would refuse.

### 3. cua-driver

cua splits the same behavior into a pure decision core plus a macOS shell, and it makes every rung and refusal explicit and unit-tested. The focus-steal preventer is a direct Rust port of Codex's Swift class.

- `rust/crates/cua-driver-core/src/background_input.rs` — `decide_background_input(target, facts, action)` returns `Execute{verification}` or `Refuse`. `BackgroundAction` = `AxSemantic | WindowPointer | InsertText | GenericKey`. `refusal_codes`: `WINDOW_NOT_FOUND`, `OWNER_PID_MISMATCH`, `OFF_SPACE_OR_AX_UNRESOLVED`, `MINIMIZED_OR_HIDDEN`, `SAME_PID_KEYBOARD_AMBIGUITY`, `ELEMENT_OUTSIDE_TARGET_WINDOW`. The `!facts.ax_window_present` branch is the off-Space limit, refusing with `OFF_SPACE_OR_AX_UNRESOLVED` and `advice: Some("foreground")` while leaving observation available.
- `rust/crates/platform-macos/src/focus_steal.rs` — `FocusStealPreventer::shared()`, `begin_suppression`, `begin_suppression_allowing` (wildcard entry that permits one intentional target activation), 5s per-entry deadline, 1s janitor. Observer on `NSWorkspaceDidActivateApplicationNotification` calls `restore_focus` via `activateWithOptions`. Header: "Rust port of Swift's `SystemFocusStealPreventer.swift` plus the PR #1521 4-layer hardening."
- `rust/crates/platform-macos/src/focus_guard.rs` — `with_focus_suppressed(target_pid, prior_frontmost, origin, f)`, the `isTargetFrontmost` skip and the 50ms post-action settle. Explicitly ships only layer 3 of Swift's `FocusGuard.withFocusSuppressed`; layers 1 and 2, AX enablement and the synthetic `AXFocused`/`AXMain` write, are called out as "a known, intentional limitation." This is the `lastViewBridgeFocusStealWasSuppressed` gap.
- `rust/crates/platform-macos/src/input/skylight.rs` — `activate_without_raise(pid, wid)` (yabai focus-without-raise recipe via `SLPSPostEventRecordTo` defocus/focus records, deliberately skips `SLPSSetFrontProcessWithOptions`); `with_foreground_assist`, `with_foreground_hid_activation`, `with_menu_shortcut_activation`, `make_exact_window_key`. `SpaceQuery` reads Space membership via `SLSCopySpacesForWindows`, `SLSManagedDisplayGetCurrentSpace`, `SLSGetActiveSpace`. These are read-only; there is no Space-move SPI here.
- `rust/crates/platform-macos/src/ax/window_scope.rs` — `WindowScope` = `Matched | NotFound | OwnerPidMismatch | AxUnresolved`, `decide_window_scope`. Structural invariant: the walk set is non-empty only for `Matched`, so an unresolved id can never return a sibling or menu-bar tree.
- `rust/crates/platform-macos/src/background_mutation.rs` — per-PID `acquire(pid)` async mutex and task-local `with_held_lease`, so two concurrent same-PID mutations cannot interleave their fact-gathering, focus change, dispatch, and restore.
- `rust/crates/platform-macos/src/tools/click.rs` (lines 409-419) — modified clicks force foreground: modifiers with a non-foreground mode or missing `window_id` refuse, "background PID-routed events cannot preserve live modifier-key state." The foreground path runs the click inside `with_foreground_hid_activation` so macOS observes real physical modifier transitions.
- `rust/crates/platform-macos/src/tools/mod.rs` — `DeliveryMode` has exactly two variants, `Background` (default) and `Foreground`. There is no gather rung.

Background vs foreground rungs: `Background` posts the AX action or window-local `CGEvent` without fronting, using `activate_without_raise` and the wildcard focus-steal lease to keep the user's app on top. `Foreground` is the last-resort rung that briefly fronts via `with_foreground_assist`, waits for `AXFocusedWindow` to actually land, acts, then restores the prior process. Modified clicks and HID hotkeys must take the foreground rung because a PID-routed synthetic event carries flag bits but not live physical modifier state.

### 4. Delta

| Aspect | Status |
| --- | --- |
| Reactive focus-steal preventer | Match. cua's `focus_steal.rs` is a literal port of Codex's `SystemFocusStealPreventer`, hardened with a deadline and janitor. |
| Focus-without-raise CPS recipe | Match. Same `SLPSPostEventRecordTo` focus/defocus records and CPS key-focus semantics. |
| AX enablement attributes | Match. Both use `AXManualAccessibility` with `AXEnhancedUserInterface` fallback. |
| Explicit route ladder + stable refusal codes | cua ahead. Codex exposes none of this; cua has a pure, table-tested `decide_background_input` with six machine-readable refusal codes. |
| Per-PID mutation serialization + singleton keyboard gate | cua ahead. `background_mutation.rs` plus `SAME_PID_KEYBOARD_AMBIGUITY` have no visible Codex equivalent. |
| Read-only capability report | cua ahead. `background_input_capability_report` surfaces per-route availability. |
| Live frontmost + Space + deminiaturize model | Codex ahead. `SystemFrontmostApplicationTracker`, `activeSpaceDidChange:`, `windowDidDeminiaturize:` observers; cua queries frontmost and Space ad hoc. |
| ViewBridge / synthetic-focus suppression (layers 1-2) | Codex ahead. `lastViewBridgeFocusStealWasSuppressed`; cua's `focus_guard.rs` ships only layer 3. |
| Off-Space acting | Codex ahead in reach, cua ahead in safety. Codex auto-activates and lets the window follow to the current Space; cua refuses with `OFF_SPACE_OR_AX_UNRESOLVED`. |

### 5. Recreate/port plan

The default should stay refuse, matching cua's safety posture. Recreating Codex's reach means adding an explicit opt-in gather rather than adopting auto-activation.

1. Add an opt-in off-Space gather rung. native-patch. Add `DeliveryMode::Gather` in `rust/crates/platform-macos/src/tools/mod.rs` and a `skylight::with_space_gather(pid, wid, body)` in `input/skylight.rs` that: records the current Space via the existing `SpaceQuery`, brings the exact window to the active Space, polls `gather_background_facts` until `ax_window_present` is true, runs `body` through the existing `with_foreground_assist`, then restores the original Space. Hook point: in the tool shells, when `decide_background_input` returns `OFF_SPACE_OR_AX_UNRESOLVED` and the caller passed `delivery_mode:"gather"`, take this path instead of returning `background_refusal_result`. The gather must hold the `background_mutation::acquire` lease across the whole record-move-act-restore transaction.
2. Add the missing Space-move SPI. native-patch bordering large-build. `input/skylight.rs` currently resolves only read-side Space symbols. Gather needs a write symbol, `SLSMoveWindowsToManagedSpace` or `CGSAddWindowsToSpaces`, resolved through the same `find_sym`/`OnceLock` pattern, failing closed to a refusal when absent. This is the deferred work called out in `docs/macos-background-input-v1-plan.md` section 7, which explicitly keeps V1 free of any SkyLight Spaces manipulation, so it needs its own restore-on-every-path cleanup contract.
3. Port focus-guard layers 1 and 2 for ViewBridge parity. native-patch. Extend `focus_guard.rs` to set `AXManualAccessibility`/`AXEnhancedUserInterface` and write then restore `AXFocused`/`AXMain` around the dispatch, closing the gap its own header documents and matching Codex's `lastViewBridgeFocusStealWasSuppressed` handling.
4. Optionally add a live frontmost/Space tracker. thin-port. A long-lived observer mirroring `SystemFrontmostApplicationTracker` and `activeSpaceDidChange:` would replace the ad-hoc `apps::frontmost_pid()` and `SpaceQuery` reads, giving the gather a cheaper and race-free baseline.
5. Copy-verbatim: none. The corpus API surface deliberately hides the ladder, so there is no Codex doc or code to copy; the behavior must be reconstructed from the binary's symbols and cua's existing ported primitives.
