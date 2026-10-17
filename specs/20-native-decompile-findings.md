## Native decompile findings (Ghidra) and the feature-flag surface

### Method

Ran Ghidra 12.1.3 headless on the aarch64 cloud workspace against OpenAI's `SkyComputerUseService` (arm64 Swift, 23 MB). The public Ghidra release ships no aarch64 decompiler, so the `ghidra_opt` decompiler native was compiled from source on the box (`ARCH_TYPE=` override to drop the x86 `-m32` flag). Auto-analysis on this Swift binary is pathologically slow, so the heavy decompiler-based analyzers were disabled and each target function was decompiled on demand. ~148 functions matching the target substrings were decompiled (the archived log carries 149 `=====FUNC=====` entries). This turns several spec items from symbol-name inference into read-the-code facts, and it surfaces why the remaining constants could not be pinned.

### Confirmed from the decompiled code

- Monotonic element IDs (specs 01, 02). `UIElementRenderTree.NextAvailableElementIDIterator.next()` decompiles (archived log lines 5317-5343) to a load-counter / store-counter-plus-one: `uVar3 = *(ulong *)(unaff_x20 + 0x10); ...; *(ulong *)(unaff_x20 + 0x10) = uVar3 + 1;`, with an `SCARRY8(uVar3,1)` signed-overflow trap guarding the increment. The element ID is a monotonic +1 counter, not a per-snapshot index. (The archived evidence is Ghidra's C decompilation, not disassembly; an earlier draft quoted an ARM `adds x0, x2, #0x1` from the live-session disassembly view, which was not saved to the log. The load-add-store is unambiguous in the archived C.)
- The AX diff machinery is real and structured (spec 02). Decompiled: `UIElementRenderDifference` with `Change` (an enum carrying `path: IndexPath`, `depthFirstOrder`, `offset`), `Change.inheritElementID()`, `Change.lines(indent:options:)`, `Change.lineCounter(options:)`, `UIElementRenderDifferenceBuffer.sortChangesDepthFirst()`, and `UIElementTreeRevision.changes`. This is exactly the old-vs-new render diff, keyed by tree path in depth-first order, that spec 02 described.
- Refetchable AX with invalidation (specs 01, 19). `RefetchableSkyshotAXTree.refetchElementIfNeeded(validate:ignoreValueChange:)` and an `invalidationMonitor: UIElementTreeInvalidationMonitor` are present, confirming elements are re-matched and revalidated across turns rather than re-indexed blindly.
- Settle is parameterized, not a fixed constant (spec 07 / observation). `ApplicationUIElement.waitForUIToSettle(delay:notificationDelay:includingScrollEvents:)` takes two `Double` delays as parameters. The decompiled body is only the async prologue that stores the caller-passed delays and does a `swift_task_switch`; the values come from the caller or a default-arg generator, so there is no single hardcoded settle number in this function.
- The screenshot-need classifier exists and is feature-flagged (spec 19). `SystemSelection.screenshotNeededForContext` is a stored `Bool`; `updateScreenshotNeededUsingClassifier(screenshot:)` reads `SlimCore.FeatureFlag.skyshotClassifier.isEnabled` before running, so whether a screenshot is even captured is gated by a flag and a classifier, not a fixed rule.

### The main finding: behavior is pervasively feature-flagged

The single biggest result of the decompile is that OpenAI's computer use is gated by a `SlimCore.FeatureFlag` system, which together with the Statsig client means much of the behavior is server-controlled experiment state, not fixed constants. (Counts, re-verified against the archived binary and reproducible: `strings | grep -ic statsig` = 214 hits, `nm | grep -ic statsig` = 1789 mangled-symbol hits, so Statsig is pervasive. Demangling the symbol table and extracting `FeatureFlag.<name>` yields 68 distinct names, archived as `decompile/feature_flags_full.txt` (a handful — `defaultValue`, `description`, `cached`, `allFeatures` — are `FeatureFlag`-type members, not flags, so the true flag count is a little under 68); `decompile/feature_flags.txt` is the 16 computer-use-relevant subset below, all of which appear in the full list. The earlier "50 flags / 231 Statsig" figures were live-session counts that do not reproduce, so the durable numbers are these.) That is why the specs' INFERRED numbers resisted extraction: several are not fixed values at all. The flag names also independently confirm every subsystem the specs cover.

Computer-use-relevant flags read out of the binary:

| Flag | What it gates | Spec |
|---|---|---|
| `axTreeDiffing`, `axTreeDiffingRemovedElementIDRanges` | AX-tree diff and the removed-ID-range compaction | 02 |
| `computerUseCursor`, `detachComputerUseCursor`, `overrideFogTexture` | the ghost cursor and its fog/texture | 08 |
| `computerUsePIP`, `computerUsePIPOverrideMinimizeButton` | the live picture-in-picture view | 17 |
| `skyshotClassifier` | the screenshot-need classifier | 19 |
| `computerUseAlwaysSimulateClick` | forces a synthetic mouse click instead of the AX action | 06 |
| `computerUsePersonalInstructionsOverridesBuiltIn` | user personal instructions overriding the built-in per-app playbooks | 04, 14 |
| `messagesIntegration` | the native Messages tools | 15 |
| `memory`, `memoryForIntegrations` | Skysight-style memory | 03 |
| `chromeExtensionInstall` | the Add-to-Chrome extension installer | 13 |
| `loopStepLimit`, `evaluations` | agent step cap and the eval harness | 12 |

`computerUseAlwaysSimulateClick` is the notable new one: it confirms OpenAI ships a mode that forces synthetic mouse clicks over the AX action, the same coordinate path my spike proved does not hit-test to a background control, so it is presumably used only when a control is not AX-addressable. `computerUsePersonalInstructionsOverridesBuiltIn` confirms the per-app playbook layer (spec 04) is overridable by user instructions.

### Honest limits

Exact magic numbers stayed out of reach for the effort budget, and for a principled reason. The diff line budget is threaded through `LineOptions` and compared inside the traversal, the settle delays are caller-passed Doubles resolved in default-arg generators, and the classifier thresholds live behind a flag-gated helper. Swift async splits these across continuation funclets, so reading one clean constant means chasing several. More decisively, the feature-flag surface shows many of these are dynamic experiment values, so there is often no single fixed constant to recover. Pinning specific numbers would need either the live-capture route (record the real IPC traffic while a task runs) or many more hours of funclet chasing for low marginal value.
