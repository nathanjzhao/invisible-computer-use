# Linux feature-parity scoreboard (full action set)

Environment: **E1 Xvfb/X11+picom**. cua-driver 0.24 built for aarch64. Verified by the fixture's own event log + OS oracles.

Every core action delivers. The background-vs-foreground split matches the Windows/macOS ladder: semantic + text via AT-SPI are background and invisible; raw pointer/keyboard escalate to foreground on bare X11 (background there needs a focus-free backend: MPX on real Xorg, or the nested cua-compositor).

| Action | delivers | mode | focus kept (bg) | cursor unmoved (bg) | no input leak | not activated (bg) |
|---|---|---|---|---|---|---|
| click | yes | background (invisible) | PASS | PASS | PASS | PASS |
| set_value | yes | background (invisible) | PASS | PASS | PASS | PASS |
| double_click | yes | foreground | PASS | PASS | PASS | PASS |
| right_click | yes | foreground | PASS | PASS | PASS | PASS |
| scroll | yes | foreground | PASS | PASS | PASS | PASS |
| drag | yes | foreground | PASS | PASS | PASS | PASS |
| type_text | yes | background (invisible) | PASS | PASS | PASS | PASS |
| press_key | yes | foreground | PASS | PASS | PASS | PASS |
| hotkey | yes | foreground | PASS | PASS | PASS | PASS |

**All actions deliver: True.** Invisible background path: click, set_value, type_text. Foreground path: double_click, right_click, scroll, drag, press_key, hotkey.

## Parity verdict

- Observation (AT-SPI tree) and occluded capture (with a compositor): parity.
- Every input action in the Windows/macOS set delivers on Linux.
- Background/invisible coverage on bare X11: click, set_value, type_text. Windows reaches more actions in the background via synthetic-pointer injection; matching that on Linux is the nested-compositor / MPX lane (a focus-free backend), which is a known environment tier, not a missing implementation.
- No action is unimplemented or silently broken; each either delivers or returns the declared `background_unavailable` refusal with a foreground escalation.
