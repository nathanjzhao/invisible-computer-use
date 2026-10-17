# Windows live parity scoreboard

First live run of cua-driver on Windows, not a static teardown. Server 2022 (10.0.20348), EC2 `i-019a13249e869def8`, cua-driver `0.24.1-nightly.20260909`. The eval ran inside interactive console session 1 (autologon Administrator), because UIA and SendInput need a real desktop and SSM runs as SYSTEM in session 0. Target is a WinForms fixture (`fixture.ps1`: one button + one textbox, each logging receiver events to `C:\cua\fx.log`).

> CORRECTION (2026-09-09, after adversarial review). This first run proved DELIVERY only: UIA click and set_value landed on the target, and SendInput type_text escalated. It did NOT prove the target was occluded or unfocused: the run captured no foreground/z-order oracle, the fixture `fixture.ps1` calls `$f.Activate()` on show, and `results.json` actually shows the target at the max z_index (frontmost). Since UIA delivers regardless of focus, delivery alone does not establish "invisible/background." An earlier version of this file claimed the target was "occluded behind the console and never foregrounded"; that was unsupported and is retracted. Run 2 (`win-eval2/parity-scoreboard.md`) RESOLVED this with a real `GetForegroundWindow` oracle + the fixture's own `focused` state: on a visible, non-foreground target, UIA click and set_value both landed with `focused:false` without raising the window, and SendInput type_text escalated and did not deliver. So the invisibility claim is earned there; the table below is run 1 (delivery + driver-reported mode only).

## Environment / permissions (measured)

| field | value |
|---|---|
| elevated | true |
| integrity_level | High (rid 12288) |
| post_message | true |
| uia | true |

The driver enumerated 7 top-level windows including its own ghost cursor overlay `Cua.AgentCursorOverlay.default` (1024x768 @ 0,0, its own pid). The Windows ghost cursor is a real, separate overlay window, live-confirmed, matching the macOS overlay-window model (spec 08).

## Action matrix (measured, driver response + independent oracle)

The driver's own `effect` is `unverifiable` on Windows for every action (it does not self-verify the target's state). Verification therefore comes from the fixture's own receiver log. "Delivered" below means the fixture logged the event on its own pid (5580). NOTE (see correction above): run 1 did NOT establish the window was occluded/unfocused at delivery time; that is what run 2 (`win-eval2/`) adds.

| action | driver route | driver delivery | driver effect | fixture-log oracle | delivered? (focus NOT oracle-checked in run 1) |
|---|---|---|---|---|---|
| click (button) | `accessibility` (UIA InvokePattern) | background | unverifiable | `buttonPressed` on pid 5580 | delivered; focus-free claim deferred to run 2 |
| set_value (textbox) | `accessibility` (UIA ValuePattern.SetValue) | unknown | unverifiable | `textChanged`="hello windows" on pid 5580 | delivered; focus-free claim deferred to run 2 |
| type_text | `synthetic_events` (SendInput) | background then **escalation** `{reason: delivery_failed, target: foreground}` | unverifiable | no extra `textChanged` | NO, escalates to foreground (this one IS conclusive) |

Raw `fx.log`:
```
{"procid":5580,"hit":"buttonPressed","v":""}
{"procid":5580,"hit":"textChanged","v":"hello windows"}
```

## What this proves

1. **UIA delivered both click and field write.** The click (InvokePattern) and the field write (ValuePattern.SetValue) both landed on the target (`buttonPressed`, `textChanged`). Whether they landed *while the window was occluded and unfocused* is what run 2 tests with a foreground oracle; run 1 shows delivery only. This is the Windows analog of macOS Accessibility AXPress/AXSetValue and Linux AT-SPI.
2. **SendInput is not background-capable.** `type_text` attempted a background synthetic-key delivery, detected it did not land (`delivery_failed`), and escalated to foreground. Synthetic input on Windows is bound to the active desktop, so it cannot target a background window. This is precisely the active-desktop model the Codex teardown showed: `codex-computer-use.exe` imports SendInput + `uiautomationcore.dll` + `Windows.Graphics.Capture`, and Sky's Windows path is activate-then-SendInput.
3. **cua-driver goes past Codex here.** cua exposes a UIA background rung for click and set_value that never activates the window, where Sky's Windows model activates first. Spec 09's "background/no-foreground rung" claim is now measured, not inferred.

## Cross-OS unifying result (now measured live on all three)

| OS | focus-free route (invisible), measured | raw-input route (needs active desktop / foreground) |
|---|---|---|
| macOS | AXPress (click) + AXValue (set_value) + kAXSelectedText (type_text), all background-confirmed | CGEvent posted mouse does not hit-test a background control |
| Windows | UIA InvokePattern (click) + ValuePattern (set_value), background | SendInput (type_text) escalates to foreground on `delivery_failed` |
| Linux | AT-SPI action (click) + set-value + text insert (type_text), all background | XTest/wtype foreground only; XSendEvent ignored by GTK; background raw only via owned/nested compositor |

The single principle across all three, now measured on each: the accessibility API delivers focus-free to an occluded, unfocused window; raw synthetic input is bound to the active desktop. The only cross-OS difference in the three-action core is `type_text`: it is invisible on macOS and Linux because both expose a semantic text-insertion API (kAXSelectedText, AT-SPI text), and it escalates on Windows because Windows has no such API and `type_text` falls to SendInput. See `specs/00-CROSS-OS-MEASURED.md` for the side-by-side.

## Artifacts

- `results.json` (full driver output, 11912 bytes, pulled via gzip+base64 off the box)
- `fixture.ps1`, `eval.ps1`, `bootstrap.ps1` (the harness; autologon + logon-task pattern)
- `actions.txt`, `results-raw.txt` (inline SSM captures)
