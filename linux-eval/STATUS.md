# Linux system status: driven + tested, and what is complete

Driven end-to-end against the real cua-driver (built for aarch64) with GTK fixtures, verified by the fixture's own event log plus OS oracles. Harness validated: a fail-on-purpose negative control (click empty space) correctly reports zero delivery, so passes are not strawmanned.

## X11 (Xvfb + Openbox + picom) — COMPLETE
All 9 core actions deliver:
- Background + invisible (no focus change, no cursor move, no input leak, target not activated): click, set_value, type_text (via AT-SPI).
- Foreground (activate, act, restore): double_click, right_click, scroll, drag, press_key, hotkey.
Occluded capture faithful (with compositor). Refusal case returns a typed structured refusal.

## Wayland (headless Sway 1.9, foreign-toplevel + AT-SPI + wlroots) — COMPLETE (9/9)
Requires `CUA_DRIVER_RS_ENABLE_WAYLAND=1` (Wayland is opt-in).
- All 9 deliver (committed oracle: `oracles/parity_wl_gtk.json`): click, set_value (background + invisible via AT-SPI); double_click, right_click, scroll, drag, type_text, press_key, hotkey (foreground). PINNED: on Wayland `type_text` is FOREGROUND (`background_unavailable` in bg — it takes the `wtype`/virtual-keyboard path), unlike X11 where it is background via AT-SPI insert. Every non-X11 lane now has a committed oracle in `oracles/` (Wayland GTK, Qt/X11, Qt/Wayland, negative control, nested).
- RESOLVED: the earlier type_text/press_key gap was a missing runtime dependency. cua's Wayland keyboard shells out to the `wtype` binary (wlroots `zwp_virtual_keyboard_v1`); with `wtype` absent it fell back to libei (compiled out) and delivered nothing. Installing `wtype` closed it. So `wtype` is a required runtime dep for Wayland keyboard; `grim`/portal are needed for some capture/libei paths.
- Enumeration and occluded capture work.

## Honest completeness verdict
- e2e driven + tested: yes, on both X11 and Wayland, with a validated (non-strawman) harness.
- Complete: X11 yes (9/9). Wayland yes (9/9, once `wtype` is installed).
- Not yet covered: Qt/Electron/Chromium fixtures (GTK only so far); repeated-trial and focus-restore rigor on the foreground path; a real (non-headless) Wayland desktop.
- Platform-gated (shared with macOS): background raw-COORDINATE input (the Windows-only capability via per-HWND synthetic pointer) needs the nested cua-compositor tier on Linux.

## Remaining work to call Linux fully complete
1. Add Qt + Electron + Chromium fixtures to the matrix (GTK proven on both display servers; other toolkits untested).
2. Add foreground focus-restore + repeated-trial oracles.
3. Package the runtime deps (wtype, grim, picom/compositor, at-spi) as a documented bundle.
4. Optional: nested cua-compositor lane for background raw-coordinate parity with Windows (macOS lacks this too).
