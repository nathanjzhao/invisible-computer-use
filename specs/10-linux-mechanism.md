## Linux mechanism: AT-SPI + Wayland + Hyprland/KWin + libei

### What it is

The Linux actuation layer: how each system captures the screen and delivers pointer/keyboard/scroll input across X11 and the Wayland compositor zoo. Sky ships one opaque `sky_linux` binary that presents a single full-desktop coordinate model and nothing else. cua-driver instead carries a large compositor-specific stack: AT-SPI tree walking, wlroots virtual-pointer, GNOME Shell and KWin identity helpers, an optional Hyprland compositor plugin, libei via xdg-desktop-portal, and X11 XSendEvent/XTest/XInput2-MPX. The central Wayland fact both must live with: standard Wayland cannot route raw input to an arbitrary occluded, unfocused window, so per-window background input only exists where cua owns or attests the compositor.

### Codex/Sky

Sky Linux is a single native executable driven from JS over a per-action CLI spawn. There is no window model, no window enumeration, no delivery-mode, and no per-window background targeting: `target: "linux"` is the full-desktop client, and every coordinate is a desktop-screenshot pixel. The native binary's internals are opaque here; only its surface is confirmed.

ACQUISITION NOTE (why this section stays surface-level): the Linux native engine binary is not obtainable from either distribution on hand. The macOS install ships no `bin/linux/`, and the Windows MSIX teardown (`ChatGPT-x64.msix`) carries only `@oai/sky/bin/windows/codex-computer-use.exe` and the `targets/linux/sky_linux.js` marshaller, with no `sky_linux` executable in either `@oai/sky` or the newer `@oai/cua` 0.2.4. CONFIRMED (checked 2026-09-09): native Computer Use does not ship on Linux at all yet, so there is no engine to tear down and this section is complete rather than merely unfinished. OpenAI shipped the ChatGPT/Codex desktop app for Linux in worldwide preview on 2026-08-11 (native .deb and .rpm, x64 and arm64, for Ubuntu 24.04/26.04, Debian 13, Fedora 43/44), but the launch explicitly excludes native Computer Use "the ability for ChatGPT to interact with desktop applications", and with it Appshots, Record and Replay, and desktop/browser voice. So the `sky_linux` full-desktop engine the `targets/linux/sky_linux.js` marshaller would spawn is not present in any obtainable build: the macOS install ships no `bin/linux/`, and the Windows MSIX carries only `@oai/sky/bin/windows/` (no `bin/linux/`) in both `@oai/sky` 0.6.26 and `@oai/cua` 0.2.4. The Codex Linux download URLs are not on the Windows `codex-app-prod` bucket (that bucket is Windows MSIX plus a Microsoft Store license only). Net: for OS parity, macOS and Windows are code-verified against real binaries; Linux is confirmed to have no shipped native engine, so its Codex side is understood as far as it exists, and this section rests on the marshaller surface plus the SKILL and full-desktop API docs that describe the coming model. The cua-driver Linux side below is fully code-verified from source and is far ahead of what Codex ships on Linux today.

- `codex-corpus/mac/sky-package.json` — `bin.sky_linux_arm64` / `bin.sky_linux_x64` are the only Linux artifacts. No Linux entry in `publishConfig.executableFiles` (those are all mac). CONFIRMED the Linux runtime is one standalone binary.
- `sky_js/src/load_options.js` — `process.platform === "linux"` maps to `{target:"linux"}`. One target, chosen by OS.
- `sky_js/src/targets/linux/sky_linux.js` — `package_bin(["bin","linux","sky_linux_"+process.arch])`, then `cli(bin, args, {input: JSON.stringify(input)})`. Each verb spawns the binary with a subcommand and JSON on stdin. The only global flag assembled is `--mouse-size-px`.
- `sky_js/src/targets/linux/create_client.js` — verb set: `click, drag, drag_handle, get_screenshot, move, press_key, scroll, type_text` (+ audio behind `SKY_ENABLE_AUDIO`). This is the entire Linux capability.
- `sky_js/src/types/full-desktop/Options.d.ts` — `LinuxOptions`: `mouse_size_px` (default 12, "resize mouse pointer in screenshots") and `post_action_sleep_ms` (default 100). Sky draws the cursor INTO the screenshot bytes and gates actions with a settle sleep.
- `sky_js/src/targets/linux/action_settler.js` — `ActionSettler.wait()`/`defer()` enforce `post_action_sleep_ms` between input and the next screenshot. CONFIRMED behavior, not just a knob.
- `sky_js/src/service.js` + `sky.js` — `drag_start/drag_move/drag_end` RPC is explicitly gated `if ("linux" !== o.target) throw "Sky drag handles require the Linux runtime"`. Stateful drag handles are a Linux-only affordance.
- `codex-corpus/mac/skills/linux-SKILL.md` and `docs/sky-full-desktop-api.md` — the agent-facing contract: `get_screenshot/click/drag/drag_handle/move/press_key/scroll/type_text`, X keysym chord names, desktop coordinates. No window handle appears anywhere.

### cua-driver

cua-driver detects the display server and compositor, then routes capture and input through the best available backend. It has real per-window capabilities that Sky lacks, and it is honest where Wayland forbids per-window background input, returning a typed `background_unavailable` refusal rather than silently hitting the focused window.

- `rust/crates/platform-linux/src/input/mod.rs` — X11 background input. `send_click`/`send_drag`/`send_type_text_with_delay` use XSendEvent (no focus steal, the PostMessage analogue); `send_*_xtest_desktop` and `with_x11_foreground`/`ewmh_activate_window` are the desktop/foreground paths; `ensure_master_pointer_for_session` + `create_uinput_pointer` build XInput2 MPX master pointers via uinput for real parallel/no-focus-steal pointer injection, with `supports_parallel_pointer_injection` refusing Xvfb/TigerVNC.
- `rust/crates/platform-linux/src/input/mpx_owner.rs` — `Owner` encodes boot-id + PID-namespace + euid + PID + start-ticks + nonce into the `XIAddMaster` device name so a crashed daemon's master pointers can be reaped safely (`reap_orphaned_master_pointers`, `Owner::stale_in`). This is the MPX-recovery contract from `docs/linux-mpx-recovery.md`.
- `rust/crates/platform-linux/src/input/delivery.rs` — `DeliveryMode {Background, Foreground}` mirroring macOS/Windows, plus `BackgroundUnavailable {NoLibeiBackend, ChromiumInput, FocusedInputOnly, WebKitSyntheticInput}` and `background_unavailable_error` telling the agent to retry `delivery_mode:"foreground"`.
- `rust/crates/platform-linux/src/wayland/mod.rs` — wlroots backend: enumerate toplevels via `zwlr_foreign_toplevel_manager_v1`, capture via `zwlr_screencopy_manager_v1`+`wl_shm` (`capture_via_screencopy`, `grim` fallback), inject via `zwlr_virtual_pointer_v1` (`click`, `click_desktop`, `scroll_at`, `drag_vptr`, `move_cursor_absolute`). `PORTAL_INPUT_ENABLED`, `with_libei_fallback`, and `activate_window_for_input` bridge to libei/foreground. Comment states Wayland has no arbitrary per-toplevel background input.
- `rust/crates/platform-linux/src/wayland/libei.rs` — cross-DE input for GNOME/Mutter and KDE/KWin via `reis` + ashpd `RemoteDesktop` (`select_devices`, `connect_to_eis`), a calloop worker, xkb keymap load, absolute coords inside an announced `ei_device.Region`. Consent persisted with `PersistMode::ExplicitlyRevoked` at `~/.config/cua-driver/libei-persistent.token`. libei delivers to compositor focus, not a chosen window.
- `rust/crates/platform-linux/src/wayland/shell_helper.rs` + `wayland-helper/winrects@cua/` — GNOME Shell extension `org.cua.WinRects` (v8): `GetRects` (frame + buffer origin, so `screen = origin + atspi_window_xy`), verified `Activate`, `Capture`, and compositor-owned cursor (`MoveCursor`/`SetCursorState`). Runs privileged, so no portal grant needed for geometry/capture. `video_wayland.rs` `GnomeShellVideoBackend` pipes its PNG frames to FFmpeg.
- `rust/crates/platform-linux/src/wayland/kwin_helper.rs` + `kwin-target-helper/` — read-only KWin effect `org.cua.KWinTarget` exposing token/PID/geometry/active/stacking. `available()` returns `false` and `with_focused_window` always `bail!`s: KWin input is focus-bound, so cua refuses raw target-addressed input until a target-bound KWin path exists.
- `rust/crates/platform-linux/src/wayland/hyprland.rs` + `hyprland_capture.rs` + `hyprland_input.rs` + `hyprland-plugin/` — Hyprland gets the most: read-only identity via `hyprctl` IPC over the compositor socket, true per-window capture via `hyprland-toplevel-export-v1` (captures an occluded/off-workspace window's own buffer), and an optional compositor plugin with a private `SOCK_SEQPACKET` protocol (`cua-input-v3.sock`, `SO_PEERCRED` UID check, two seats `Cua-Agent`/`Cua-Agent-2`) that is the only route to genuine raw per-window background input on Wayland. Default plugin build is discovery-only and refuses input.
- `rust/crates/platform-linux/src/wayland/ext_toplevel.rs` / `ext_screencopy.rs` — vendor-neutral staging protocols: `ext_foreign_toplevel_list_v1` enumeration (opaque stable ids merged with AT-SPI pid/geometry) and `ext-image-copy-capture-v1` output capture for mutter47+/plasma6.2+/niri/labwc.
- `rust/crates/platform-linux/src/atspi/mod.rs` + `native.rs` + `a11y.rs` — native AT-SPI2 over zbus (no Python). `walk_tree_bounded` walks the tree, `perform_action_at_screen_point`/`insert_text`/`type_into_editable` give focus-free semantic actuation that lands where pixel injection cannot (GTK4, Wayland). `a11y::ensure_chromium_accessibility_enabled` flips `org.a11y.Status.ScreenReaderEnabled` so Chromium/Electron build their tree, with per-desktop policy (GNOME/COSMIC `IsEnabled` only, Cinnamon none).
- `rust/crates/platform-linux/src/capture.rs` — per-window X11 capture: persistent MIT-SHM `shm_get_image`, then `XGetImage`, then ImageMagick `import`.
- `rust/crates/platform-linux/src/overlay.rs` — the drawn agent cursor is a separate X11 ARGB override-redirect overlay window composited at ~60Hz, NOT pixels blended into the screenshot the way Sky's `mouse_size_px` does.
- `rust/crates/platform-linux/src/pip/mod.rs` — `LinuxPipBackendFactory::start` returns an error; picture-in-picture is unimplemented on Linux.

### Delta

| Area | Sky/Codex | cua-driver |
| --- | --- | --- |
| Coordinate model | Full-desktop only | Full-desktop AND per-window (xid/token) |
| Window enumeration | None | wlr + ext + AT-SPI + Hyprland + KWin |
| X11 input | Opaque inside binary | XSendEvent + XTest + XInput2 MPX/uinput, with MPX-recovery ownership |
| Wayland input | Whatever the binary does, to focus | wlr virtual-pointer, libei/portal, Hyprland-plugin per-window raw, honest refusals |
| Per-window background input | Not offered | Only on X11 and the Hyprland plugin; KWin/GNOME refuse (focus-bound) |
| Semantic AT-SPI actuation | Not exposed | `perform_action_at_screen_point`, `insert_text`, tree walk |
| Cursor in screenshots | Composited into bytes (`mouse_size_px`) | Separate overlay window / compositor actor |
| Action settling | `post_action_sleep_ms` settler in JS | Per-action delays in Rust; no single desktop settler knob |
| Agent surface | 8 tiny verbs, one binary | Large MCP tool set, many refusal codes |
| Compositor identity/consent | None visible | WinRects/KWin attest identity; libei portal consent persisted |

Where Sky is ahead: simplicity and portability. One binary, one coordinate space, a settle-and-shoot loop, cursor already in the frame. Where cua is far ahead: everything mechanical, plus honesty about Wayland's per-window limits.

### Recreate/port plan

Goal is Codex-like behavior on cua, i.e. the Sky full-desktop surface. cua already has every primitive, so this is a facade, not new Linux input.

1. Thin-port: add desktop-coordinate tool variants that wrap the existing `wayland::click_desktop`/`scroll_at`/`drag`/`move_cursor_absolute` and `input::send_*_xtest_desktop`/`send_type_text_with_delay`, all keyed on desktop pixels with no `window_id`. Map Sky verbs 1:1: `click/drag/drag_handle/move/press_key/scroll/type_text/get_screenshot`. Reuse the display-cascade capture in `capture.rs` + `wayland/ext_screencopy.rs`. native-patch scoped to a new tools module.
2. Thin-port: implement a stateful `drag_handle` (start/move_to/end) over `input::VirtualPointerDrag` and the wlr virtual-pointer drag, matching `sky.js` drag RPC semantics.
3. Native-patch: add a `mouse_size_px`-equivalent option that composites the agent cursor into the returned screenshot bytes, since cua currently renders it in a separate `overlay.rs` window that a raw capture will not include. Add a `post_action_sleep_ms` settle gate around the desktop verbs to mirror `action_settler.js`.
4. Copy-verbatim: adopt `skills/linux-SKILL.md` and `docs/sky-full-desktop-api.md` as the agent-facing contract for the facade, mapping keysym chord names onto cua's key parser.
5. Leave the deep stack as an optional escalation. cua's per-window background input, AT-SPI actuation, and `delivery_mode` are strictly beyond Sky; keep them behind the richer tool surface and only surface the flat full-desktop verbs when emulating Codex. No Wayland input engineering is required; the hard part already exists.

### Measured parity (cua-driver 0.24, E1 Xvfb/X11 + compositor, 2026-09-09)

Built cua-driver for aarch64 and ran the full action set against a GTK fixture through the harness in `../linux-eval/`. Every core action delivers, verified by the fixture's own event log plus OS oracles, not the driver's own reply.

- Background and invisible (no focus change, no cursor move, no input leak, target never activated): `click`, `set_value`, `type_text` via AT-SPI.
- Foreground (briefly activate the target, act, restore): `double_click`, `right_click`, `scroll`, `drag`, `press_key`, `hotkey`. These return a typed `background_unavailable` refusal in background mode with a `foreground` escalation, then deliver in foreground.
- Observation (AT-SPI tree) and occluded capture (with a compositor for XComposite redirection) both work.

Parity verdict. Linux matches macOS exactly: on both, semantic and text actions run in the background while raw pointer and keyboard escalate to foreground, because neither X11 nor macOS can route raw input to an arbitrary unfocused window. Linux delivers the entire Windows action set too. The single capability Windows has that Linux (and macOS) lack is background raw-coordinate input, which Windows gets from per-HWND synthetic-pointer injection. On Linux that requires owning the compositor (the nested `cua-compositor`, cua's E3 lane) or real-Xorg MPX; it is a platform-gated environment tier, not a missing implementation. No action is unimplemented or silently broken. This closes the Linux implementation gap: every feature present on macOS and Windows is present and delivering on Linux, at the coverage the platform allows.
