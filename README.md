# invisible-computer-use

A focus-free computer-use layer over [cua-driver](https://github.com/trycua/cua), recreating the
Codex/Sky computer-use feature delta as one installable component you can bundle into a project.
It observes and drives real applications through the accessibility tree without stealing focus,
adds the pieces Codex/Sky ship on top of a raw driver, and puts them behind a single API, a single
MCP server, and a single build-and-install command.

Current signed-package evidence is tracked in the [round-thirteen audit](study/audit/parity-round13-2026-10-09/README.md).
[Round fourteen](study/audit/parity-round14-2026-10-10/README.md) verifies native Windows app-activity history through capture, pause/resume, review and deletion on a signed Windows 11 package.
The earlier [round-twelve audit](study/audit/parity-round12-2026-10-09/README.md) retains the reviewed-memory regressions.
Reviewed history can now produce untrusted app memory, with explicit value sharing,
truncation markers, growing-file review and real-model recall checks.
It adds separately approved native Mac control labels, text values and normal
Chrome page context, with secure-field, domain, private-window and lock checks.
Cross-platform action and audio evidence
remains in the [round-four audit](study/audit/parity-round4-2026-10-09/README.md).
The Windows candidate in that audit passes image-driven actions, audio GUI capture,
native JavaScript persistence and permissions, and elevated input with negative
controls. Both native engines completed the same two tasks under one model and
controller. Mac notarization, browser download and system audio also have retained
evidence, with build boundaries stated in the audit. The broader feature target,
including Messages, dictation and authenticated locked-session use, remains open.
Full product equivalence is not established.

What the layer adds over the raw driver:

- **Stable element identity and AX-tree diffs** so the model sees what changed between turns, not
  the whole tree every time.
- **Semantic target recovery on macOS** for controls replaced after an observation. The patched
  driver revalidates the target and requires a unique match before and after recovery, while
  preserving exact-window checks. [Behavior and limits](impl/native/RECOVERY.md).
- **Per-app instruction packs** injected on first touch (Slack, Spotify, Clock, Numbers, Music,
  Notion, iPhone Mirroring), plus Spotify's runtime link-rewriting note.
- **The window2 observation surface** with the exact Codex tool names and argument shapes, and the
  accessibility text Sky renders, checked byte for byte against captures of the real service. The
  known gaps are listed in `conformance/golden/ax-diff-format.md`.
- **A confirmations and app-approval gate** in front of every input tool.
- **Skysight-style memory**: an ambient capture loop, 10-minute and 6-hour summaries, and phase-2
  consolidation into a durable profile.
- **Host readiness**: whether the screen is locked, the session is on the console, and the display
  is awake, so input is never delivered into a lock screen.
- **A readiness and capabilities call** with the typed error taxonomy, so onboarding and failures
  are one shape across the CLI, the MCP tool, and the library.

## Install

One command builds the workspace and the patched driver for the host, installs both, and prints the
MCP client config:

```
impl/install.sh                 # installs icu + the patched cua-driver into ~/.local/bin
impl/install.sh --prefix DIR    # install elsewhere
impl/install.sh --no-native     # skip the driver build; use a cua-driver already on PATH
impl/install.sh --linux-docker  # also build Linux binaries for a container
```

Several features need the PATCHED driver this repo builds, not a stock `CuaDriver.app`: the system
text-selection reader (focused, selected, and document text), native accessibility diffing with
stable ids, the generalized `perform_secondary_action`, transient-surface z-ordered capture (open
menus and popovers), the occlusion cursor clip, and host readiness. `impl/install.sh` installs that
driver and points the layer at it. If you run `--no-native` and use a stock driver on PATH, the
layer still runs its own diffing and packs, but those native-patch features are unavailable; run
`icu doctor` to see whether the running driver is the patched one.

On macOS the driver needs Accessibility and Screen Recording. Grant them once:

```
cua-driver permissions grant
```

For the fastest calls, run the driver as a daemon once per login and point the layer at its socket:

```
cua-driver serve --socket /tmp/icu.sock
export ICU_SOCKET=/tmp/icu.sock
```

## Use it from an MCP client

`icu mcp` is a stdio MCP server. Add it to any MCP client with the snippet `icu mcp-config` prints:

```json
{
  "mcpServers": {
    "invisible-computer-use": {
      "command": "/Users/you/.local/bin/icu",
      "args": ["mcp"],
      "env": { "ICU_DRIVER": "/Users/you/.local/bin/cua-driver" }
    }
  }
}
```

The server exposes the 13 window2 tools by their Codex names, plus `get_state`, `get_app`, the four `desktop_*` verbs, `prepare_action`, `commit_action`, `capabilities`, `verify_state`,
`cursor`, `record_start` / `record_stop` / `record_state`, `replay`, `events`, `host_state`, the
`memory_*` tools, `confirmations_policy`, `confirmation_decision`, and `doctor`.

When the client declares the elicitation capability, the first input tool on an app the user has not
approved raises Codex's "Allow Computer Use to use X" prompt, records the answer, and proceeds.

## The Sky pipe

`icu sky-pipe [--socket PATH]` serves the wire protocol of Codex's own client (`native-pipe.js`):
a unix socket (default `~/.icu/computeruse.sock`, or `SKY_CUA_SERVICE_NATIVE_PIPE_PATH`) carrying
JSON-RPC 2.0 messages with a 4-byte little-endian length prefix, a `ping {clientApiVersion}`
handshake (`CodexComputerUseIPC-5`; any other version is `-10013`), then `request` for
`ListApps`, `AppPolicy`, `AppStart`, `AppGetSkyshot` and `AppPerformAction`. The client's real code
runs against it in `conformance/test_sky_pipe.py`. It names an app, not a window: an app resolves
to its frontmost window, and the window an `AppGetSkyshot` read is where later actions go.
Screenshots are PNG files, coordinates are window-relative points, audio is refused, and the
socket is mode 0600 in a 0700 directory for the same user. On Windows it is a named pipe,
`\\.\pipe\icu-computeruse-<user>` (override with `SKY_CUA_SERVICE_NATIVE_PIPE_PATH` or `--socket NAME`), whose
DACL admits only the owner and SYSTEM.

The adapter refuses expired requests before dispatch and rechecks deadlines after input preparation
and settling. It cannot cancel an action already executing in the driver. If the observed window
closes or the app restarts, input requires a fresh `AppGetSkyshot`. Paste supports `format: "text"`
and selection supports `selection: "text"`; HTML, Markdown and cursor-placement modes return an
error. The [current runtime trace and gap assessment](specs/21-runtime-refresh-2026-10-06.md)
records the installed client versions and the tests against both old and current clients.

## Use it from the command line

Every tool is reachable one-shot through the same code path as the MCP server:

```
icu capabilities --text            # driver, permissions, host, features, typed errors, active mode
icu doctor                         # driver health, patch status, packs, paths
icu call list_windows
icu call get_window_state '{"window":{"app":"com.apple.clock","id":25427},"include_text":true}'
icu call click '{"window":{"app":"com.apple.clock","id":25427},"element_index":7}'
icu events --window 25427          # stream ax / focus / permission change events
icu memory serve --interval 5000   # ambient capture (app focus only, no titles or text) + 10min/6h summaries and consolidation
```

## Use it as a library

```rust
use icu::{Config, Layer};
use serde_json::json;

let layer = Layer::connect(Config::from_env())?;
let state = layer.dispatch("get_window_state", &json!({
    "window": {"app": "com.apple.clock", "id": 25427},
    "include_text": true,
}))?;
```

`Layer::readiness()` returns the same capabilities shape the `capabilities` tool serves.

## The tool surface

| Tool | What it does |
|------|--------------|
| `list_windows`, `get_window`, `list_apps`, `launch_app` | find and launch targetable windows and apps |
| `get_window_state` | capture a window: screenshot and/or the diffed accessibility tree with stable ids and the first-touch app pack |
| `click`, `press_key`, `type_text`, `scroll`, `set_value`, `drag`, `perform_secondary_action` | input, addressed by element id or coordinate, gated by app approval |
| `paste`, `select_text` | paste plain text through the clipboard (the prior clipboard is restored); select a run of text in an editable element by matching its content |
| `activate_window` | bring a window to the foreground and onto its Space |
| `capabilities`, `host_state`, `doctor` | readiness, host lock and console state, driver health |
| `verify_state` | check predicates against a window after an action; `{"file": ...}` and `{"process": ...}` predicates check state outside any window when the operator enables them |
| `get_state`, `get_app` | the unified `cua.getState()` / `cua.getApp(name)` facade: an inventory of every app and window in one call, and an app selected by name, bundle id or path with its window and initial state |
| `desktop_screenshot`, `desktop_click`, `desktop_drag`, `desktop_drag_handle`, `desktop_move`, `desktop_scroll`, `desktop_press_key`, `desktop_type_text` | the Linux full-desktop contract (Sky's `get_screenshot`, `click`, `drag` with a path of two or more points, `drag_handle`, `move`, `scroll`, `press_key`, `type_text`), in pixels of one whole-desktop screenshot with the agent cursor drawn in; no window argument. A click or key `duration`, a drag through more than two points, a drag handle and a held `key` need the native driver and are refused, not approximated, on a stock one. Contract: the MCP resource `icu://skills/linux-desktop` (`impl/icu/skills/linux-SKILL.md`) |
| `prepare_action`, `commit_action` | two-phase actions for irreversible steps: plan without performing, then commit once after the user approves (and may edit the text) |
| `cursor`, `record_start`, `record_stop`, `record_state`, `replay` | the agent cursor and trajectory recording |
| `events` | poll for accessibility, focus, and permission changes |
| `memory_summarize`, `memory_rollup`, `memory_consolidate`, `memory_list`, `memory_prior`, `memory_grep` | Skysight-style memory |
| `confirmations_policy`, `confirmation_decision` | the confirmations policy text and the decision for a category |

Input tools take a `delivery_mode`, an optional `deadline_ms`, an optional `verify` list of
predicates run after the action, and an optional `settle` that waits until the tree stops changing.
When `delivery_mode` is omitted, `attended` selects foreground delivery; `headless` and `replay`
select background delivery. The default mode is `attended`. Set `ICU_MODE=headless` when foreground
delivery must be refused.

## Configuration

All configuration is environment variables, read by `Config::from_env`:

| Variable | Meaning |
|----------|---------|
| `ICU_DRIVER` | the cua-driver binary (default: `cua-driver` on PATH) |
| `ICU_SOCKET` | a running daemon socket to proxy through |
| `ICU_DRIVER_DAEMON` | macOS without `ICU_SOCKET`: `0` talks to `<driver> mcp` over stdio instead of starting `<driver> serve` (for drivers with no daemon mode) |
| `ICU_HOME` | base for state and approvals (default `~/.icu`) |
| `ICU_MEMORY_ROOT` | the Skysight resources folder (default `~/.icu/memory`, isolated from the real `~/.codex`); on macOS and Linux it is kept 0700 and the files memory writes 0600 |
| `ICU_MEMORY_RETENTION_DAYS` | days to keep raw ambient event records before capture drops them: a whole number from 1 to 3650 (default 7), anything else is an error; summaries are kept |
| `ICU_MODE` | `attended`, `headless`, or `replay` |
| `ICU_REQUIRE_APP_APPROVAL` | set to `0` to skip the per-app approval gate |
| `ICU_DENIED_APPS`, `ICU_FORBIDDEN_APPS` | bundle ids refused by policy |
| `ICU_ALLOW_ALWAYS` | offer persistent (always) approvals |
| `ICU_DEADLINE_MS` | advisory action deadline |
| `ICU_DRIVER_TIMEOUT_MS` | hard transport exchange budget, default 15000 ms; initialization/health/discovery capped at 1500 ms |
| `ICU_MAX_VALUE_CHARS` | per-value budget in characters for `get_window_state` (default 4096, `0` returns whole values); a longer value is clipped and described by `{available_range, count}` in `_value_truncations` |
| `ICU_MOUSE_SIZE_PX` | draw the agent cursor into returned screenshots at the last pointer action, this many pixels tall (default 12, `0` disables); only while the agent cursor is on |
| `ICU_POST_ACTION_SLEEP_MS` | minimum gap after a successful input before the next action or screenshot (default 100, `0` disables) |
| `ICU_CLOSE_ENOUGH_PX`, `ICU_CLOSE_ENOUGH_PROGRESS` | end the agent cursor's glide early within this many points of the target, or after this fraction (0 to 1) of the path; off by default; needs the patched driver |
| `ICU_PERSISTENT_PREDICATES`, `ICU_PREDICATE_ROOTS` | allow `verify_state` file and process predicates (off by default); a path list that confines the file paths they may read |
| `ICU_RUN_METADATA` | a JSON object (`model`, `prompt`, `run_id`, `eval`, ...) stamped into every turn of recordings `record_start` begins; a `run` argument overrides it key by key |
| `ICU_SUMMARIZER_BASE_URL` | back memory summaries with any OpenAI-compatible endpoint (a local model, a proxy, or OpenAI); a keyless local model needs no key |
| `ICU_SUMMARIZER_MODEL`, `ICU_SUMMARIZER_API_KEY` | the model name and optional key for the summarizer endpoint |
| `ANTHROPIC_API_KEY` | one summarizer option: when set with no `ICU_SUMMARIZER_BASE_URL`, summaries use the Anthropic model; otherwise the keyless reference summarizer runs |

## Modes

A mode presets the real knobs: `attended` allows foreground delivery and shows the agent cursor,
`headless` is focus-free only and refuses a foreground request, `replay` turns recording on.
`icu capabilities` reports the active mode and which modes are available right now.

## Performance

Use one persistent `icu mcp` process, not `icu call` per action: a one-shot call spawns a fresh
driver connection and re-runs setup (about four seconds of overhead), while a persistent session
pays that once. Through a persistent session, a click or keystroke on a real app is about 1.1 to
1.3 seconds.

Each one-shot `icu call` is also its own driver session, and the driver reads a pixel sent
without a screenshot id only against the calling session's own capture of that window. So a
coordinate `click`, `scroll` or `drag` through `icu call` has to carry the `screenshotId` from the
`get_window_state` it was read from; without it the driver refuses `ambiguous_capture`. In one
`icu mcp` session the latest capture is used, and element ids need no capture at all.

Two knobs affect per-action latency:
- **The agent cursor.** In `attended` mode the visible ghost cursor uses detached rendering on
  every macOS action path. Input delivery and focus restoration still complete before the action
  returns. Historical Calculator pixel-click medians were 3.04s attached, 1.54s detached and 1.53s
  cursor-off; six samples without raw timing data do not establish equivalence. The expanded
  cursor fix has inspected visibility and completed-action evidence in the owned fixture run113414Z.
  Its three-sample AX medians were1.384s attached,1.183s detached and1.433s off; these do not
  establish broad timing equivalence.
  `headless` and `ICU_CURSOR=off` disable rendering when explicitly selected.
- **The background path.** Driver-side settling contributes to observed latency. The retained
  measurements do not establish an irreducible floor. [Audit fix evidence](study/audit/FIX-STATUS.md)
  separates historical measurements from the corrected bounded desktop acceptance. Final
  physical typing confirmation and a relocked-host test recheck remain pending; no further
  tested input is planned. [Current raw evidence](study/audit/finish-acceptance/README.md).

The layer caches each process's app identity, so the approval gate does not re-list apps on every
action.

## Limits

- **Golden capture**: a no-login probe of the real `SkyComputerUseService` captured its tree, diff
  and error text, and the layer renders that format byte for byte on the captured turns. A
  logged-in Codex trace, which would show what the model sees around it, is not recorded yet.
- **The memory model call** is a pluggable backend. The deterministic reference summarizer is the
  keyless default. Set `ICU_SUMMARIZER_BASE_URL` to back it with any OpenAI-compatible model (a local
  model on the host or cloud box needs no key), or `ANTHROPIC_API_KEY` to use Anthropic. Anthropic is
  one option, not required.
- **The occlusion cursor clip** ships on macOS only. The Windows and Linux clip was retired
  (build plan slice 13(c)): on those platforms the agent cursor can draw over a window that covers
  the target.
- **The live picture-in-picture mirror** (Codex RemoteHostedPIP) is not recreated; trajectory
  recording is the exposed capture surface.
- **Linux lock detection** covers D-Bus screen-saver services, logind, and the X11 locker
  signature; a locker with no D-Bus presence on Wayland is reported as unknown, not asserted.
- **Windows display sleep and lid** are reported only after the first power-setting notification.
- **Windows keyboard input** to some apps (e.g. classic Notepad, whose edit exposes TextPattern not ValuePattern) needs the driver's UIAccess-manifested worker for background delivery; observation and the tool surface work without it. Background chords to ordinary Win32 windows are refused rather than risk typing their last key.
- **Pixel coordinates** can be bound to the capture they came from: `get_window_state` returns a
  `screenshot_id`, and `click`, `double_click`, `right_click` and `drag` accept it and refuse
  `stale_capture` if the window changed size since. Without it, the pixel is read against the calling
  session's own latest capture of that window (none refuses `ambiguous_capture`). `scroll` at a point
  accepts it on macOS, Linux and Windows. Surfaces in `screenshots[]` cannot be bound.
- **Sway capture** is refused (`surface_identity_unproven`) whenever another Wayland client is
  connected, because sway 1.9 cannot report where layer surfaces sit. On a desktop with a wallpaper
  process, a bar or a second app, window capture on sway does not work. By default no compositor's
  native Wayland windows are listed, only XWayland ones; with the backend enabled, mutter still
  refuses capture.
- **macOS system actions**: the Apple menu is left out of observations and refused by `invoke_menu`,
  and session chords (Ctrl-Cmd-Q, log out, Force Quit, power) are refused in every delivery mode.
- **WPF on Windows**: pixel clicks into a WPF window without UI Automation Invoke currently do
  nothing while reporting success; a fix is in progress. Only WinForms targets were tested.

## Verify

```
impl/verify.sh            # workspace tests, then conformance for the layer, raw cua, and the patched driver
impl/verify.sh --native   # also rebuild the patched driver and run its unit tests
impl/verify.sh --live     # also the live checks (needs a resolvable window and, for native, a daemon)
```

The build plan and per-slice evidence are in `impl/BUILD-PLAN.md`.
