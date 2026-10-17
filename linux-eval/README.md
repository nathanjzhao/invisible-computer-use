# Linux invisible-CU measurement + iteration harness

Measures cua-driver's invisible-computer-use behavior on Linux across every axis, against
expected behaviors, with visual audits, and a loop to iterate continuously.

## What it measures (axes, verified by ground truth not the driver's OK)
- delivery: the target fixture's own event log records the click/keystroke
- focus preservation: X11 `_NET_ACTIVE_WINDOW` (xdotool getactivewindow) before vs after
- cursor non-movement: xdotool getmouselocation before vs after
- no input leak: occluder + bystander fixtures record zero events
- target not activated: fixture never fires activeChanged
- occluded capture fidelity: cua screenshots the covered target; non-black fraction + visual audit
- refusal correctness: a bad target returns the declared structured refusal

## Run it
On the cloud box: `bash ~/re/run_measure.sh` (sets up Xvfb + Openbox + dbus + at-spi + picom,
launches target/occluder/bystander GTK fixtures, drives cua-driver, writes report.json + audit PNGs).

## Files
- `harness/measure.py` — the harness + oracles
- `harness/fixture.py` — the GTK receiver fixture (logs every event; exposes AT-SPI)
- `harness/run_measure.sh` — environment setup + run
- `report.json` — latest per-axis results
- `audit/*.png` — visual audits (scrot per action + cua's occluded-window capture, before/after)
- `scoreboard.md` — human-readable results + the iterate-loop story

## Continuous iteration
Each run's oracles are adversarially reviewed for false passes (the occluded-black case was one).
Next lanes: stock Sway/GNOME (expect target-addressable + refusals) and the nested cua-compositor
(expect true focus-free per-surface input). Add each as a new environment with its expected-behavior row.
