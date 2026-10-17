## Capture: per-window occluded + transient/z-index surfaces

### 1. What it is

How each system turns a live on-screen window into pixels for the model. Two hard parts: (a) capturing a window that is partially covered by other windows so the agent sees the real window and not whatever occludes it, and (b) capturing transient UI that renders as a separate window layered above the target (NSMenu popups, popovers, autocomplete dropdowns, tooltips). cua captures exactly one surface per call and pushes menu interaction onto the accessibility rung; Sky's window2 API returns an array of bounded, z-ordered screenshots covering the window and its related transient surfaces.

### 2. Codex/Sky

The window2 contract (CONFIRMED from docs, and the type files shipped in the binary) makes multi-surface capture a first-class return value. `WindowState.screenshots` is an `Array<Screenshot>`, each carrying `zIndex` (larger is visually above), `originX`/`originY` (screen origin of that bounded region), `width`/`height`, and a stable `id`. The doc comment on the field is the whole intent: "Bounded screenshots captured for the window and related transient UI." Actions (`click`, `scroll`, `drag`) take an optional `screenshotId` so a coordinate is interpreted against the specific surface it was read from. The actual capture is done in the closed native service; the shipped mac JS I could read still uses the older single-screenshot `get_app_state` path.

- `codex-corpus/mac/docs/sky-window2-api.md` — the `Screenshot` type (`zIndex`, `originX`, `originY`, `id`, data-URL `url`) and `WindowState.screenshots: Array<Screenshot>`. `ClickInput.screenshotId` / `ScrollInput.screenshotId` / `DragInput.screenshotId` bind a coordinate to one surface.
- `codex-corpus/mac/skills/macos-SKILL.md` — the older `get_app_state` surface where `AppState.screenshot` is a single nullable `Screenshot { url }`; screenshot URLs are `file://` in this environment and read with `fileURLToPath` + `emitImage`. Confirms the single-surface legacy path that window2 supersedes.
- `.../@oai/sky/dist/project/cua/sky_js/src/types/window2/Screenshot.d.ts` and `.../WindowState.d.ts` — the shipped TS types: `zIndex: number`, optional `originX`/`originY`/`width`/`height`, and the "window and related transient UI" comment on `screenshots`.
- `.../@oai/sky/dist/project/cua/sky_js/src/targets/mac/window_result.js` — thin client: reads `skyshot.screenshot.url` (single URL), throws if absent. The z-ordered array is assembled below the JS in the native service.
- `"Codex Computer Use.app/.../SkyComputerUseService"` (native, strings/nm) — CONFIRMED symbols: `captureImageWithFilter:configuration:completionHandler:` (SCScreenshotManager, same API cua calls), plus window-ordering primitives `orderedWindows`, `windowNumber`, `windowNumbersWithOptions:`, `windowNumberAtPoint:belowWindowWithWindowNumber:`, and menu/AX symbols `AXShowMenu`, `AXShownMenuUIElement`, `_currentlyOpenedMenu`. INFERRED: the service enumerates the layered windows over the target, captures each with SCScreenshotManager, and emits them as the z-ordered `screenshots` array. The per-surface capture code itself is not readable (compiled Swift).

### 3. cua-driver

cua does per-window occluded capture correctly but returns a single surface. `screenshot_window_bytes` builds an `SCContentFilter` with `desktopIndependentWindow` for one `window_id` and grabs it with `SCScreenshotManager::capture_image`. Desktop-independent means only that window's pixels are captured, so an occluded window comes back whole regardless of what covers it. This is the occlusion win. But it is one window: a menu or popover that renders as a separate window above the target is not in the frame. Before any pixel action, `px_frame` proves the capture and the WindowServer bounds are one coherent 1x/2x frame and refuses otherwise. Menus are handled on the AX rung, not by screenshot: `invoke_menu` walks `AXMenuBar` by title path and fires `AXPress`/`AXPick`/`AXShowMenu`.

- `rust/crates/platform-macos/src/capture.rs` — `screenshot_window_bytes` → `screenshot_window_bytes_sck` → `build_window_capture_plan`. Key line: `SCContentFilter::create().with_window(&window).build()` (desktop-independent single-window). Output size = `content_rect * point_pixel_scale`, encoded to PNG in-process. `WindowCaptureIdentity` (pid, layer, x/y/w/h) is re-checked after capture (`capture_then_validate_identity`) so bytes never escape if the window moved/resized mid-grab; shell fallback is `screencapture -l <id> -x -o`.
- `rust/crates/platform-macos/src/tools/px_frame.rs` — `validate_capture_frame` proves `capture_px / bounds` is ~1.0 or ~2.0 on both axes (tolerance 0.03) and returns the backing scale; `resolve_window_px_frame` and `resolve_or_refuse`. `PxFrameError::FrameMismatch` is the issue #2237 guard: a parent-window capture cannot masquerade as a panel's frame. Refusal is explicit, never a silent screen-absolute fallback.
- `rust/crates/platform-macos/src/tools/get_window_state.rs` — the perception entry point. `should_capture` → `crate::capture::screenshot_window_bytes(window_id)` → `resize_png_if_needed(raw, max_dim)` → one `Content::image_png`. Emits `screenshot_width/height`, `screenshot_scale`, `screenshot_frame_valid`. It returns exactly ONE screenshot of the requested window; there is no `screenshots` array and no transient-surface capture.
- `rust/crates/platform-macos/src/tools/get_desktop_state.rs` — full main-display capture at native pixel size (via `screencapture -x`). This is the only cua surface that incidentally shows a transient menu, because a menu window paints onto the display; but it is whole-desktop, gated behind desktop scope, and carries no per-surface z-order metadata.
- `rust/crates/platform-macos/src/tools/zoom.rs` — crops a region of the same single-window capture to a <=500px JPEG (`crop_png_to_jpeg`) and registers a `ZoomContext` so `from_zoom` clicks translate back. Still one surface.
- `rust/crates/platform-macos/src/tools/invoke_menu.rs` — menus via AX: `normalized_path` (1-16 title segments), `resolve_exact_prefix` re-resolved from the live app root each hop, `choose_action` (`AXPress`/`AXPick`/`AXShowMenu`/`AXOpen`). cua invokes menus, it does not screenshot them.
- `rust/crates/platform-macos/src/windows.rs` — the primitives a port needs already exist: `WindowInfo { window_id, pid, bounds, layer }`, `enumerate_windows(..., LayerFilter::AnyLayer)` reaches accessory layers (tooltips, popovers, NSMenu, Dock), and `z_index_from_front_to_back` normalizes CGWindowList order to "higher is frontmost." `all_windows_any_layer` exists but is deliberately kept off `list_windows`/`get_window_state` so callers are not swamped by transient windows.
- `rust/crates/cua-driver-core/src/capture_mode.rs` — capture_mode is deprecated/ignored: get_window_state always returns tree AND screenshot.

### 4. Delta

| Aspect | cua-driver | Codex/Sky window2 |
|---|---|---|
| Occluded single-window capture | Yes, SCContentFilter desktopIndependentWindow | Yes, SCScreenshotManager (same API) |
| Capture-frame coherence proof (1x/2x, refuse mismatch) | Yes, `px_frame::validate_capture_frame` (issue #2237) | Not found in readable code; `Screenshot` carries size/origin but no proof step |
| Multiple z-ordered surfaces per call | No, single `image_png` | Yes, `screenshots: Array<Screenshot>` with `zIndex` |
| Transient UI (menu/popover/dropdown/tooltip) as a captured surface | No, only via whole-desktop `get_desktop_state` | Yes, "window and related transient UI" |
| Per-surface origin metadata for coordinate binding | No | Yes, `originX`/`originY` + `screenshotId` on actions |
| Menu interaction | AX only, `invoke_menu` (AXPress/AXPick) | AX (`AXShowMenu`) plus captured menu surface |
| Layered-window enumeration + front-to-back z-index | Yes, already in `windows.rs` (unused by capture) | Yes, `orderedWindows` in native service |

- Matches: occluded per-window capture uses the same Apple API on both; both prefer AX for menus.
- cua ahead: px_frame coherence proof and post-capture identity re-check make a returned surface provably the right one, which Sky's readable surface does not guarantee.
- Codex ahead: window2 returns transient surfaces as a z-ordered array with per-surface origins, so an open menu, autocomplete, or popover is visible and clickable by pixel against the exact surface it was read from. cua drops these unless the agent escalates to full-desktop capture.

### 5. Recreate/port plan

Goal: `get_window_state` returns a z-ordered array of surfaces (target window at the bottom, each overlapping transient window above it), matching Sky's `screenshots` shape. cua already has every primitive; this is a native-patch, not a rewrite.

1. Add a `screenshots` array field to the get_window_state structured output and (optionally) one `Content::image_png` per surface. `native-patch` — extend `rust/crates/platform-macos/src/tools/get_window_state.rs`. Keep the existing single top-level screenshot for back-compat, add `screenshots: [{ id, zIndex, originX, originY, width, height }]`.

2. Build the overlay set: enumerate `visible_windows`/`all_windows_any_layer` in `rust/crates/platform-macos/src/windows.rs`, filter to windows whose `bounds` intersect the target's bounds and whose front-to-back position is above the target (or same-owner child windows and `layer != 0` accessory windows: NSMenu, popover). `native-patch` — a new helper `overlay_windows_over(target: &WindowInfo) -> Vec<WindowInfo>` reusing `z_index_from_front_to_back`.

3. Capture each overlay with the existing `screenshot_window_bytes(window_id)` and validate each with `px_frame::validate_capture_frame`, attaching `originX/originY` from that window's `bounds` and `zIndex` from the front-to-back index. `thin-port` — loops the code that already exists in `capture.rs` + `px_frame.rs`; the native capture gate serializes SCK calls, so capture surfaces sequentially or widen the gate.

4. Bind coordinates to a surface: add an optional `screenshot_id` to `click`/`scroll`/`drag`/`double_click`/`right_click` that selects which surface's px frame `px_frame::resolve_or_refuse` uses, mirroring window2's `screenshotId`. `native-patch` — schema plus a lookup from id to `(window_id, WindowPxFrame)`; reuse the `zoom_registry`/`resize_registry` pattern in `tools/mod.rs`.

5. Copy the contract wording verbatim where useful: the `Screenshot` type comments and field semantics from `codex-corpus/mac/docs/sky-window2-api.md` (`zIndex` "larger is above", `originX/Y` "screen origin of this bounded region") into the new schema descriptions. `copy-verbatim` — docs only.

6. Keep `invoke_menu` as the preferred menu path. The captured menu surface is a grounding/fallback aid for non-AX or custom-drawn menus, not a replacement. No change to `invoke_menu.rs`.

Net effort: `native-patch`. No new capture technology is required, and no Sky code is copyable (native service is compiled Swift); the work is assembling cua's existing layered-window enumeration and per-window SCK capture into a z-ordered array behind get_window_state.
