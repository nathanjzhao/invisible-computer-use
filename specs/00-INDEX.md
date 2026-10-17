# Invisible Computer Use — Spec & Comparison Set

Codex/Sky (what we recreate) vs cua-driver 0.24.0 (what we build on). 19 sections, each grounded in files read on this machine with exact paths and symbols. Trees live at `~/dev/invisible-computer-use` (`codex-corpus/`, `vendor-cua/`).

The [round-four scope](../study/audit/parity-round4-2026-10-09/SCOPE.md) now includes
the previously excluded broader product features. Their acceptance is tracked in
the [current audit](../study/audit/parity-round4-2026-10-09/README.md).

The coverage and platform-availability statements below describe the original
September snapshot. [Section 21: current runtime trace](21-runtime-refresh-2026-10-06.md)
records the October 6 installed versions, a current native disassembly, four adapter
fixes, and remaining gaps. Historical code verification is not full behavioral
parity with the current app; Linux availability outside this macOS bundle remains
unverified in the refresh.

Cross-OS live measurement: `00-CROSS-OS-MEASURED.md` runs the same three-action matrix (click, set_value, type_text) live against a receiver-logging fixture on macOS, Windows, and Linux, so the "which actions are invisible" claims rest on real oracle-verified runs on all three OSes, not source-reading. It pins the one genuine behavioral difference (Windows `type_text` uses SendInput and escalates to foreground; macOS and Linux use a semantic text-insertion API and stay background).

Completeness: sections 01-13 were the first pass; a coverage audit (`00-COVERAGE.md`, 129-row matrix) then found ~18 Codex features with no section plus two factual errors. Sections 14-19 close the genuine gaps; the errors are fixed. Remaining uncovered Codex features are deliberate non-goals for a generic driver, listed at the bottom.

Evidence parity: every macOS subsystem is code-verified against the shipped Codex binaries. Windows (section 09) is now also CODE-VERIFIED, from a teardown of the Store-signed MSIX (`codex-app-prod/ChatGPT-x64.msix`): the native engine is a Rust `codex-computer-use.exe` importing SendInput, `uiautomationcore.dll`, and Windows.Graphics.Capture, confirming the active-desktop model, a Windows ghost cursor, and lock-screen detection. Windows is further LIVE-MEASURED: cua-driver 0.24.1 was run on an EC2 Windows Server 2022 box against an occluded WinForms fixture (section 09 part 6, `win-eval/parity-scoreboard.md`), proving UIA InvokePattern/ValuePattern deliver focus-free to an unfocused window while raw SendInput self-reports `delivery_failed` and escalates to foreground, exactly Sky's active-desktop model, with the ghost-cursor overlay window live-confirmed. The newer Windows plugin corpus (including `confirmations.md`) is in `codex-corpus/windows/`. Linux (section 10) is resolved as far as it exists: native Computer Use does not ship on the Linux app at all yet. OpenAI shipped the ChatGPT/Codex Linux desktop app in preview on 2026-08-11, but its launch explicitly excludes native Computer Use, Appshots, and Record and Replay, so there is no Linux native engine to code-verify (only the `sky_linux.js` marshaller ships; no `bin/linux/` binary in any obtainable build). That is a confirmed fact, not a gap in our understanding. All three OSes are therefore refined to completion: macOS and Windows code-verified against real binaries, Linux understood as far as OpenAI has shipped it, with cua-driver's Linux stack fully code-verified from source. The cua-driver side of all 19 sections is code-verified from vendored source. Section 20 adds a Ghidra decompile pass on the macOS service: it confirms the render-diff machinery, the monotonic element IDs, refetch/invalidation, and the screenshot classifier at the code level, and shows the behavior is heavily feature-flagged (a large SlimCore flag surface, 16 computer-use-relevant flags archived in `feature_flags.txt`, plus a pervasive Statsig client, ~214 string hits), so several of the INFERRED constants are dynamic experiment values rather than fixed numbers.

## Sections

| # | Subsystem | Verdict | Effort |
|---|-----------|---------|--------|
| 01 | [Observation model: indexed AX tree + screenshot](01-observation-model.md) | Both pair a per-element-indexed AX tree with a screenshot and cua is structurally ahead (opaque tokens, strict window scoping, structured elements array), but Codex parity needs two things cua lacks entirely: a monoto... | **native patch** |
| 02 | [AX-tree diffing between turns](02-ax-diffing.md) | Sky ships a real cross-turn AX diff subsystem (tree-revision lineage + structural diff + line-budget fallback + removed-ID-range summary, gated by disableDiff); cua has none and returns a full tree every turn, so recr... | **large build** |
| 03 | [Long-horizon memory: Skysight vs cua computer-history](03-memory-skysight-vs-history.md) | Near-opposite designs: Skysight is an ambient, content-rich, LLM-summarized memory of everything the user does; cua computer-history is a narrow, encrypted, metadata-only ledger of only what the driver itself did, wit... | **large build** |
| 04 | [Per-app instruction packs](04-per-app-instructions.md) | Sky ships seven bundle-id-keyed playbooks that the native service returns as appSpecificInstructions and the JS prefixes once per app per session; cua has no such layer, so recreate it by copying the seven .md files v... | **native patch** |
| 05 | [Background input ladder, focus preservation, off-Space](05-background-input-focus.md) | cua already ships a stricter, testable version of Codex's focus machinery, including a literal Rust port of SystemFocusStealPreventer; the one real gap is an opt-in off-Space "gather" rung, which is a native patch add... | **native patch** |
| 06 | [Input actuation: AX actions vs pixel](06-input-actuation.md) | Both systems run the same two-rung model (semantic AX action primary, background CGEvent/SkyLight pixel fallback); cua-driver ships a more complete, harder-verified actuator set and is missing only Codex's dedicated s... | **native patch** |
| 07 | [Capture: per-window occluded + transient surfaces](07-capture.md) | cua does per-window occluded capture well (SCContentFilter + px_frame proof) but returns ONE surface; Sky's window2 returns a z-ordered array covering the window plus transient UI. cua already has every primitive (lay... | **native patch** |
| 08 | [Ghost cursor overlay](08-cursor-overlay.md) | cua's ghost cursor is more capable than Codex on theming and motion and already ships the whole subsystem cross-platform; the one Codex behavior worth porting is the always-on-top overlay-level plus clip-to-visible-re... | **native patch** |
| 09 | [Windows mechanism: UIA + SendInput + WGC + PiP](09-windows-mechanism.md) | cua already ships UIA + SendInput + WGC occluded capture and goes past Sky with a background/no-foreground rung; Sky's Windows model is active-desktop (activate then SendInput), so recreating Codex behavior is a thin ... | **thin port** |
| 10 | [Linux mechanism: AT-SPI + Wayland + Hyprland/KWin + libei](10-linux-mechanism.md) | cua-driver's Linux stack is far deeper than Sky's single full-desktop binary, so recreating Codex means exposing a thin desktop-coordinate facade over cua primitives, not building Linux input. | **thin port** |
| 11 | [Permission modes, capability manifests, session lifecycle, locked screen](11-permissions-sessions.md) | cua-driver's authorization model is materially ahead of Codex on modes, manifests, consent, and revocation; the only Codex pieces worth porting are the per-bundle-id app allow/deny/forbid gate and the TCC-adjacent ent... | **native patch** |
| 12 | [Verification (verify_state) and trajectory recording/replay](12-verification-recording.md) | cua-driver already IS the verifier foundation (verify_state tri-state predicates + reserve/dispatch/write_turn trajectory recording + replay_trajectory); Codex/Sky ships neither, so the OSWorld board is a native cua e... | **native patch** |
| 13 | [Transport, tool surface, and browser CDP path](13-protocol-tools-browser.md) | Both speak JSON-RPC 2.0 to a native service over a unix socket, but they diverge everywhere it matters: cua uses newline-delimited MCP with a generic tools/list registry and a real loopback-CDP browser attach, while S... | **native patch** |
| 14 | [Action-approval: Confirmations Policy, elicitation, on-disk allowlists](14-approval-confirmations.md) | cua has a strong daemon-side capability/risk/consent skeleton but no equivalent of Codex's side-effect Confirmations Policy nor its runtime per-app elicitation with disk-persisted session/always allowlists; the policy... | **thin port** |
| 15 | [Native app tools: Messages/iMessage two-phase send](15-native-app-tools.md) | Codex ships a full native Messages MCP server (5 read tools over chat.db SQLite + a two-phase PrepareSend/CommitSend send with a user-editable approval store); cua-driver has none of this and none of it belongs in a g... | **non-goal** |
| 16 | [Audio and media capture](16-audio-media-capture.md) | Codex ships a gated, high-risk-approved computer-audio recorder (native Swift on mac, ffmpeg+PulseAudio on linux); cua-driver has rich video/action-trajectory recording and an ffmpeg installer but zero audio, only a d... | **non-goal** |
| 17 | [Takeover UX and live view: Appshot, SkyLens, Remote PiP, multi-cursor](17-takeover-ux-liveview.md) | Codex is genuinely ahead on takeover UX: it ships an Appshot magic-move capture transition, a 45-frame animated SkyLens glow, and a live ScreenCaptureKit window-mirror PiP with in-PiP cursor (269 symbols); cua-driver ... | **large build** |
| 18 | [Broad TCC management and consent UI](18-broad-tcc-consent-ui.md) | Codex gates 9 TCC scopes behind a PermissionCatalog window plus a SystemSettings accessory overlay that physically points at the toggle; cua-driver gates exactly 2 (Accessibility + Screen Recording) with a plain NSPan... | **native patch** |
| 20 | [Native decompile findings + feature-flag surface](20-native-decompile-findings.md) | Ghidra decompile confirms the monotonic element ID, the render-diff machinery, refetch/invalidation, and the flag-gated screenshot classifier; the headline is that CU behavior is pervasively SlimCore-feature-flagged, so many "constants" are dynamic experiment values, not fixed. | **reference** |
| 19 | [Observation, Errors, Skysight, IPC Auth Addendum](19-observation-error-skysight-addendum.md) | Codex ships a much larger native surface here: a first-class current-selection reader, per-value truncation ranges, refetch/invalidation, a screenshot-need classifier, private-WindowServer occluded capture with corner... | **native patch** |

## Build order by effort

### thin port
- **Windows mechanism: UIA + SendInput + WGC + PiP** — 09-windows-mechanism.md
- **Linux mechanism: AT-SPI + Wayland + Hyprland/KWin + libei** — 10-linux-mechanism.md
- **Action-approval: Confirmations Policy, elicitation, on-disk allowlists** — 14-approval-confirmations.md

### native patch
- **Observation model: indexed AX tree + screenshot** — 01-observation-model.md
- **Per-app instruction packs** — 04-per-app-instructions.md
- **Background input ladder, focus preservation, off-Space** — 05-background-input-focus.md
- **Input actuation: AX actions vs pixel** — 06-input-actuation.md
- **Capture: per-window occluded + transient surfaces** — 07-capture.md
- **Ghost cursor overlay** — 08-cursor-overlay.md
- **Permission modes, capability manifests, session lifecycle, locked screen** — 11-permissions-sessions.md
- **Verification (verify_state) and trajectory recording/replay** — 12-verification-recording.md
- **Transport, tool surface, and browser CDP path** — 13-protocol-tools-browser.md
- **Broad TCC management and consent UI** — 18-broad-tcc-consent-ui.md
- **Observation, Errors, Skysight, IPC Auth Addendum** — 19-observation-error-skysight-addendum.md

### large build
- **AX-tree diffing between turns** — 02-ax-diffing.md
- **Long-horizon memory: Skysight vs cua computer-history** — 03-memory-skysight-vs-history.md
- **Takeover UX and live view: Appshot, SkyLens, Remote PiP, multi-cursor** — 17-takeover-ux-liveview.md

### non-goal
- **Native app tools: Messages/iMessage two-phase send** — 15-native-app-tools.md
- **Audio and media capture** — 16-audio-media-capture.md

## Deliberate non-goals (not building, marked for completeness)

- Locked-screen auth plugin (SecurityAgentPlugin) — operate a locked Mac
- Native Messages/iMessage tools + chat.db — app-specific, not generic driver surface
- Broad app TCC (Contacts, Calendar, Reminders, Location, Mail AppleEvents) — for Codex's native app tools
- Statsig remote feature gating — Codex infra, not a capability
- Codex menu-bar status item and signed build flavors — host app shell
- Writes Codex's own MCP-server config TOML — Codex-specific packaging

See `00-COVERAGE.md` for the full feature-by-feature matrix and the completeness verdict.
