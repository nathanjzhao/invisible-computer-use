## Per-app instruction packs

### What it is

Sky ships app-specific playbooks, one Markdown file per app, that are injected into the accessibility text the first time an agent touches that app in a session. Each pack encodes hard-won quirks of a single app's UI so the model does not have to rediscover them: which key sends a Slack message, how Numbers cells commit, that Spotify is network-backed and needs a re-poll. cua-driver has no equivalent. It ships generic per-platform skill packs only, and `get_window_state` returns a raw AX tree with no app-keyed prefix.

### Codex/Sky

The content lives in seven Markdown files that ship verbatim inside the service app bundle and are byte-identical to the corpus copies. This is reproducible from the archive: `cmp -s` on all seven (`Package_ComputerUse.bundle/Contents/Resources/AppInstructions/*.md` in the live ChatGPT.app vs `codex-corpus/mac/ComputerUse/AppInstructions/*.md`) reports IDENTICAL for every pack; the shipped-copy sha256sums are recorded in `reference/openai-sky/app-bundle-packs/SHA256SUMS.txt`. The rules each encodes:

- **Slack.md** — the composer sends on focus + `Return`, so use `set_value(...)` not `type_text(...)` to avoid sending a half-typed or multiline message; `\n` behaves differently between the two calls; the send-vs-newline key is user-configurable and readable from a composer hint. Rule: never let a typing action fire an accidental send.
- **Notion.md** — the block model: `Return` semantics per block type, how `cmd+a` escalates from block to document, placeholder text you must type over rather than delete, inverted `Return`/`shift+Return` inside code vs quote blocks. Rule: drive Notion through its block/keyboard grammar, not raw text entry.
- **Spotify.md** — the app is not fully local state, so a click result can show stale/paused playback; re-run `get-state` instead of re-clicking, and never sleep. Rule: poll, do not double-act, on a network-backed app.
- **AppleMusic.md** — search via the sidebar Search row + `set-value` (auto-submits); `filterField` filters the current view and is not catalog search; double-click to play; queue via More > Play Next/Last. Rule: disambiguate the two search surfaces and the play/queue affordances.
- **Numbers.md** — one click to append to a cell, three clicks to replace; return a click plus a keyboard call in parallel; one row per `type_text` with `\t` delimiters but never multiple rows or formulas; cells save without `Return`; checkbox cells take `0`/`1`. Rule: batch-safe spreadsheet entry.
- **Clock.md** — a procedural task list (world clock, timer, stopwatch, alarms) with explicit state-checks, the `TimePicker` container's three sliders, and bounds rejection over 23:59:59. Rule: a deterministic UI script for a fiddly stock app.
- **iPhone Mirroring.md** — the `⌘1/⌘2/⌘3` shortcuts, use `scroll` not `drag`, click icon centers not labels. Rule: the mirrored-iPhone input model differs from a native window.

The injection is a native-produces / JS-wraps split, confirmed by reading the shipped code:

- `.../Codex Computer Use.app/Contents/Resources/Package_ComputerUse.bundle/Contents/Resources/AppInstructions/{Slack,Notion,Spotify,AppleMusic,Numbers,Clock,iPhone Mirroring}.md` — the shipped packs.
- `SkyComputerUseService` (native binary) — `strings` shows `appSpecificInstructions`, the `<app_specific_instructions>` / `</app_specific_instructions>` tag pair, the app bundle ids `com.tinyspeck.slackmacgap`, `com.spotify.client`, `com.apple.ScreenContinuity`, and a *dynamic* supplemental Spotify note ("Spotify app links must be rewritten as regular links...") that is not in the .md. So the native service resolves the app to its pack, may append runtime text, and returns it in the skyshot field `appSpecificInstructions`.
- `.../sky_js/src/targets/mac/window_result.js` — function `r()` reads `t.appSpecificInstructions`, and if present and non-empty returns `` `<app_specific_instructions>\n${u}\n</app_specific_instructions>\n${s}` `` (instructions prefixed before the AX text `s`). Dedup key `p` is the app's `bundleIdentifier`, falling back to app name then identifier. `o.has(p)` -> skip (already injected); else `o.add(p)`. A module-level `const e = new Set(["com.apple.iWork.Numbers"])` excludes Numbers from this JS wrapper.
- `.../sky_js/src/targets/mac/create_client.js` — `const u = new Set` is created once per client, i.e. the first-touch tracker is per-session; `get_app_state(e)` forwards it as `i(e, u)`.
- `.../sky_js/src/targets/mac/get_app_state.js` — calls the native `getAppState({app, disableDiff})` then `window_result(app, skyshot, seenSet)`, so the prefix is applied on the state-read path only.
- `.../docs/sky-window-api.md` line 43 and `.../types/window/AppState.d.ts` (line 8) — document the contract: `text` is "Accessibility text, prefixed with app-specific guidance on first access when available."
- `.../skills/macos-SKILL.md` — the workflow doc tells the agent to call `get_app_state` first and re-read after actions, which is what triggers the first-touch prefix; the doc does not itself carry per-app rules.

INFERRED: Numbers is excluded from the JS wrapper because the native service injects its guidance inline into `text` on every snapshot rather than once, or suppresses double-wrapping; the reason is not stated in the JS. The full bundle-id-to-file map is resolved inside the native binary and is not cleanly extractable from the minified strings; only Slack, Spotify, iPhone Mirroring (and Numbers via the JS exclusion) are confirmed by literal bundle id.

### cua-driver

cua has **no per-app instruction layer and no first-touch injection**. Nearest modules:

- `rust/crates/platform-macos/src/tools/get_window_state.rs` — `GetWindowStateTool::invoke` builds the response summary as `format!("window_id={window_id} pid={pid} size={w}x{h} elements={element_count}\n\n{tree_markdown}")` (around lines 505-513) and a `structured` JSON blob. There is no hook that prefixes app guidance. It already resolves the app: `chromium_browser_window(pid)` (line 143) calls `crate::apps::get_app_name_for_pid` and `crate::apps::bundle_id_for_pid`, proving bundle-id resolution is available at exactly the point a pack would be injected.
- `rust/crates/platform-macos/src/apps/mod.rs` — `pub fn bundle_id_for_pid(pid: i32) -> Option<String>` (line 664) and `pub fn get_app_name_for_pid` (line 676). This is the key the pack map would use.
- `rust/crates/cua-driver-core/src/session.rs` — session state is a set of `OnceLock<Mutex<HashMap<..>>>` keyed by session id (`String`) or session `u64` (e.g. `SESSION_ACTIVITY`, `LIFECYCLE_RECORDS` at lines 337/355). This is where a per-session `HashSet<String>` of already-injected bundle ids belongs, mirroring Sky's per-client `const u = new Set`.
- `rust/Skills/cua-driver/MACOS.md` — the only app-shaped guidance is generic, not injected: lines 371-382 warn about Catalyst/Electron apps (naming Slack's composer, Discord, Linear) echoing AX writes, handled by the driver refusing to trust the echo. This is a driver behavior note, not a bundle-id-keyed playbook.
- `rust/Skills/cua-driver/SKILL.md`, `README.md` — describe the snapshot-action-verify loop and reading order; no `app_specific`, `per-app`, or `playbook` concept (grep returns nothing).

### Delta

| Aspect | Sky | cua-driver |
| --- | --- | --- |
| Per-app playbooks | 7 shipped `.md` packs | none |
| Injection point | native `appSpecificInstructions` -> JS prefix on AX text | none; `get_window_state` summary only |
| First-touch dedup | per-client `Set` keyed by bundle id | none; session HashMaps exist but unused for this |
| Dynamic supplemental text | yes (e.g. Spotify link rewriting) | none |
| Bundle-id resolution | native map | present (`bundle_id_for_pid`) but not wired to any pack |

Where cua is ahead: nothing specific to this subsystem; cua's `get_window_state` returns a richer structured `elements` array, so an injected pack could reference structured fields, not just markdown.

Where Codex is ahead: the entire feature. Sky front-loads app quirks so the model avoids known failure modes (accidental Slack send, stale Spotify state, Numbers batch limits) without trial and error.

### Recreate/port plan

1. **Copy the packs verbatim.** Copy the seven files from the corpus (`codex-corpus/mac/ComputerUse/AppInstructions/*.md`) into a cua resource dir, e.g. `rust/crates/platform-macos/src/tools/app_instructions/`, and embed them with `include_str!`. copy-verbatim.
2. **Add a bundle-id -> pack map.** New module (e.g. `app_instructions.rs`) with a static table. Confirmed keys: `com.tinyspeck.slackmacgap` (Slack), `com.spotify.client` (Spotify), `com.apple.ScreenContinuity` (iPhone Mirroring), `com.apple.iWork.Numbers` (Numbers). Fill the rest with standard bundle ids and verify each at runtime via `bundle_id_for_pid`: Apple Music `com.apple.Music`, Clock `com.apple.clock`, Notion `notion.id`. native-patch.
3. **Add a per-session first-touch set.** In `rust/crates/cua-driver-core/src/session.rs`, add a session-keyed `HashSet<String>` of already-injected bundle ids alongside the existing session HashMaps, with an `insert_if_absent`-style accessor that returns whether this is the first touch. native-patch.
4. **Wire the injection in `get_window_state.rs`.** After building `summary` (lines 505-513), resolve `bundle_id_for_pid(pid)`, look up the pack, and if the session set reports first touch, prefix `<app_specific_instructions>\n{pack}\n</app_specific_instructions>\n` onto the text content. Match Sky's tag exactly for parity. Decide the Numbers policy (inject-every-time or first-touch) explicitly. native-patch.
5. **Optional dynamic supplements.** Sky appends runtime text (Spotify link rewriting). Port later as a per-app hook that can append to the static pack; not required for baseline parity. thin-port.

Overall this is a native-patch: a small Rust change (map + session field + one prefix at a known line) plus a verbatim file copy. No large redesign; the resolution primitives already exist in the driver.
