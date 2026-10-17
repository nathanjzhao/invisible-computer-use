# Keyboard-grab incident: root cause (2026-09-12)

The user's physical keyboard stopped typing across apps. WindowServer had routed
keyboard delivery to Calculator (pid 46314, connection 1A3393) as `keyThief`,
even while ChatGPT (pid 75533) was frontmost. Quitting Calculator at 17:16:05 PT
produced `releaseKeyFocus` then `Setting keyThief connectionID: 0` and typing
recovered. The retained Calculator key grab was the immediate cause.

## Findings (read from source, not inferred)

The background pixel-click path acquires the WindowServer keyboard key
connection for the target and never releases it symmetrically.

1. A background (`delivery_mode` != foreground) **left** click **with a
   window_id** takes `PixelActivationPolicy::AllowTargetWithoutRaise`
   (`tools/click.rs` `pixel_activation_policy`).
2. That policy calls `input::mouse::prepare_background_pixel_click(pid, wid)`
   (`tools/click.rs` ~1019), which calls
   `input::skylight::activate_without_raise(pid, wid)` (`input/mouse.rs:358`).
3. `activate_without_raise` (`input/skylight.rs:509`) captures the current front
   process PSN (`_SLPSGetFrontProcess`), then posts two raw WindowServer focus
   records via `SLPSPostEventRecordTo`:
   - step 3: `buf[0x8A]=0x02` (defocus) to the **previous** front PSN;
   - step 4: `buf[0x8A]=0x01` (focus) to the **target** window's PSN.
   This transfers the WindowServer keyboard key connection to the target. There
   is no paired inverse (defocus target / refocus previous) anywhere.
4. The only post-click restore is `background_pixel_restore_pid` +
   `apps::activate_pid` (`tools/click.rs` ~1160). It is:
   - an **AppKit-level** `activateWithOptions`, a different layer than the raw
     WindowServer focus record grabbed in step 3; and
   - **conditional**: it runs only when
     `prior_front != Some(target_pid) && observed_front == Some(target_pid)`, so
     it is **skipped entirely when the target was already frontmost** at click
     time.

So a background left click leaves the target holding the WindowServer key
connection, and the restore neither reverts that raw grab nor runs at all when
the target was already front.

## Hypothesis (matches the incident; consistent with the findings above)

The FIX 1 cursor-latency benchmark issued dozens of background left clicks with
window_id on Calculator (pid 46314, window 1725) via the driver
(`delivery_mode:"background"`, default left button). Provenance: the FIX-1
benchmark scripts (`/tmp/lat.py`, `/tmp/lat2.py`, `/tmp/lat3.py`) and the
`fix1-cursor-detach-latency.txt` evidence; the earlier 7+6 Calculator demo used
the same target pid. Calculator was the driver's own front during the drive, so
`prior_front == Some(46314)` and the AppKit restore was skipped; every click
re-posted the "focus target" record and none reverted it, so Calculator retained
the key connection after the driver exited. NSWorkspace frontmost moved to the
user's app, but WindowServer keyboard delivery stayed on Calculator (keyThief),
hijacking physical typing until Calculator quit and WindowServer released it.

This is a real defect in the vendored driver's background-click path; the
automation triggered it at scale. It is NOT specific to `icu-fix5.txt` or
TextEdit — the leak is the Calculator background-click focus grab.

## Not yet confirmed / still open
- Whether any single click (vs. the cumulative effect) is sufficient to strand
  the grab. Either way the fix (symmetric release) is the same.
- Exact WindowServer debug lines are debug-level and were not retained in the
  default unified log on this host; the release evidence came from the user.

## Correct focus model (spec 05 + Codex reference) and how the fix aligns

The CGS/CPS/SLPS focus SPIs are private and undocumented by Apple; the governing
principle, from Codex's own design (spec 05, read from the `SkyComputerUseService`
symbols), is: **every focus/front/key change is reverted at the same layer**,
backed by a reactive `SystemFocusStealPreventer` that restores the prior
frontmost on `NSWorkspaceDidActivateApplicationNotification`, plus observation of
the CPS key-focus events (`kCPSNotifyKeyFocusChanged` / `KeyFocusTaken` /
`LostKeyFocus`). Codex's default is activate-then-act with that reactive restore.

Why the reactive net missed this: `activate_without_raise` uses the yabai
focus-WITHOUT-raise recipe (`SLPSPostEventRecordTo` focus records), which
deliberately does NOT post an `NSWorkspace` activation. So cua's ported
`SystemFocusStealPreventer` (which only fires on that notification) never saw the
grab, and the recipe had no same-layer revert. That is the hole.

Cross-check of the sibling paths (confirms the diagnosis, source-read):
- The FOREGROUND input paths (`type_text`, `scroll`, `hotkey`, `press_key`,
  modified/HID clicks) go through `with_foreground_assist` /
  `with_foreground_hid_activation`, which restore the prior front at the CPS
  layer via `set_front(prev_psn, 0, kCPSNoWindows)` on success, error, and the
  activation-timeout path. They are NOT the culprit. (Minor: their restore is not
  guarded against an outright panic in the body, which returns `Result` and does
  not panic in practice; noted as a follow-up to make RAII, to validate in the
  bounded session.)
- The BACKGROUND left-click path was the only one using the focus-WITHOUT-raise
  CPS grab, and the only one whose restore was wrong-layer + conditional.

The fix therefore brings the background path in line with the foreground paths'
already-correct CPS-layer restore, and adds the deterministic per-call symmetry
the reactive net cannot provide for a no-activation grab. The guard now does
BOTH: (a) posts the exact inverse focus records, and (b) re-asserts the prior
front process with `set_front(prev_psn, 0, kCPSNoWindows)` — the same primitive
the foreground paths use — so a stranded key connection cannot survive even if a
single focus record fails to land.

Only one site uses the focus-without-raise grab (the background left click); the
fix covers it. `make_exact_window_key` / `set_front_process_persistently` are
used by user-initiated activation (`bring_to_front` / `activate_window`) where
leaving the target front is the intended result, not a leak.

## Cross-OS audit (Windows + Linux) — no equivalent bug

A fresh-context source survey of the Windows and Linux input paths found NO
instance of the bug class (a background/focus-free rung that redirects global
keyboard routing or foreground without a symmetric restore):
- Windows: background rungs use PostMessage per-window queues and UIA over the
  a11y channel, which never change global focus; synthetic-pointer inject uses a
  RAII `NoActivateGuard`. Foreground rungs (`with_confirmed_foreground`, SendInput
  z-order paths) save and restore prior foreground/z-order/cursor on success,
  error, and timeout.
- Linux: background rungs use XSendEvent per-window, AT-SPI semantic actions, and
  MPX dedicated-pointer + XI2 shield grabs, none of which change global focus;
  the MPX paths still defensively save/restore. Foreground rungs
  (`with_x11_foreground`, sway/shell/kwin helpers) save and restore prior
  `_NET_ACTIVE_WINDOW` + X input focus on all paths.
- Two caveats, neither this bug: (1) those guaranteed restores are manual
  finally-blocks, not RAII, so an outright `body()` panic would skip them (the
  macOS fix used RAII Drop specifically to be panic/cancellation-proof); (2)
  Windows WPF self-activation during UIA and Linux AT-SPI `GrabFocus` can leave
  the TARGET app activated with best-effort/no restore, but these are documented
  tradeoffs affecting only the target app, not the user's global keyboard owner.

## LIVE PROOF (2026-09-12, user-authorized bounded run) — PASS
`ICU_BOUNDED_TEST=1 cargo test -p platform-macos background_focus_grab_is_released_at_the_windowserver_layer -- --ignored`:
1 passed. It read the WindowServer front/key owner (`current_front_psn`, the
keyThief layer), grabbed key focus on a visible window owned by a non-front
process via `activate_without_raise`, dropped the `KeyFocusRestore` guard, and
asserted the WindowServer owner returned to its pre-grab value. A background
focus-free click no longer strands the keyboard. The user's physical keyboard was
confirmed working after the run.
