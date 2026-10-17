# Linux invisible-CU measurement scoreboard

Environment: **E1 Xvfb/Openbox X11** (cua-driver 0.24 built for aarch64; compositor: picom/XComposite).

Verified by the target fixture's own event log and OS-level oracles, not the driver's own OK.

| Action | delivery | focus preserved | cursor unmoved | no input leak | not activated |
|---|---|---|---|---|---|
| click_button | PASS | PASS | PASS | PASS | PASS |
| set_value_field | PASS | PASS | PASS | PASS | PASS |

## Cross-axis checks

- **Occluded capture fidelity**: PASS (non-black fraction 0.999). cua screenshots the covered Target and its real content (button + typed 'hello linux') comes back.
- **Refusal correctness**: PASS (bad target returns a structured refusal: `stale_element_token`).

## The iterate loop, live

1. First pass scored occluded capture as 'got a screenshot' = PASS. The visual audit showed the image was **black**: a false pass.
2. Hardened the oracle to measure fidelity (non-black fraction), which then correctly scored **FAIL** on bare X11.
3. Root cause: X11 needs a compositor (XComposite redirection) for occluded-window pixels, unlike macOS ScreenCaptureKit.
4. Added picom to the environment and re-measured: fidelity 0.999, **PASS**. See `audit/` before/after.

## Expected-behavior notes (per environment)

- E1 Xvfb/Openbox X11 **with a compositor**: all input axes deliver via AT-SPI; occluded capture faithful. This board.
- E1 **without** a compositor: occluded capture degrades to black (expected). Input axes unaffected.
- Stock Wayland (Sway/GNOME): input is target-addressable only; arbitrary focus-free raw input refuses (expected). Next lane to add.
- Nested cua-compositor: true focus-free per-surface input (expected complete). Next lane to add.
