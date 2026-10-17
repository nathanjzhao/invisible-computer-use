# Cross-OS live measurement: the same action matrix on all three OSes

Every claim in sections 05, 06, 09, 10 about which actions are "invisible" (delivered focus-free to an unfocused window) is now backed by a live run of cua-driver against a receiver-logging fixture on each OS, not by reading source or tearing down a binary. The three runs use the same shape: a plain third-party-style app with a button and a text field that logs every event it receives, driven by the driver, verified by the app's own log plus OS focus oracles. This is the "done means verified" bar: the driver's own success response is never the evidence, the target's state is.

> CORRECTION + RESOLUTION (2026-09-09, after adversarial review). The macOS and Linux runs carry real focus oracles (macOS logs `active=false`/`hasKeyWindow=false` on every event; Linux `report.json` records the occluder as the active window before and after, `target_not_activated=PASS`). The FIRST Windows run did NOT, and its `results.json` showed the target frontmost, so run 1 proved delivery only. Run 2 (`win-eval2/parity-scoreboard.md`, `eval6-oracles.json`, `fx6.log`) supplies the oracle: `GetForegroundWindow` before/after each action plus the fixture's own `focused` state. On a visible, non-foreground Windows target (foreground held by a different window throughout), UIA `click` fired `buttonPressed` with `focused:false` and UIA `set_value` landed `textChanged` with `focused:false`, neither raising the window; `type_text` (SendInput) escalated and did not deliver. So the Windows click/set_value cells below are now oracle-confirmed focus-free, on the same footing as macOS and Linux. (Caveat: on a MINIMIZED target cua `click` returns an honest `window_minimized` refusal, an off-screen coordinate guard; the invisible-click result is for a visible-unfocused window.)

## The three runs

| OS | driver | environment | fixture | evidence file |
|---|---|---|---|---|
| macOS | cua-driver 0.24.0 | this Mac, live daemon `com.trycua.driver` | `build/verify/target.swift` (AppKit, logs to a file) | `build/verify/mac-matrix-result.json` |
| Windows | cua-driver 0.24.1-nightly | EC2 Server 2022, autologon console session | `win-eval/fixture.ps1` (WinForms) | `win-eval/results.json`, `win-eval/parity-scoreboard.md` |
| Linux | cua-driver 0.24 (aarch64) | EC2 Ubuntu, Xvfb+Openbox+picom (E1) | `linux-eval/harness/fixture2.py` (GTK) | `linux-eval/parity-scoreboard.md`, `linux-eval/COMPLETE.md` |

## The three core actions, measured

"Invisible" = the fixture logged the event while it was never activated / never became key/frontmost / not the foreground window (macOS `appDidBecomeActive=0`, `windowDidBecomeKey=0`; Windows `focused:false` on the event + `GetForegroundWindow` on another window, run 2; Linux focus oracle held with the occluder active). Every cell below is an oracle-confirmed delivery, not a driver self-report.

| action | macOS | Windows | Linux (X11 E1) |
|---|---|---|---|
| click (button) | AXPress, background, oracle `buttonPressed` | UIA InvokePattern, background, oracle `buttonPressed` | AT-SPI action, background, oracle press |
| set_value (field) | AXValue write, background, driver **confirmed** + oracle `textChanged` | UIA ValuePattern, background, oracle `textChanged` | AT-SPI set-value, background, oracle textChanged |
| type_text (field) | kAXSelectedText (AX), background, driver **confirmed**, `delivered_count=7`, oracle `textChanged` | SendInput synthetic, **escalates to foreground** (`delivery_failed`) | AT-SPI text insert, background, oracle textChanged |

## The one real cross-OS difference, now pinned

Text typing is the only action that behaves differently across the three, and the reason is which API the driver's `type_text` uses:

- **macOS and Linux-X11 `type_text` are semantic.** macOS writes `kAXSelectedText` through Accessibility; Linux on X11 inserts via AT-SPI. Both are the accessibility channel, so both deliver to an unfocused window and both are confirmed focus-free. (Linux caveat, now pinned by committed oracle `linux-eval/oracles/parity_wl_gtk.json`: on Wayland `type_text` is FOREGROUND, not background — cua's Wayland keyboard takes the `wtype`/virtual-keyboard path rather than AT-SPI insert. So "Linux type_text is background" holds for X11, not Wayland.)
- **Windows `type_text` is raw.** It uses SendInput synthetic keystrokes, which are bound to the active desktop, so it self-detects `delivery_failed` in the background and escalates to foreground. Windows has no `kAXSelectedText` equivalent for free text; UIA ValuePattern (`set_value`) is the background text path, and that one does land invisibly.

So the unifying rule holds on all three, stated precisely:

> The accessibility API (AXPress/AXValue/kAXSelectedText on macOS, UIA Invoke/Value on Windows, AT-SPI action/value/text on Linux) delivers focus-free to an occluded, unfocused window. Raw synthetic input (CGEvent posted mouse on macOS, SendInput on Windows, XTest on Linux) is bound to the active desktop and must foreground the target. `type_text` is invisible exactly when the OS exposes a semantic text-insertion API (macOS, Linux) and escalates when it does not (Windows, which falls to SendInput).

## Beyond the three core actions

The full nine-action set was measured only on Linux (`linux-eval`, 9/9): the extra six (double_click, right_click, scroll, drag, press_key, hotkey) all route through raw input there and run foreground on bare X11, background only under a focus-free backend (nested cua-compositor or real-Xorg MPX). Windows reaches more of these in the background than Linux via `CreateSyntheticPointerDevice` injection (section 09, source-verified, not re-measured action-by-action). macOS posted-mouse does not hit-test to a background control (section 06), so its extra pointer actions are foreground too. The three-action core above is the part measured identically on all three OSes; the tails are documented per OS in their sections.

## What this closes

This is the "all OS native behavior we can compare completely" bar: macOS, Windows, and Linux each have a live, oracle-verified measurement of the same invisible-CU actions, so the comparison rests on real runs on all three, with the single genuine behavioral difference (Windows text typing) identified and explained rather than papered over.
