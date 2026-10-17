# Committed Linux oracle files

Regenerated 2026-09-09 on the aarch64 cloud box (cua-driver debug build) so the per-lane claims in STATUS.md / COMPLETE.md rest on committed artifacts, not prose. Each parity_*.json has per-action background/foreground results under the side-effect axes (delivered, focus_preserved, cursor_unmoved, no_input_leak, not_activated); `invisible_bg=true` means the background attempt passed every axis.

- `parity_wl_gtk.json` — GTK on headless Sway (Wayland). Background/invisible: click, set_value. Foreground: double_click, right_click, scroll, drag, **type_text**, press_key, hotkey. (type_text is FOREGROUND on Wayland — the wtype/virtual-keyboard path — unlike X11 where it is background via AT-SPI.)
- `parity_qt_x11.json` — PyQt on Xvfb/X11. Qt accepts synthetic XSendEvent, so click, set_value, type_text, press_key, hotkey, right_click are background/invisible; scroll/drag foreground; double_click delivers in bg but fails an axis. 9/9 deliver.
- `parity_qt_wl.json` — PyQt on Wayland-native (QT_QPA_PLATFORM=wayland). Coordinate-only: double_click/right_click/scroll/drag deliver; the AT-SPI-dependent click/set_value/type_text/press_key/hotkey do NOT (Qt does not expose the AT-SPI tree under Wayland-native; XWayland is the workaround). This is an upstream Qt limitation, not cua.
- `negative_control.json` — click at empty coordinates: driver refuses (`desktop_coordinate_scope_required`), fixture logs 0 delivery events. The harness does not false-positive.
- `nested_result.log` — nested cua-compositor lane (background raw-coordinate). EXPERIMENTAL and FLAKY: this session's two runs both delivered 0 (a prior session got one focus-free background double_click). tinywl's minimal focus/enter bookkeeping is the cause; production needs a hardened compositor. Do not treat the nested lane as reliably reproducible.

The X11 GTK lane's committed oracle is the top-level `../parity.json` / `../report.json`.
