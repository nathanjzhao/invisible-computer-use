**As of September 8, 2026.** “RE” means a third-party reverse-engineering finding, not OpenAI confirmation.

**1. macOS mechanisms**

- **Confirmed:** Codex operates Mac apps in the background with independent cursors; multiple agents can work without interrupting foreground work. [Announcement](https://openai.com/index/codex-for-almost-everything/)
- **RE:** Native Swift `SkyComputerUseClient`/`SkyComputerUseService`; screenshots through ScreenCaptureKit, structured controls through Accessibility, input through `CGEvent` synthesis or direct `AXPress`. Symbols include `SystemFocusStealPreventer` and `SyntheticAppFocusEnforcer`. [July teardown](https://blog.leeguoo.com/en/posts/codex-computer-use-teardown/)
- **Inferred, not established:** BetterTouchTool’s developer explains background AX actions and combining `CGEventPostToPid` with `CGEventTapCreateForPid`; this describes feasible techniques, **not verified Codex internals**. Exact target-window event fields and virtual-display use remain **unknown**. [HN technical discussion](https://news.ycombinator.com/item?id=47809186)

**2. Occluded, minimized, other Spaces/displays**

- **Confirmed:** Background operation is supported; official documentation does not guarantee every occlusion/minimization/display configuration. **Inferred:** window-specific capture plus AX makes ordinary occlusion plausible; minimized-window operation remains **unknown**. [Docs](https://developers.openai.com/codex/app/computer-use)
- **Reported failure:** Another-Space targeting captured the active Space or unrelated windows in `26.506.21252`, macOS Tahoe 26.4.1. Cross-Space reliability therefore cannot be assumed; separate-display behavior remains **unknown**. [Issue #21668](https://github.com/openai/codex/issues/21668)

**3. Cursor/overlay UX**

- **Confirmed:** OpenAI’s live demonstration explicitly calls it a “ghost cursor.” [Official webinar](https://webinar.openai.com/on-demand/f4d5175f-233a-44f8-af6d-a7170dcf484c)
- **RE:** Pre-capture highlighting uses `CaptureAnimation` messages. [Teardown](https://blog.leeguoo.com/en/posts/codex-computer-use-teardown/)
- **Unknown whether optional today:** A request to hide/dim the transparent cursor was closed “not planned”; this does **not** establish a supported visibility toggle. [Issue #21303](https://github.com/openai/codex/issues/21303)

**4. Permissions**

- **Confirmed:** macOS TCC **Screen Recording + Accessibility** permissions for “Codex Computer Use,” plus separate per-app approvals and revocable **Always allow** entries. [Docs](https://developers.openai.com/codex/app/computer-use)
- **Confirmed:** Administrators can default-deny and allow specific bundle IDs, restrict saved approvals, or disable Computer Use. [Help Center](https://help.openai.com/en/articles/20001510-manage-browser-and-computer-use-in-your-enterprise-workspace)
- **Confirmed repository evidence:** `codex-rs/core/config.schema.json` contains `features.computer_use` and platform-specific app policies; these establish configuration, not the native input implementation. [Source](https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json)

**5. Windows/Linux**

- **Confirmed:** Windows uses the **active, visible, unlocked desktop**, moving the real pointer and taking foreground input. A Windows VM is the documented isolation option; exact native input APIs are **unknown**. [Docs](https://developers.openai.com/codex/app/computer-use)
- **Engineer statement:** Linux desktop preview lacks native-app Computer Use; browser control works, native support “coming soon.” No Linux implementation disclosed. [James Sun’s X post](https://x.com/JamesZmSun/status/2087251350694592920) ([readable mirror](https://x.noodl3.net/JamesZmSun/status/2087251350694592920))

**6. Model-facing actions**

- **RE, version-specific:** `list_apps`; `get_app_state(app)` → screenshot + indexed AX tree; `click(elementIndex | x,y, button, clicks)`; `set_value(elementIndex,value)`; `perform_secondary_action`; `scroll(elementIndex,direction,pages)`; `drag(startX,startY,endX,endY)`; `press_key(key)`; `type_text(text)`. [Extracted schema, helper 1.0.780](https://github.com/huyansheng3/codex-analysis/blob/main/docs/codex-computer-use-implementation.md)
- **RE:** July’s helper reports ten tools and incremental AX-tree diffs; parameter spelling differs (`element_index`). Treat extracted schemas as snapshots. [July teardown](https://blog.leeguoo.com/en/posts/codex-computer-use-teardown/)

**7. Limitations**

- **Confirmed:** Terminal apps, the host ChatGPT app, administrator authentication, and security/privacy approval dialogs are excluded. [Docs](https://developers.openai.com/codex/app/computer-use)
- **RE:** Chromium/Electron accessibility is enabled with `AXManualAccessibility`; Electron is not categorically unsupported. General game compatibility remains **unknown**. [Teardown](https://blog.leeguoo.com/en/posts/codex-computer-use-teardown/)
- **Reports:** Chrome wrong-window screenshots; Windows Danea Easyfatt’s visible VCL window not enumerated. [Chrome](https://github.com/openai/codex/issues/24301), [Easyfatt](https://github.com/openai/codex/issues/27874)
- **Reddit anecdote:** Chrome extension connected but unusable; this concerns Browser Use and does not establish native input failure. [Thread](https://www.reddit.com/r/codex/comments/1twccpt/anyone_uses_chrome_with_codex_on_windows/)

**8. Releases**

- **Confirmed:** macOS Computer Use: **April 16, 2026, app 26.415**. Windows: **May 29, 2026, app 26.527**. Linux desktop preview: **August 11, 2026**, distinct from native Computer Use availability. [Official changelog](https://developers.openai.com/codex/changelog/)