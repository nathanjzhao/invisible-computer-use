## Ghost cursor overlay

### What it is
A synthetic "agent" pointer painted in a transparent, click-through window so a human watching the screen can see where the automation is acting. It glides along a planned path, shows a click/press cue, and must stay visually correct when other windows sit in front of it. Both systems drive this with a native macOS overlay window; cua adds a full cross-platform theme system on top.

### Codex/Sky
Codex renders the cursor entirely in the native `SkyComputerUseService` binary. The JS layer never renders it: the only `cursor` hits in the JS are text-caret fields (`cursor_before`/`cursor_after` in `SelectText`), no overlay code. The overlay is a borderless, always-on-top overlay window — the nested `ComputerUseCursor.Window` class (`_TtCC11ComputerUse17ComputerUseCursor6Window`) — driven by an XPC method whose whole payload is a point plus an active flag. (An exact window level is not recoverable from the binary; the overlay-level machinery is `overlayWindowLevelReasons`.)

- `.../SkyComputerUseService` symbols `ComputerUse.ComputerUseCursor` (source `ComputerUse/ComputerUseCursor.swift`) with nested `Window`, `Style`, and `AppMonitor` classes and a cursor delegate whose methods are in the binary (`computerUseCursorDidFinishMove`, `computerUseCursorDidHide`, `computerUseCursorLocationDidChange`; the `ComputerUseCursorDelegate` protocol name itself is inferred from those methods) — the top-level cursor controller.
- Two render styles under the nested `Style`, rendered through a generic `CursorView`: a soft "fog" look (`FogCursorViewModel`) and a plain software-pointer look (`SoftwareCursorStyle`). No Lottie: `strings | grep -ic lottie` returns `0`.
- IPC surface `setComputerUseCursorLocationWithX:y:isActive:withReply:` and the Swift `setComputerUseCursorLocation(on:point:isActive:)` — the only cursor control input is position + `isActive`. `isActive` toggles the click/press visual.
- Motion strings `BezierAnimation`, `BezierFunction`, `BezierParameters`, `arcSize`, `SpringAnimation` — a Bezier glide with an arc parameter plus a settle spring. No `dubins`/`turnRadius`/`peakSpeed` strings appear.
- Occlusion machinery: `overlayWindowLevelReasons`, `occlusionState`, `windowDidChangeOcclusionState:`, `applicationDidChangeOcclusionState:`, `clippingView`, `setClipsToBounds:`, `So12CAShapeLayerC`, plus `AppMonitor`, `SystemFrontmostApplicationTracker`, `targetWindowID`, `visibleFrame`. This is a mask/clip pipeline: the always-on-top cursor is clipped to the target window's visible region and reacts to occlusion-state changes, so it disappears exactly where a covering window overlaps it.
- `AccessibilitySupport.VirtualCursor` protocol (`virtualCursor`) — the AX-side cursor abstraction the service publishes.

### cua-driver
cua implements the shared math and state in the `cursor-overlay` crate and keeps the macOS `NSWindow` adapter thin. Motion is a documented 1:1 port of OpenAI's C#/Swift reference, so the glide feel already matches Codex.

- `rust/crates/cursor-overlay/src/motion.rs` — `MotionConfig` ("1:1 port of `AgentCursorMotion.cs`"): `peak_speed=900`, `turn_radius=80`, `arc_size`/`arc_flow`, `spring=0.72`, `idle_hide_ms=20000`; plus the `Spring` settle state.
- `rust/crates/cursor-overlay/src/bezier.rs` — `CubicBezier` + `build_motion_bezier` using `arc_size`/`arc_flow`, "1:1 port of `Bezier.swift`". This is the same Bezier+arc+spring model Codex ships.
- `rust/crates/cursor-overlay/src/path_planner.rs` — `PathPlanner::plan`, a full Dubins arc-straight-arc planner (`dubins_lsl`..`dubins_lrl`) with linear fallback. cua has this in addition to the Bezier path; Codex's binary shows no Dubins.
- `rust/crates/cursor-overlay/src/theme.rs` + `theme_artifact.rs` — `CursorVisualState`, `CursorAction` (12 semantic actions), `session_fill_rgba` palette, `DEFAULT_CURSOR_FILL=[94,192,232]`, `paint_compiled_theme_with_tint`. Artwork is a compiled dotLottie artifact rasterized through tiny-skia/Skia.
- `rust/crates/cursor-overlay/src/render_state.rs` — `RenderStateCore`, `paint_cursor`/`render_frame`; `click_t` legacy click-pulse ring (`+dt*4.0`, full pulse over 0.25s) with the real click cue now coming from the theme's one-shot `click` action; `FocusRect` cyan highlight.
- `rust/crates/cursor-overlay/src/session_badge.rs` + `badge_glyphs.rs` — session-name badge with delivery/target chips, 2s hold + 400ms fade.
- `rust/crates/cursor-overlay/src/z_order.rs` — `ZOrderEnforcer::reassert(target)` trait. Its doc names `NSWindowDidChangeOcclusionState` as a future seam that is not yet wired.
- `rust/crates/cursor-overlay/src/lib.rs` — `OverlayCommand` (`MoveTo`/`SnapTo`/`ClickPulse`/`SetPressed`/`BeginAction`/`ShowFocusRect`/`PinAbove`...), `OverlayMsg`, `CursorRegistry`, `track_pointer_command` with `CLICK_OFFSET=16`.
- `rust/crates/platform-macos/src/cursor/overlay.rs` — the native window: `initWithContentRect: styleMask: 0` borderless, `setOpaque:false`, `setIgnoresMouseEvents:true`, `setSharingType:1`, `setCollectionBehavior:` CanJoinAllSpaces|FullScreenAuxiliary|Stationary, and critically `setLevel: 0i64` (NSNormalWindowLevel). Z is managed by `dispatch_pin_above` / `MacZOrderEnforcer::reassert` calling `orderWindow: 1 relativeTo: target_wid` (NSWindowAbove) so the overlay sits one slot above the target and behind anything covering it. 60fps `render_loop`, `hardware_cursor_position` hover poll, `pixmap_to_cgimage` for CALayer contents.
- `rust/crates/platform-macos/src/cursor/state.rs` — per-session `CursorRegistry` with a session-ended resurrection guard.
- `rust/crates/platform-macos/src/tools/cursor_tools.rs` — MCP tools `set_agent_cursor_enabled`, `set_agent_cursor_motion`, `set_agent_cursor_theme`, `get_agent_cursor_state`.
- Docs: `docs/cursor-themes.md`, `docs/cursor-modifier-badge-restructuring-plan.md`.

There is no clip-to-visible-region path in cua: grep for `occlus|clip to|visible region|CAShapeLayer|mask` across `cursor-overlay/src` and `platform-macos/src/cursor` finds only the z_order.rs comment and the borderless-styleMask comment. Occlusion is handled purely by re-ordering the whole overlay relative to the target.

### Delta
| Aspect | Match / who is ahead |
| --- | --- |
| Bezier glide + arc + settle spring | Match. cua ports the same reference model; `arc_size`/`spring` vs Codex `arcSize`/`SpringAnimation`. |
| Transparent, non-activating, click-through window | Match. Both borderless + `setIgnoresMouseEvents:true`. |
| Click/press cue | Match. Codex `isActive`; cua `ClickPulse` + theme `click` one-shot. |
| Follows frontmost/target app | Match. Codex `AppMonitor`+`SystemFrontmostApplicationTracker`+`targetWindowID`; cua `dispatch_pin_above` + window list. |
| Theming | cua ahead. 12-action dotLottie themes, custom installable themes, per-session color palette, focus-rect, Dubins planner, cross-platform crate, 4 typed tools, multi-cursor registry. Codex has only Fog vs Software styles, no Lottie, no theme authoring. |
| Occlusion correctness | Codex ahead. Codex keeps an always-on-top overlay and clips the cursor to the target window's visible region via `clippingView`/`CAShapeLayer` mask driven by `occlusionState`/`visibleFrame`, so it is hidden precisely where covered and reacts on `windowDidChangeOcclusionState:`. cua sits at normal level 0 (`setLevel: 0i64`) and re-orders the whole overlay relative to the target, hiding the entire cursor behind any occluder rather than masking to the exposed sub-region, and polls on the render tick instead of observing occlusion events. |

### Recreate/port plan
1. Keep cua's overlay, theme, motion, and badge stack as-is. It already matches Codex on the pointer look and glide feel and exceeds it on theming, so no corpus files apply. There are no cursor docs or JS in the Codex corpus to copy. Effort: copy-verbatim of nothing.
2. Add Codex's occlusion model to `rust/crates/platform-macos/src/cursor/overlay.rs`. Compute the target window's visible region from the existing `crate::windows::visible_windows()` z-ordered list, build a `CAShapeLayer` mask (or set a clip path on the content view) from the exposed rectangles, and apply it each frame so the cursor is masked out only where a covering window overlaps it, instead of the whole overlay dropping behind. Effort: native-patch, moderate.
3. Wire an `NSWindowDidChangeOcclusionState`/`windowDidChangeOcclusionState:` observer into the `z_order.rs` `reassert` seam so occlusion updates are event-driven rather than tick-polled, matching Codex `occlusionState`. Effort: native-patch, small.
4. Optional: expose an "always on top" window band as an alternative to `setLevel: 0` for hosts that want Codex's always-on-top behavior, gated so the default keeps cua's normal-level, list_windows-visible contract. Effort: native-patch, small.
5. IPC parity is already covered. Codex's single `setComputerUseCursorLocationWithX:y:isActive:` maps onto cua's `SnapTo`/`MoveTo` + `BeginAction(Click)`; no new tool is needed, at most a thin wrapper if a Codex-shaped entry point is required. Effort: thin-port.
