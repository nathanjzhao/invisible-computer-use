# Cua KWin target helper

This is an **optional KDE Plasma/KWin Wayland integration**. It is not part of
the portable `cua-driver` binary and must be built against the user's installed
KWin/Qt/KF6 development files. Do not use a helper built for a different KWin
or Qt ABI.

The observation methods are **read-only**. They provide trusted KWin window
identity and state metadata: stable opaque tokens, PID, geometry,
active/minimized state, stacking order, and captures of one window's own content. It does not expose window activation
or input mutation methods. Setup can persist this helper's enabled preference
after explicit host approval. KWin portal/libei input is focus-bound, so focus can
change between any focus check and compositor-side input processing. Cua
therefore refuses raw target-addressed KWin keyboard/pointer actions until a
target-bound KWin input path exists.

## Requirements

- KDE Plasma/KWin 6 with a matching KWin development package
- Qt 6 development files
- ECM/KF6CoreAddons development files
- CMake 3.21 or newer
- a running Wayland KWin session for loading/testing

On Arch-derived systems, install the distribution's matching packages (package
names vary by distribution), then build from this directory:

```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
```

The module is installed in KWin's effect plugin namespace:

```text
kwin/effects/plugins/cua_kwin_target_helper.so
```

For a user-local installation, use a prefix supported by the local Qt/KWin
plugin search path and verify discovery before loading:

```bash
cmake --install build --prefix "$HOME/.local"
```

If the local KWin installation does not search that prefix, use the
distribution's native packaging mechanism or an explicitly approved
system-local installation. Never copy the module into an arbitrary plugin
directory and never overwrite an existing file.

Load/reload through KWin's supported Effects interface where available:

```bash
qdbus6 org.kde.KWin /Effects loadEffect cua_kwin_target_helper
```

The helper exposes the Cua-owned service `org.cua.KWinTarget` only while the
effect is loaded by the current `kwin_wayland` process. Verify the service
owner, protocol version, and target snapshots before trusting identity data.
`GetVersion` remains identity protocol **1**, compatible with the existing
History collector. The additive `GetCaptureVersion` returns **2** and
`CaptureWindow(token, pid)` returns PNG bytes with the same token, PID and frame
geometry. It draws only the selected EffectWindow into a cleared offscreen
framebuffer and validates the target before and after rendering. It never reads
output pixels. Input mutation and activation remain outside this API.

History input protocol **1** adds a short-lived, caller-bound observation lease
for the exact active native Wayland window. `BeginHistoryInput(token, pid)`,
`DrainHistoryInput`, `CheckHistoryInput` and `EndHistoryInput` expose bounded
left/right press coordinates and Return/Tab/Escape metadata. The helper never
reads printable key text. XWayland, locked, grabbed, inactive and changed scopes
refuse. Client and decorated-frame geometry are bound separately; presses on
server decorations are withheld. Native event timestamps retain compositor
queue delay. The lease expires after one second without renewal and is removed
when its caller disconnects. This is transient metadata for the existing History
collector, not another recorder.

Setup protocol **1** provides `RememberHistoryApproval()` to persist only this
helper's enabled setting in the running desktop's own configuration. The host
calls it after explicit capture-scope approval. An isolated host config cannot
silently redirect the desktop's preference into its private profile. Setup does
not change any other effect selection or grant content-sharing permission.

Tokens are never reused while KWin remains alive, including an effect reload.
Closed, minimized, changed or missing windows, unavailable OpenGL rendering and
capture timeouts return explicit errors. The driver pins the compositor's unique
D-Bus owner, verifies its identity, negotiates the capture protocol and rechecks
the target and geometry around capture.

The four distribution-specific `copilot-kwin-helper` packages must be rebuilt:
Fedora 43, Fedora 44, Ubuntu 26.04 and Debian 13. Keep their exact KWin runtime
ABI dependency and native Qt plugin directories from the RC packaging workflow.
KWin 6.6+ uses the new Region/viewport API; earlier supported KWin 6 packages
use QRegion. Ubuntu 24.04's KWin 5 remains unsupported.

## Input safety boundary

Portal/libei events are delivered according to compositor focus when KWin
processes them. A target focus check immediately before dispatch cannot bind the
later event to that target, so adding more checks does not remove the race.
Until a future KWin integration can bind the mutation itself to an exact target,
Cua fails closed rather than sending raw focus-based input to a requested KWin
window.

## ABI and rollback

The module is compositor/Qt ABI-specific. Rebuild after Plasma/KWin or Qt
upgrades. To roll back a user-local test installation, unload the effect and
remove only the exact module installed by that test; do not remove KWin or
change the desktop session. The portable Cua Driver continues to build without
this optional helper and without KWin development packages.
