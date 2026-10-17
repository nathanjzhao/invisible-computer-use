# COVERAGE.md — Codex/Sky Computer Use vs the 13-section spec set

Does our 13-section spec set (`~/dev/invisible-computer-use/specs/01..13`) cover the full Codex/Sky computer-use feature surface? This matrix takes every feature in the four ground-truth inventories (model-facing `@oai/sky` tools, the native `SkyComputerUseService` binary, and the policy/per-app corpus) and asks three things: which spec section addresses it (or NONE), whether cua-driver 0.24.0 already has an equivalent (cross-checked against the live `cua-driver list-tools`/`describe`/`manifest` inventory), and the build effort to reach parity. CONFIRMED items were read in the specs or the cua inventory; where a section only touches a feature obliquely it is marked `partial`.

Effort legend: `none` (cua already at parity) / `copy-verbatim` (drop in a prompt or .md) / `thin-port` (wiring, no new native code) / `native-patch` (bounded new native code) / `large-build` (new subsystem).

> The `cua` column is raw cua-driver 0.24.0. Rows tagged **[BUILT]** in the Gap column are now provided by invisible-computer-use: the owned crates under `impl/` and/or `impl/native/cua-driver.patch`. The patched-driver features (system text-selection reader, native AX diff + stable ids, generalized `perform_secondary_action`, transient-surface z-ordered capture, the occlusion cursor clip, host readiness / `host_state`) only work when the REBUILT driver from `impl/install.sh` is the running daemon; a stock `CuaDriver.app` install runs the old 0.24.0 code and does NOT have them.

Current evidence: [app/GAPS.md](../app/GAPS.md) and the [round-twelve history/memory audit](../study/audit/parity-round12-2026-10-09/README.md), with the expanded target retained in [full scope](../study/audit/parity-round4-2026-10-09/SCOPE.md).
The [earlier seam audit](../study/audit/seam-audit.json) and the `cua` cells below are
historical inventories. **[BUILT]** means implemented in the layer or patch, not
acceptance of the latest binary. Current model, recording, memory and client-OS
claims are bounded by the dated evidence; they do not establish full equivalence.

## Observation

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| get_app_state (mac) + disableDiff | 01 | partial | **[BUILT]** layer/native diff and disableDiff; raw driver still resends full trees | native-patch |
| get_window_state (window2/windows) + include_screenshot/include_text | 09/01 | partial | **[BUILT]** the patch's `read_selection` adds `focused_element`/`selected_text`/`document_text` | thin-port |
| Unified "Skyshot" capture (screenshot+AX+text in one) | 01 | has | cua `get_window_state` bundles all three | none |
| Numbered element-ID indexing of AX tree | 01 | partial | **[BUILT]** conservative cross-turn identity; ambiguous replacements get new IDs; snapshot indices/tokens remain ephemeral | native-patch |
| AX render-tree diffing between turns | 02 | absent | **[BUILT]** layer/native lineage with identity and cumulative-label audit guards; relabels require evidence | large-build |
| AX value truncation descriptor (range+count) | 01 | partial | **[BUILT 2026-10-04]** `max_value_chars` (default 4096, inferred) and a `_value_truncations` descriptor in the layer; run on TextEdit ([gaps-parked](../study/audit/gaps-parked-2026-10-04/README.md)) | thin-port |
| Element-uniqueness frequency table | 02 | absent | **[BUILT, macOS 2026-10-06]** recovery counts exact semantic matches before and after refresh; no general frequency index | native-patch |
| Lazy refetchable AX elements + invalidation monitor | 01 | absent | **[BUILT, macOS 2026-10-06]** bounded semantic revalidation and unique replacement recovery; per-action checks replace Sky's notification monitor. [Evidence and limits](../impl/native/RECOVERY.md) | native-patch |
| System-wide text-selection reader | 01 | absent | **[BUILT]** selection reader plus UTF-16 AXSelectedTextRange writer; historical readback retained, latest acceptance pending | native-patch |
| Window ordering / key-window tracking | 07/05 | has | cua `windows.rs` z-order + `frontmost_pid` | none |
| list_apps (mac) | NONE | has | cua `list_apps` exists; no mac-focused spec section documents it. **[BUILT]** the patch lists apps from the Process Manager, so apps launched after the daemon started appear | none |
| App usage catalog (running + last-14-days useCount) | NONE | partial | cua `list_apps` lacks 14-day usage-frequency metadata | thin-port |
| list_windows (window2) | 09 | has | cua `list_windows` (off-screen/other-Space) | none |
| get_window rehydrate (window2) | 09 | has | cua addresses by `window_id` via list_windows+get_window_state | none |
| screenshotId coordinate binding | 07 | absent | **[BUILT, partial]** every driver returns `screenshot_id`; click/double/right/drag refuse `stale_capture` against it on macOS, Linux and Windows; point scroll is bound on Linux only; `screenshots[]` surfaces are not bindable | native-patch |
| Sky runtime error surface (named codes) | 13/06 | has | cua ships richer structured refusal codes | none |
| action_settler post-action wait | 10 | partial | cua has per-action delays + mac auto-wait; no single `post_action_sleep_ms` knob | thin-port |

## Input

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| click (mac) | 06 | has | cua `click` (AX index/token or pixel), verified | none |
| scroll (mac) | 06 | has | cua `scroll` | none |
| drag (mac) | 06 | has | cua `drag` (single segment, same coord space) | none |
| type_text (mac) | 06 | has | cua `type_text` with AX + CGEvent fallback | none |
| press_key (mac) incl. chord/hotkey | 06 | partial | cua `press_key`/`hotkey`; keysym vocab differs (own table vs xdotool/X keysyms). **[BUILT]** the patch refuses session chords (Ctrl-Cmd-Q, log out, Force Quit, power) | thin-port |
| paste (mac, format text/md/html + clipboard restore) | 06 | absent | **[BUILT, partial]** plain-text clipboard save/paste/restore in icu; rich md/html and all clipboard formats are not implemented | thin-port |
| select_text (mac, match+prefix/suffix+selection_type) | 06 | absent | **[BUILT, partial]** text/prefix/suffix to UTF-16 AXSelectedTextRange; no WebKit marker range implementation | native-patch |
| set_value (mac) | 06 | has | cua `set_value` (AX value write). **[BUILT]** the patch refuses non-settable controls, steps steppers, and errors when nothing moved | none |
| perform_secondary_action (mac) = menu/AX invoke | 06 | partial | **[BUILT]** the patch generalizes it (any advertised AX action name / window2 label; fail-closed for unadvertised) | thin-port |
| Rich input actions beyond click/type (paste/secondary/set_value/select/scroll-by-pages/drag/mouse down-up) | 06 | partial | **[BUILT, partial]** icu plain-text paste/select_text/secondary; low-level mouse down/up remain driver-specific | native-patch |
| Virtual key-press synthesis from strings | 06 | has | cua `keyboard.rs` string->keycode+modifiers | none |
| Menu-bar and menu handling (AXMenuBar, shortcut chars) | 06 | has | cua `invoke_menu` walks AXMenuBar by path. **[BUILT]** the patch drops the Apple menu from observations and refuses paths into it | none |
| UI text-marker ranges (WebKit-style) | 06 | absent | no UITextMarker/Range primitives | native-patch |
| System focus-steal prevention | 05 | has | cua `focus_steal.rs` is a literal port + deadline/janitor. It never fired in a `--no-overlay` daemon; **[BUILT]** the patch runs the main run loop there and restores through WindowServer | none |
| Synthetic app-focus enforcer (background control) | 05 | partial | cua ships focus-guard layer 3 only; layers 1-2 (AX enablement + synthetic AXFocused/AXMain) are a documented gap | native-patch |
| click (windows) | 09 | has | cua UIA/inject click | none |
| scroll (windows) | 09 | has | cua UIA/inject scroll | none |
| drag (windows) | 09 | has | cua synthetic-touch drag | none |
| type_text (windows) | 09 | has | cua PostMessage/SendInput text | none |
| press_key (windows) incl. chord | 09 | partial | keysym vocab differs | thin-port |
| set_value (windows) | 09 | has | cua UIA ValuePattern | none |
| perform_secondary_action (windows) | 09 | has | cua UIA Invoke/Expand/Collapse/Select patterns | none |
| click (linux full-desktop, +key/duration) | 10 | partial | **[BUILT 2026-10-04]** `desktop_click` (`duration` needs the native driver); real X11 and headless sway ([gaps-linux-2](../study/audit/gaps-linux-2-2026-10-04/README.md)) | thin-port |
| drag (linux, path[] >=2 pts + key) | 10 | partial | **[BUILT 2026-10-04]** `desktop_drag` with a path of two or more points and a held key (native driver) | thin-port |
| drag_handle (linux stateful start/move/end) | 10 | partial | **[BUILT 2026-10-05]** `desktop_drag_handle` start, move_to, end over the native `drag {scope: desktop, hold}`; real X11 and headless sway ([gaps-linux-3](../study/audit/gaps-linux-3-2026-10-05/README.md)); on native Wayland the button needs a driver that lives between calls | thin-port |
| move (linux pointer + key) | 10 | partial | **[BUILT 2026-10-04]** `desktop_move`; a lone move reaches the client on a headless sway since gaps-linux-3 (the pointer waits for the client to bind it) | thin-port |
| press_key (linux, chord + duration) | 10 | partial | **[BUILT 2026-10-05]** `desktop_press_key` (chord, `duration` through the native `hold_ms`); real X11 and headless sway | thin-port |
| scroll (linux, direction+pixels+key) | 10 | partial | **[BUILT 2026-10-05]** `desktop_scroll` (wheel clicks of 40 pixels, a held `key` through the native `modifier`); real X11 and headless sway | thin-port |
| type_text (linux) | 10 | partial | **[BUILT 2026-10-05]** `desktop_type_text` into the keyboard focus; real X11 and headless sway | thin-port |

## Capture

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| get_screenshot (linux full-desktop) | 10 | has | **[BUILT 2026-10-04]** `desktop_screenshot`, the agent cursor drawn in | thin-port |
| Per-window occluded screenshot (private WindowServer SPI) | 07 | has | cua SCContentFilter desktopIndependentWindow (same result via public SCK); **[BUILT]** the patch adds the occlusion cursor clip (mac) | none |
| Hardware-accelerated window screenshot / ScreenCaptureKit | 07 | has | cua uses SCScreenshotManager | none |
| Transient-surface capture (z-ordered screenshots array + origins) | 07 | absent | **[BUILT]** layer-aware selection; logical bounds/decoded scale; explicit visible-region composites. Latest menu/bounds live acceptance pending | native-patch |
| Window corner-radius masking | NONE | absent | no corner-radius mask on captures/overlays; not in any spec | thin-port |
| Stage Manager integration (pull offscreen window to stage) | 05 | absent | cua refuses off-Space; no stage-pull; spec 05 covers off-Space concept only | native-patch |
| start/stop_audio_recording (mac) | 16 | absent | **[PARTIAL 2026-10-09]** app system-audio capture verified through the observer; reference WAV/tool contract pending | native-patch |
| start/stop_audio_recording (windows) | 16 | absent | **[PARTIAL 2026-10-09]** app capture and source consent verified on Windows 11; reference WAV/tool contract pending | native-patch |
| start/stop_audio_recording (linux, ffmpeg PulseAudio) | 16 | absent | Source-specific observer extension available; current Linux acceptance pending | native-patch |
| Microphone audio recording (native) | 16 | absent | **[PARTIAL 2026-10-09]** separate microphone consent and capture verified on Windows; other native platforms pending | native-patch |

## Cursor

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| Software Cursor (app-drawn ghost overlay) | 08 | has | cua overlay cursor (borderless click-through NSWindow) | none |
| Fog animated physics cursor | 08 | has | cua bezier/spring/dubins motion, 1:1 port of the reference model | none |
| Cursor style switching + "current" selector | 08 | has | cua `set_agent_cursor_theme` + registry | none |
| Multi-cursor management (all cursors) | 08 | has | cua per-session `CursorRegistry` | none |
| Virtual (AX-based) cursor | 08/06 | partial | cua drives AX actions directly, not as a cursor abstraction | thin-port |
| "Close enough" cursor targeting | 08 | partial | **[BUILT 2026-10-04]** native `close_enough_distance_px` and `close_enough_progress` end a glide early; measured on a real Mac, 0.45x to 0.82x of the full time ([gaps-mac-2](../study/audit/gaps-mac-2-2026-10-04/README.md)). The shared detached animation policy (`motion_for_action`) is a different feature | thin-port |
| Sky Lens highlight overlay (frame-sequence glow) | 08 | partial | **[BUILT]** the patch's `cursor-overlay/src/skylens.rs` draws the 45-frame Codex lens sequence around the acting cursor under the Codex theme, on every platform, in place of cua's FocusRect. Not built: clipping the glow to the visible region from occlusion events | native-patch |
| Remote-hosted PiP live view (cross-process CALayer stream) | NONE | partial | cua `experimental_pip` is a basic screenshot preview, not a shared-layer live stream; no spec section | large-build |
| Transient-surface / remote CALayer context | NONE | absent | no cross-process remote-layer primitive; not specced | large-build |

## Cross-platform

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| Platform target auto-selection | 13 | has | cua platform crates + `manifest.features` (portal/wayland gated) | none |
| startApp (mac internal, not model-facing) | 13 | has | cua exposes model-facing `launch_app`; exceeds Codex's implicit mac launch | none |
| launch_app (windows) | 09 | has | cua `launch_app` (bundle_id/name, background) | none |
| activate_window / bring-to-front (windows) | 09 | has | cua `bring_to_front` (verified focus+z-order) | none |
| Windows native-pipe / helper approval | 09 | partial | cua has UIAccess worker + authorization; different mechanism (no shipped helper.exe) | native-patch |
| Running-process enumeration (pid/uid/name/path) | NONE | partial | cua `list_apps` lists running; no full uid/executablePath process filter; not specced | thin-port |
| Power/display wake assertions | NONE | absent | no wake/power assertion while a session runs; not specced | thin-port |
| Concurrent multi-app instances | 11 | has | cua drives multiple pids/windows + sessions | none |
| Native iMessage/Messages tools (find/read/search/count/read-image via chat.db) | PARTIAL | standalone read/image MCP service and native Contacts helper | signed Contacts fixture acceptance, scoped host forms and bounded workers; app-native UI and successful reference data comparison remain open ([round seven](../study/audit/parity-round7-2026-10-09/README.md)) | native-compare / host-integration |
| Two-phase Messages send with user-editable approval | PARTIAL | reviewed commit, native AppleEvents helper and durable journal | immutable files, revalidation, uncertainty/restart recovery and group creation tested through a native self-fixture; real-app Automation consent and signed-in delivery remain open ([round eight](../study/audit/parity-round8-2026-10-09/README.md)) | signed-in native acceptance |

## Safety / session

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| Per-app approval elicitation ("Allow ... to use X") | 11 | partial | **[BUILT]** icu MCP elicitation and per-app approval gate | native-patch |
| App policy gating allowed/denied/forbidden (bundle id) | 11 | partial | **[BUILT]** layer bundle-ID verdicts via denied/forbidden configuration | thin-port |
| Physical Escape / user-intervened interrupt (exit 130) | 11 | partial | cua has revoke/stop + idle sweep; no in-turn physical-Escape abort | native-patch |
| Per-app risk policy + persistent approval | 11 | partial | **[BUILT, partial]** layer risk label and session/always bundle-ID approval stores; no full native org policy reproduction | thin-port |
| MCP app-approval gating (persisted per-app) | 11 | partial | **[BUILT]** icu app gate and persisted stores | thin-port |
| Idle-timeout auto-end | 11 | has | cua `evict_idle` (5-min default) | none |
| Per-session app allowlist (TOML) | 11 | partial | **[BUILT]** session TOML shape/roundtrip tested; no captured reference store for byte comparison | thin-port |
| Persistent per-app approval allowlist (JSON) | 11 | partial | **[BUILT]** persistent JSON shape/roundtrip tested; no captured reference store for byte comparison | native-patch |
| Computer Use Confirmations Policy (approval taxonomy) | 11 | absent | **[BUILT]** embedded policy byte-equal to the reference/golden | copy-verbatim |
| Prompt-injection / confirmation hygiene rules | 11 | absent | **[BUILT]** policy text shipped verbatim; format fidelity is not complete behavioral equivalence | copy-verbatim |
| node_repl trusted-runtime restriction | 13 | partial | cua has trusted transport/services trust model; different runtime | thin-port |
| Enterprise / organization policy layer (organization_id) | 11 | partial | cua has managed-policy layer + hash; org-id provenance wiring needed | native-patch |
| Built-in safety forbidden-app layer + override | 11 | partial | encode as built-in managed ceiling + trusted override | thin-port |
| Composer permissions mode | 11 | has | cua enforced Standard/Bounded/Unrestricted exceeds Codex's analytics-only mode | none |
| Permission consent window UI (catalog) | 11 | partial | cua has TCC gate/panel; no multi-permission catalog window | native-patch |
| System Settings accessory-window guidance | 11 | partial | cua opens the right pane/deep link; no floating accessory arrow overlay | native-patch |
| Broad TCC permission management (FDA/Contacts/Calendar/Reminders/Location/AppleEvents) | 11 | partial | cua handles only Accessibility + Screen Recording | native-patch |
| MDM enterprise control (native org verdict, no profile) | 11 | partial | cua managed policy; neither ships an MDM profile | native-patch |
| User-facing stop/control UX + training-data control | 11 | partial | cua has revoke/stop; banner + training-data toggle are host-UI concerns | thin-port |
| Screen-unlock / CGSession observation | 11 | absent | Expanded target includes an authenticated workflow; current input refuses locked sessions | large-build |
| Lock-screen guardian / auto-unlock subsystem | 11 | absent | Expanded target includes authenticated locked-session use; not implemented | large-build |
| Lock-screen SecurityAgent authorization plugin | 11 | not-applicable | explicit do-not-copy | none |
| SecurityAgentPlugin installer mechanics | 11 | not-applicable | explicit do-not-copy | none |
| Lock-screen physical-input bail-out + socket peer verify | 11 | not-applicable | explicit do-not-copy | none |
| Codex menu-bar status item | NONE | not-applicable | host-UI chrome; cua is a driver, not an app shell | none |
| Signed distribution + normal/relaxed build flavors | NONE | partial | cua is Developer-ID signed (com.trycua.driver); no dual normal/relaxed flavors | none |

## Transport

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| node_repl JavaScript entry (persistent runtime) | 13 | partial | cua MCP server is the alternative runtime (no JS REPL) | native-patch |
| nodeRepl.write (text to transcript) | 13 | has | cua returns MCP tool-result text | none |
| nodeRepl.emitImage (image into input) | 13 | has | cua `Content::image_png` | none |
| nodeRepl.rpc("sky") bridge | 13 | partial | cua daemon RPC / `handle_request` is the analogue | thin-port |
| @oai/sky sky proxy import (setup RPC method discovery) | 13 | partial | cua `tools/list` registry is the discoverable analogue | thin-port |
| Native-pipe length-prefixed transport + ping version gate | 13 | partial | cua uses newline JSON + `initialize`/`DaemonMetadata`; **[BUILT 2026-10-04]** `icu sky-pipe` serves the 4-byte LE + ping protocol on a unix socket and a Windows named pipe | thin-port |
| Authenticated IPC sender context | 13 | has | cua `AuthenticatedActionConnection` + transport lease | none |
| Extensive analytics telemetry (protobuf schema) | 13 | partial | cua has content-free telemetry; not the Codex protobuf envelope schema | native-patch |
| Statsig feature gating / experiments | NONE | absent | cua has channel/telemetry toggles, no embedded experiment client; not specced | large-build |
| Writes Codex MCP-server config (TOML) | NONE | partial | cua `mcp-config` prints guidance + `skills install` symlinks; does not write codex config.toml; not specced | thin-port |

## Memory

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| Skysight ambient observation service | 03 | partial | native Mac labels/values plus separately approved Chrome page context; 23 fresh-profile browser checks and 15 native regressions pass; other browsers, frames, ambiguous windows, events/screenshots, Windows/Linux and packaged acceptance remain open ([round eleven](../study/audit/parity-round11-2026-10-09/README.md)) | rich-capture / native-compare |
| Skysight memory pipeline / summarization (10min/6h) | 03 | partial | memory stream shares approved focus/control content and capture time; automatic input receives canonical domains; reviewed imports can share redacted values and pass real-model recall, scope and deletion checks; automatic content summaries, executable replay and reference cadence comparison remain open | content / native-compare |
| Skysight observation policy (allow/block, private-browsing) | 03 | partial | app/domain gates and separate browser consent tested; actual Chrome incognito windows excluded before capture, pinned window/tab identity checked; other browser and platform contexts remain open | native-context |
| Skysight skill/automation suggestion engine | 03 | absent | no auto skill/automation suggestion from observed activity | large-build |
| Skysight two-tier cadence + phase2 consolidation | 03 | absent | **[BUILT]** `icu memory serve` runs `skysight::schedule`: closed 10min windows, closed 6h windows with children, then phase2 into `MEMORY.md`/`USER_PROFILE.md` (cua itself has none) | large-build |
| Skysight memory security boundary (prompt) | 03 | absent | portable prompt contract; nothing to defend yet | copy-verbatim |
| Computer-History MCP server (Codex, Skysight-backed) | 03 | partial | five captured control schemas implemented over the app's shared store; 24 UI/backend/MCP checks pass; exact reference behavior remains unverified | native-compare |
| Record & Replay event-stream recorder (CGEvent tap of user) | 12 | partial | cua records driver actions per-turn, not a user-event CGEvent stream | large-build |
| Event stream tagged with model/prompt/run metadata | 12 | partial | **[BUILT 2026-10-04]** `record_start {run}` stamps model, prompt hash and run id into `action.json` and `evidence.json` | native-patch |
| Dictation (voice input path) | NONE | absent | no dictation; not specced | large-build |

## Per-app

| Codex feature | Spec | cua | Gap | Effort |
|---|---|---|---|---|
| Playbook: Slack | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: Notion | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: Spotify | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: Apple Music | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: Numbers | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: Clock | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| Playbook: iPhone Mirroring | 04 | absent | **[BUILT]** embedded verbatim; all 7 pack hashes match reference | copy-verbatim |
| App-specific instruction injection engine (native resolve + `<app_specific_instructions>` prefix, first-touch dedup) | 04 | absent | **[BUILT]** bundle-ID pack map, session first-touch injection and Spotify supplement | native-patch |

## Completeness verdict

This is a raw-driver feature census with annotations for implemented additions, not a Codex parity score. Byte checks pin the packs, policy, tool names and reference error vocabulary. Offline audit fixes cover the eight reported correctness findings with red/green evidence. They do not establish complete behavioral equivalence or latest native desktop acceptance.

The current boundary is in [app/GAPS.md](../app/GAPS.md). Installed-app runs on disposable macOS, Linux and Windows hosts, two rounds of adversarial review, and real-model tasks are recorded there. The logged-in Codex trace and the macOS physical-typing confirmation remain open. On 2026-10-09 the user expanded the target to include the broader feature set. Historical non-goal labels below and in the original sections no longer exclude those features from the current backlog. App-level audio and the persistent JavaScript runtime now have partial acceptance; Messages, dictation and authenticated locked-session use remain open. Existing partial features (including plain-text-only paste, point scroll unbound on macOS and Windows, and unbindable transient surfaces) remain identified; no new feature expansion is implied by a [BUILT] annotation.
