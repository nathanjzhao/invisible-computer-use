# Windows parity scoreboard, run 2 (with a real focus oracle)

Run 1 (`win-eval/`) proved delivery but had no foreground/z-order oracle and the target was actually frontmost, so it could not establish "invisible." An adversarial review caught that. This run adds the oracle run 1 lacked: `GetForegroundWindow` recorded before and after every action (P/Invoke), plus the fixture logs its own `focused` state on every event. EC2 Windows Server 2022, cua-driver 0.24.1-nightly, on-demand scheduled task in interactive console session 1.

Two scenarios were measured, both with the target NOT the foreground window throughout (foreground was a different window in every action, `target_was_fg=false` and `target_fg_after=false`):

## Scenario A: minimized target (`win-eval2/eval4-oracles.json`, `fx3.log`, `fx4.log`)

Target minimized (`IsIconic=true`, `is_on_screen:false`), foreground held by window 196872 (a different process). Snapshot still built a 7-element UIA tree.

| action | driver | oracle | verdict |
|---|---|---|---|
| set_value | route `accessibility` (UIA ValuePattern), fg stayed 196872 | fixture logged `textChanged`="unfocused hello" while `minimized:true` | DELIVERED to a minimized, non-foreground window |
| click | REFUSED, `code: window_minimized` ("element resolves to the Win32 iconic sentinel (-31850,-31964)… call bring_to_front then re-snapshot") | no `buttonPressed` | cua REFUSES honestly rather than posting off-screen |
| type_text | route `synthetic_events` (SendInput), `escalation {delivery_failed, foreground}`, fg never became target | no event | NOT background-capable |

The click refusal is a coordinate guard (a minimized window has no on-screen point), not a UIA limitation, which is why scenario B was run.

## Scenario B: visible, non-foreground target (`win-eval2/eval6-oracles.json`, `fx6.log`)

Target minimized to drop foreground, then `ShowWindow(SW_SHOWNOACTIVATE)` to restore it visible WITHOUT activating: `is_iconic=false`, `is_visible=true`, `is_on_screen:true`, foreground held by a different window (327936) the whole time. The fixture logged `deactivated` (it lost focus), then:

| action | driver | oracle (fixture's own log) | stole foreground? | verdict |
|---|---|---|---|---|
| click (button) | route `accessibility` (UIA InvokePattern), delivery `background` | `buttonPressed`, `focused:false` | no (`target_fg_after=false`) | INVISIBLE: fired while unfocused, no focus theft |
| set_value (field) | route `accessibility` (UIA ValuePattern), delivery `unknown` | `textChanged`="visible unfocused", `focused:false` | no (`target_fg_after=false`) | INVISIBLE: landed while unfocused, no focus theft |
| type_text | route `synthetic_events` (SendInput), `escalation {delivery_failed, foreground}` | no ` typed` event | no (couldn't foreground) | NOT background-capable, escalates |

## What this earns

Three independent signals agree, so the "invisible" claim is no longer inferred from delivery alone:
1. the driver's own route/delivery/escalation fields,
2. `GetForegroundWindow` before/after showing a different window owns the foreground throughout, and
3. the fixture's own `focused:false` on the `buttonPressed`/`textChanged` events.

On a visible, unfocused Windows window, UIA `click` (InvokePattern) and `set_value` (ValuePattern) deliver focus-free without raising the window; `type_text` (SendInput) cannot and escalates to foreground. This matches the macOS and Linux focus-oracle results, and it matches the Codex Windows teardown: SendInput is the active-desktop path, UIA is the background rung cua adds over Sky's activate-then-SendInput model.

The one honest asymmetry with run 1's overclaim: cua's `click` refuses on a MINIMIZED (off-screen) window rather than delivering, so "invisible click" holds for a visible-unfocused window, not a fully hidden/minimized one. set_value has no such coordinate dependency and lands even when minimized.
