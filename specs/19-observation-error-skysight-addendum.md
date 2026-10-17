## Addendum: richer observation, error taxonomy, native Skysight machinery, IPC auth

### What it is

The first pass under-covered the native `SkyComputerUseService` surface. This folds in four clusters confirmed by `nm | swift demangle` and `strings` on the service binary and by the shipped `sky_js` sources: (1) richer observation primitives (a global selection reader, element-uniqueness disambiguation, per-value truncation ranges, refetchable AX elements, a screenshot-need classifier); (2) capture via private WindowServer SPI (occluded windows, Stage Manager offscreen gather, corner masking); (3) the exact `ServerErrorCode` taxonomy the model sees; and (4) the passive Skysight recorder + Computer-History MCP server + the XPC sender-authorization layer. For each, whether cua-driver has it and the port effort. All native paths CONFIRMED from symbols/strings; internal behavior INFERRED from signatures.

### Codex / Sky

Observation (native binary):
- `ComputerUse.SystemSelection` reads the current selection anywhere: `.llmDescription(_:lineOptions:) -> String`, `.contextType : UIElementContextType`, `.applicationWindow : AccessibilitySupport.ApplicationWindow`, `.windowRevision`, `.eventStreamDescription(...)`, `.writeScreenshotToFile(...)`. CONFIRMED.
- `AccessibilitySupport.FrequencyTable` for element uniqueness: `.isUnique(A) -> Bool`, `.increment(A)`, `.subscript -> Int`. Used to disambiguate otherwise-identical elements. CONFIRMED.
- Clipped-value ranges: `AccessibilitySupport.AXTruncationDescriptor { availableRange: Range<Int>?, count: Int }`, `AXPartialValue.textTruncationDescriptor`, `TransformedUIElement.truncationRange : NSRange?`. Reports how much of a value was clipped, not just a boolean. CONFIRMED.
- Refetchable AX + invalidation: `RefetchableSkyshotAXTree`, `RefetchableUIElement.refetchIfNeeded(validateElement:ignoreValueChange:)`, `ComputerUseAppController.lastAXTree : RefetchableSkyshotAXTree?`, `SystemSelection.invalidationMonitor : UIElementTreeInvalidationMonitor?`. Interaction path `prepareToInteract(with:...) -> (RefetchableSkyshotAXTree, UIElementProtocol)` re-resolves a stale element before acting. CONFIRMED.
- Screenshot-need classifier: `SystemSelection.screenshotNeededForContext : Bool`, `SystemSelection.updateScreenshotNeededUsingClassifier(screenshot: CGImageRef)`, mirrored onto `ComputerUseSkyshotAttachment.screenshotNeededForContext`. A model decides whether the text tree alone suffices or a pixel screenshot must be attached. CONFIRMED.

Capture (native binary, private SPI):
- `SystemSoftware.WindowServerSPI.captureWindowImagesInRect(_:options:rect:connection:) -> [CGImageRef]?` with `WindowServerCaptureOptions.ignoreGlobalClipShape`, plus `CGWindow.snapshot(width:height:)` / `snapshotAtBackingSize()`, `mainConnectionID()`, `associatedWindowIDs(forWindow:)`, `getWindowBounds`/`getScreenRect`. This is the occluded/background path, independent of ScreenCaptureKit. CONFIRMED.
- Stage Manager offscreen gather: `ComputerUse.StageManager.addWindowToStageIfOffscreen(_:)`, `.addWindowToStage(_:)`, `.offscreenWindowIDs() -> Set<UInt32>`, `.isEnabled`. Pulls an offscreen target back onto the active stage before capture/interaction. CONFIRMED.
- Corner-radius masking: `WindowServerSPI.resolvedCornerRadius(forWindow:) -> CGFloat?` and `SlimCore.WindowCornerRadiusMetrics.legacyDefault`. CONFIRMED.

Errors (model-facing, `sky_js/.../mac/errors.js` + `errors.d.ts`):
- `ServerErrorCode` is one flat enum, codes -10000..-10020 (21 values). Model-relevant members: `appNotAllowed -10006`, `runningApplicationNotFound -10007`, `accessibilityError -10008`, `permissionsNotGranted -10009`, `userStoppedSession -10012`, `incompatibleClientVersion -10013`, `permissionsPending -10014`, `blockedURL -10015`, `userIntervened -10016`, `ambiguousApp -10018`, `screenLocked -10020` (plus infra codes `senderProcessNotAuthenticated -10000`, `couldNotGetSenderPID`, `couldNotGetBootstrapPort`, `invalidApp`, `noActiveSession`, etc.). CONFIRMED.
- `SkyComputerUseError` carries `{code, errorName, request, requestType}` and reverse-maps code→name; `SkyComputerUseTransportError` wraps connection/framing failures separately. CONFIRMED. `messagesPermissionsPending` and `isScreenLocked` also appear as strings. CONFIRMED.

Skysight (native, passive always-on recorder, distinct from the CUA agent path):
- `EventStreamRecorder` (Observable): `_eventTap : AccessibilitySupport.EventTap`, `_axObserver : AXNotificationObserver`, `_frontmostAppObserver`, `textBuffer : TextInputBuffer`, `terminalValueChangedBuffer : TerminalValueChangedBuffer`, `latestURLByWindowID : [UInt32 : URL]`, `shouldCaptureApplication : (NSRunningApplication)->Bool`, `onRecordStored` / `onSuppressedRecordStored`, `flushPendingRecords()`. So: a CGEvent tap plus AX observers building per-window URL / typed-text / terminal buffers. CONFIRMED.
- `SkysightService`: `getSettings`/`updateSettings`, `pruneExpiredSegments()`, `recentApplications(now:) -> [SkysightRecentApplication]`, `clearHistory(_:now:calendar:)`/`canClearHistory`, `updateObservationPolicy(_:forApplication:)` and `(_:forURLDomain:)`, `StopReason`, scheduled resume task, `SkysightSegmentWriter`, `SkysightMemoryPipeline`/`SkysightSummariser`. 14 method symbols. CONFIRMED.
- Observation allow/block + URL blocklist: `ComputerUseURLBlocklist`, `ComputerUseURLBlocklistCache {entries, now, ttl}`, `EventStreamURLPolicyRecordFilter.isURLBlocked`, `SkysightObservationRule` / `ObservationRuleScope` / `ObservationDefaultBehavior`. CONFIRMED.
- Computer-History MCP server: `ComputerUseClient.ComputerHistoryMCPServer { run(), tools: [MCP.Tool], toolDefinitionVersion }`; tool strings `computer_history_get_settings`, `computer_history_pause`, `computer_history_resume`, `computer_history_status`, `computer_history_update_settings` (server id `computer-history`). CONFIRMED. This is a separate MCP surface from the CUA action IPC.

IPC auth (native binary):
- XPC listener: ObjC `ComputerUseIPCXPCSession { listener:shouldAcceptNewConnection:, sendRequestWithTypeName:requestData:codexMetadataData:fileDescriptors:withReply: }` and `ComputerUseIPCServer.handleEvent:withReplyEvent:`. CONFIRMED.
- Sender authorization: `ComputerUseIPCSenderAuthorization.ProcessIdentity`, `ComputerUseIPCSenderContext { clientType, mcpRuntime, parentIdentity, responsibleIdentity, allowsLockScreenAutoUnlock }`, `ComputerUseIPCSenderContextResolver`, `ComputerUseIPCPermissionResult` / `PermissionRequirement`. The service resolves and authorizes the calling process identity per request; failure surfaces as `senderProcessNotAuthenticated -10000`. CONFIRMED.
- The wire itself (from `native-pipe.js`): length-prefixed JSON-RPC (4-byte LE length, 8 MiB frame cap) over the unix socket `~/Library/Group Containers/2DC432GLL2.com.openai.sky.CUAService/IPC/computeruse.sock`, with a `ping` version handshake (`CodexComputerUseIPC-5`) and a host `ensureService` launch RPC. CONFIRMED.
- `ChromeExtensionInstaller` ("Add to Chrome" flow): ObjC class `Codex_Computer_Use.ChromeExtensionInstaller`, held by `CUAServiceApplicationDelegate.chromeExtensionInstaller`; strings `_isChromeExtensionInstalled`, `_isChromeExtensionInstallEnabled`, `_isChromeExtensionInstallInProgress`, "Add to Chrome", plus install-banner analytics events. CONFIRMED.

### cua-driver

Observation:
- Current-selection reader: absent. No `get_selection` tool or `SystemSelection` analog. `AXSelectedText` appears only as a write path (`platform-macos/src/tools/type_text.rs` `set_string_attr(element,"AXSelectedText",...)`, `focus_guard.rs`, `terminal.rs`), never as a read-current-selection observation. No `contextType`/`llmDescription` for the focused selection. Looked in `cua-driver-core/src/tool.rs` (tool list), `element_query.rs`, `platform-macos/src/ax/tree.rs`.
- Element uniqueness / FrequencyTable: absent as such. Disambiguation is by stable `element_index` (`cua-driver-core/src/element_query.rs::project_elements_for_query`, `HashSet<u64>` of visible indices), not a frequency table over labels.
- Truncation: partial. `platform-macos/src/ax/tree.rs` has a `truncated: bool` at both the whole-tree level (DFS stops at `max_elements`, emits "AX tree truncated at N nodes") and a per-node `truncated` flag, but no `availableRange`/`count` describing how much of an individual value was clipped. Nearest: the `truncated` field near `ax/tree.rs:139` and `:335`.
- Refetch / invalidation: absent for AX elements. `invalidate`/`stale` in the tree exist only in Linux overlay z-order (`platform-linux/src/overlay.rs`) and testkit observers; no `RefetchableSkyshotAXTree` / `refetchIfNeeded` re-resolve-before-interact contract in `platform-macos/src/ax/tree.rs`.
- Screenshot-need classifier: absent. No `screenshotNeededForContext`. `window_inspection.rs` instead marks a static `capture_coverage` recovery hint (`BrowserChromeCaptureCoverage`) rather than a learned classifier deciding whether pixels are needed.

Capture:
- Private WindowServer occluded path: absent by design. `platform-macos/src/capture.rs` uses public ScreenCaptureKit (`SCScreenshotManager::capture_image`, `SCContentFilter` desktop-independent window, warm TTL cache) with a `screencapture -l <id> -x -o` CLI fallback. No CGS/SLS `captureWindowImagesInRect`, no `ignoreGlobalClipShape`. Occlusion is only *classified* (testkit `observer.rs` `TargetZ::BackgroundOccluded`, `is_occluded`), not captured through.
- Stage Manager offscreen gather: absent. No `addWindowToStageIfOffscreen`. "offscreen" in cua is browser-viewport ranking (`browser/semantic.rs`) and recording markers (`cua-driver-core/src/recording.rs`), not a macOS Stage Manager gather.
- Corner masking: absent (`resolvedCornerRadius` has no cua analog; not needed without the private capture path).

Errors:
- Matches at the class level, not code level. `cua-driver-core/src/server.rs` defines `ToolErrorClass { InvalidParams, UnknownTool, PermissionDenied, BackgroundUnavailable, TransportError, InternalError }` and `ToolRefusalCode` (17 `browser_*` refusal codes + `Other`). `protocol.rs::ToolResult` carries `permission_denied_result(...)` and structured `code` strings. So permission/transport/ambiguous-target concepts exist, but there is no numeric `-100xx` enum, no `appNotAllowed`/`screenLocked`/`userStoppedSession`/`incompatibleClientVersion` members, and no `SkyComputerUseError` code→name object; cua's rich vocabulary is browser-refusal-shaped, not app-launch/permission-shaped.

History:
- Present and arguably ahead on privacy: `cua-driver-core/src/history.rs` is an encrypted (ChaCha20Poly1305 + HKDF + COSE `CoseEncrypt0`), metadata-only history with `pause()`/`resume()`, `prune_expired_chunks`, retention/quota (`DEFAULT_RETENTION_DAYS=7`, `DEFAULT_QUOTA_BYTES=100MiB`), a rich `HistoryHealthCategory` set, and two MCP tools `history_status` + `history_query` (bounded, "omits screen and page content, geometry, and inferred user intent"). `platform-macos/src/history.rs` is the platform key-store shim.
- But: cua history records only the agent's own tool actions. There is no passive `EventStreamRecorder` (no CGEvent tap / AX observer capturing the human's typing, per-window URLs, or terminal buffers), no `recentApplications`, no per-app / per-URL-domain observation policy, no `SkysightSummariser` memory pipeline. Codex-ahead on passive ambient capture; cua deliberately narrower.

IPC:
- `cua-driver-core/src/socket_io.rs` is framed writes to a unix daemon socket with EAGAIN/`SO_SNDTIMEO` retry; `server.rs` exposes stdio and HTTP MCP transports and authorizes at the tool layer (`authorize_tool_call(name,args)`), not the caller-identity layer. No `getpeereid`/`SO_PEERCRED`/audit-token/XPC sender-identity resolution, no `ProcessIdentity`/`SenderContextResolver`, no `senderProcessNotAuthenticated`. Absent (searched `socket_io.rs`, `server.rs`).
- Chrome-extension installer: absent (it is an app onboarding affordance, not a driver concern).

### Delta

- Matches: an encrypted local history with pause/resume/prune/quota exposed as MCP tools (cua `history_*` vs Codex `computer_history_*`); coarse error semantics (permission-denied, transport-error, ambiguous/stale target); AX-tree size truncation with a truncated flag; occlusion *classification*.
- cua-ahead: history encryption depth (ChaCha20Poly1305/COSE, HKDF-derived per-chunk keys, explicit quota + health taxonomy) and its stated metadata-only privacy contract; `ToolRefusalCode` browser vocabulary is finer-grained than anything in `ServerErrorCode` for the browser case.
- Codex-ahead: read-current-selection (`SystemSelection`), element-uniqueness `FrequencyTable`, per-value truncation ranges (`AXTruncationDescriptor`), refetch-before-interact with invalidation monitor, the screenshot-need classifier, private-WindowServer occluded/offscreen capture + Stage Manager gather + corner masking, a numeric model-facing error enum, the entire passive Skysight recorder/service/summariser + Computer-History MCP + observation allow/block + URL blocklist, and XPC sender-identity authorization.

### Recreate / port plan and "should we build this" call

Worth porting (native-patch scale):
- Read-current-selection tool. Hook: add a `get_selection` tool in `cua-driver-core/src/tool.rs` backed by a new `platform-macos/src/ax` reader over `kAXFocusedUIElement` + `kAXSelectedText`/`kAXSelectedTextRange` + owning window/app + a `context_type`. Reuses the existing markdown renderer. Genuinely useful for a generic driver (models routinely need "what is selected").
- Per-value truncation range. Hook: extend the per-node struct near `platform-macos/src/ax/tree.rs:139` to carry `available_range`/`count` when a value is clipped, alongside the existing `truncated` bool, and surface it in `element_query.rs`. Small, high-value for editing tasks.
- Error-taxonomy alignment. Hook: add app-launch/permission/session members (`app_not_allowed`, `screen_locked`, `user_stopped_session`, `incompatible_client_version`, `permissions_pending`, `ambiguous_app`) to `ToolErrorClass`/`ToolRefusalCode` in `server.rs` and thread them through `permission_denied_result` in `tool.rs`. Cheap; improves parity with what a model already expects from Codex.

Larger, judgment call:
- Refetch/invalidation before interact. Hook: wrap the macOS click/type path with a "re-resolve element by stable id, validate role/value, retry once" step in `platform-macos/src/tools/*`. Worth it for flaky dynamic UIs (large-build), but only after the tree carries stable handles the way `RefetchableUIElement` does.
- Private-WindowServer occluded capture + Stage Manager gather. Non-goal for now: it depends on unstable CGS/SLS SPI (`captureWindowImagesInRect`, `ignoreGlobalClipShape`) and Stage Manager internals, and cua's stated cross-platform contract favors public ScreenCaptureKit. Keep the existing classify-then-recover approach in `window_inspection.rs` instead. Revisit only if occluded-window capture becomes a hard requirement.

Should-not-build (app-specific / out of scope):
- Passive Skysight recorder + summariser + Computer-History MCP + observation policy/URL blocklist. This is ambient user-behavior capture for a consumer app's memory feature, not a driver capability; it carries a large privacy/consent surface. cua's action-only encrypted history is the right scope for a generic daemon. Non-goal.
- Screenshot-need classifier. Requires a trained model and Codex's attachment pipeline; a generic driver should keep returning both text and (optionally) pixels and let the caller decide. Non-goal unless cua ships its own classifier.
- XPC sender-identity authorization (`ComputerUseIPCSenderAuthorization`). This exists because Codex's service is a shared macOS XPC endpoint that must vet arbitrary callers. cua's daemon is single-tenant over a private unix socket with tool-layer authorization; adding audit-token peer auth is only warranted if the socket becomes multi-client. Defer / non-goal.
- `ChromeExtensionInstaller` "Add to Chrome". App onboarding UI, not a driver concern. Non-goal.
