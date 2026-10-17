2026-10-15 scope decision: Nathan selected capture, review/navigation, replay, summaries and memory. The active target is H1-H4 and M1-M2 across three platforms: 10 proven, eight open, plus their existing privacy and G08 runtime dependencies. The broader matrix remains historical/deferred scope. Finish collection and cleanup of the repository batch already running, then continue only the [active core queue](../study/audit/QUEUE.md#active-core-queue-2026-10-15). Signed-build evidence and separate code/evidence commits remain required; no push.

2026-10-15 current signed field-context checkpoint: [Mac 51, Windows 20 and Linux 22 native assertions](../study/audit/parity-field-context-2026-10-15/README.md#current-signed-c-checkpoint) pass with exact hosted request/response bindings. Full field values retain ordinary identifiers; raw typing stays conservative, and credential wording prevents shape clearance. Useful partial-window summaries, literal recall, same-process memory controls and Mac recorded replay are verified within the stated scope. Windows cache reuse, provider-selection and relay failures remain retained. Signing grants are removed. Full H1, Windows H2/H4/M1/M2 and G08 work remain runnable; H3-W still awaits the signed-in reference. No cell promotion.

2026-10-15 field-context choice: ordinary identifiers in named nonsecret fields are retained by `25c6ab4cc`; review/import follows in `84ce0f727`. The [notarized Mac comparison](../study/audit/parity-field-context-2026-10-15/evidence/mac-a-verified.json) passes 31 candidate and eight signed-in reference assertions with eight real hosted exchanges. Exact literal recall and fresh-process memory-off pass. Generic summary omission and two stale-target setup failures remain retained; benign summaries and same-process target controls remain runnable. Windows native component tests exposed a library lock-replacement bug, fixed in `4bd04e5e5` with all 86 Windows stream tests passing. Current signed B builds and cross-platform native regressions are in progress. [Qt5 initialization diagnostics](../study/audit/parity-linux-qt5-capture-2026-10-15/README.md) remain component-only. No cell promotion; active counts remain 10 proven and eight open.

2026-10-15 repository signing: [native verification](../study/audit/parity-linux-repository-signing-2026-10-15/evidence/verified.json) binds eight APT and six RPM signature cases, final GPG/updater signatures over identical RPM bytes, and native before/after evidence for the metadata-renewal namespace fix `bd3416bd3`. The first age fixture failed XML parsing and is retained as invalid expiry evidence. Corrected, signed metadata with a 40-day-old revision is accepted by DNF 4, confirming the client freshness gap. Fedora key-expiry/retirement behavior remains under test; production rollout and broader S-L are unchanged. No cell promotion.

2026-10-15 authenticated Debian upgrades: [nine current signed checks](../study/audit/parity-linux-updater-rotation-2026-10-14/evidence/deb-success-verified.json) prove 0.1.0 to 0.1.1 to 0.1.2 through two normal GNOME administrator prompts. OS one-shot authorization receipts bind requesting PIDs and exact signed package paths. Both relaunches retain settings and the expected driver/collector. Original package bytes and the exact locked administrator account are restored; temporary credentials are removed. The failed first authentication remains retained. Other U-L sessions/formats and human first-consent remain open.

2026-10-15 Debian update refusal: [nine signed native checks](../study/audit/parity-linux-updater-rotation-2026-10-14/evidence/deb-refusal-verified.json) pass in one complete run. Missing dependency is named before unpacking; signature, tamper and version refusals preserve the old package. Normal Escape cancels the real GNOME authorization prompt with the old app still running. Exact paused History settings and clean dpkg state survive. Both failed adapter attempts remain retained. Successful package upgrades and the other U-L formats/desktops remain runnable.

2026-10-15 updater rollover: [signed AppImage verification](../study/audit/parity-linux-updater-rotation-2026-10-14/evidence/verified.json) passes all 14 native checks in one GNOME Wayland run. Old-key-signed transition, new embedded key/channel, old-key refusal, new-key acceptance, fresh-old-client recovery, tamper/version refusal, dismiss/no-download and exact paused settings retention pass. The failed selector attempt remains retained. This is isolated test distribution; U-L/S-L keep their other formats, desktops, repository and production-release requirements.

2026-10-15 KDE input discovery: [signed AD native evidence](../study/audit/parity-linux-portal-deadline-2026-10-14/evidence/plasma-discovery-verified.json) binds two actual hosted attempts with zero receiver effects. Installing the missing normal KDE portal backend preserves app, driver, KWin and helper bytes, but input still refuses because the helper lacks a target-bound delivery API. This is runnable implementation work in R-L; no guard is removed and no cell is promoted.

2026-10-14 GNOME portal closure: [signed AD verification](../study/audit/parity-linux-portal-deadline-2026-10-14/evidence/ad-native-verified.json) passes four complete cases, 41 native assertions and nine hosted exchanges. Timeout/denial preserve typed zero-effect refusals; approval after 14 seconds delivers exactly once. Stop cancels the pending grant with no input, and a new task in the same app obtains a new driver and fresh consent. Foreign same-user cancellation is refused. All AC counterexamples remain retained. KDE portal and broader R-L requirements remain open.

2026-10-14 signed portal checkpoint: [AC native verification](../study/audit/parity-linux-portal-deadline-2026-10-14/evidence/ac-native-checkpoint.json) passes approval after 14 seconds and explicit denial. The unanswered dialog closes with no input but loses its typed per-window timeout. Stop followed by late approval still delivers F6. Both signed failures are retained; host-authenticated setup cancellation and precise refusal reporting are under test. No R-L promotion.

2026-10-14 portal deadline: implementation `4bf3b7ad8` closes acquired Remote Desktop sessions after timeout/denial, expires queued input with its original caller and gives Linux input time to return a typed driver outcome. [Fifteen native protocol tests, fourteen transport tests and two killed mutations](../study/audit/parity-linux-portal-deadline-2026-10-14/evidence/component-verified.json) pass. Signed AC native timeout, denial, delayed approval and Stop acceptance is pending. Broader R-L remains open; no cell is promoted by component tests. Current queue wording also reconciles the completed Z capture/replay contracts without promoting broader H1.

2026-10-14 Fedora setup: [signed AA verification](../study/audit/parity-linux-kwin-setup-2026-10-14/evidence/fedora-aa-verified.json) passes normal approval, desktop persistence, actual relogin and 22 capture checks on Fedora 43 KDE Wayland. Runtime ABI and binary remain unchanged. RPM correctly refused the first empty RUNPATH, and its normal checks pass after rebuilding without one; a stale same-version app install is retained and corrected with a normal reinstall. [Signed AB identity-only setup](../study/audit/parity-linux-identity-setup-2026-10-14/evidence/signed-ab-verified.json) passes all 14 native checks, preserving paused state, exact settings bytes and identity-only scope; content values remain absent. Other I-L distribution/format and human-consent requirements remain assigned; matrix count is unchanged.

2026-10-14 KWin setup: [signed AA verification](../study/audit/parity-linux-kwin-setup-2026-10-14/evidence/signed-aa-verified.json) proves normal capture-scope approval persists only the helper preference in the running desktop configuration, automatically loads after a real login and passes all 22 capture regressions. The app's isolated profile stays separate. Apt's refused nonmonotonic test version remains retained; the corrected increasing version installs normally without a downgrade override. This closes the Ubuntu 26 KWin persistence subgap in I-L; other distro/format and human-consent checks continue. Matrix count remains eleven proven, 36 open and four not applicable.

2026-10-14: **H4 Linux is proven on signed Z for its frozen three-session contract.** [Independent verification](../study/audit/parity-linux-kwin-input-2026-10-14/evidence/full-z-verified.json) requires 52/52 assertions in one complete cohort each on GNOME X11, GNOME Wayland and Fedora KDE X11, including all 16 replay controls. All 156 assertions, 229 actual hosted exchanges, source/package binding, 15 negative verifier controls and process/clock cleanup pass. M1/M2 Linux are refreshed by these complete cohorts. Prior missed-action and relay failures are preserved; no partial checks are combined. Broader H1 capture and R-L desktop/portal coverage remain open. [Signed Plasma capture](../study/audit/parity-linux-kwin-input-2026-10-14/evidence/signed-plasma-z-verified.json) passes 132 assertions and [AppImage acceptance](../study/audit/parity-linux-kwin-input-2026-10-14/evidence/appimage-z-verified.json) passes rich/lifecycle plus normal FUSE launch and capture. Candidate AA still needs signed approval/relogin acceptance for KWin persistence. Current count: eleven proven, 36 open and four not applicable.

2026-10-14 Wayland continuation: signed Y passes the complete 52-check X11 cohorts on GNOME and Fedora KDE, but misses the initial GNOME Wayland Export action. The failed native-only attempt is retained, with zero hosted requests. Commit c8b3685b8 adds bounded background GNOME target capture and integrates the Linux lane KWin observer; commit 50bd82dbc adds nonexclusive core X11 key observation. [Independent component verification](../study/audit/parity-linux-kwin-input-2026-10-14/evidence/integrated-components-verified.json) binds actual effects, delay/refusal controls and current source. Signed Z full acceptance is running; H4-L remains open. KWin's initial inactive-helper state before normal scope approval remains tracked under I-L. No cell is promoted by these components.

2026-10-14 signed continuation: W passes all 52 memory/replay assertions on GNOME Wayland and Fedora KDE X11. Its complete GNOME X11 run passes 51/52 and misses the native Export action, so H4-L stays open. [All actual native/model evidence](../study/audit/parity-linux-replay-2026-10-14/README.md) is retained. Fix 4cd7092a1 preserves already resolved safe X11 targets through a bounded collector delay; [native timing and 159 core tests](../study/audit/parity-linux-input-retention-2026-10-14/evidence/component-verified.json) pass, with signed Y acceptance pending. [Signed X helper migration](../study/audit/parity-linux-helper-upgrade-2026-10-14/evidence/helper-x-verified.json) closes that I-L subgap with 14 native assertions; its rejected cache-inventory attempt is retained. [KWin input discovery](../study/audit/parity-linux-kwin-input-2026-10-14/README.md) is component-only. No whole cell is promoted; ten proven, 37 open and four not applicable remain.

2026-10-14 replay continuation: signed V passes all 16 H4 assertions in complete GNOME X11 and Fedora KDE X11 runs, with [independent native/model verification](../study/audit/parity-linux-replay-2026-10-14/evidence/h4-v-x11-checkpoint.json). GNOME Wayland remains required. Commit d735db6be adds caller-bound pointer metadata to the existing helper and sole History collector; real compiled-reader clicks pass, but signed app acceptance is pending. Commit 9ec2a2ea8 fixes isolated-config GNOME extension selection loss; [Signed W normal setup and fresh-install verification](../study/audit/parity-linux-replay-2026-10-14/evidence/helper-w-verified.json) passes; safe helper-version migration remains tracked under I-L. No new cell is promoted.

2026-10-14 browser scope clarification: [fetched official documentation](../study/audit/parity-browser-scope-2026-10-14/README.md) distinguishes passive Computer History capture from explicit browser-history queries. B1 now states the request-scoped approval, denial/revocation and no-always-allow checks separately. The existing Linux native browser contract is unchanged; Firefox authority and native package gaps are not silently marked N/A. No cell is promoted by this documentation review.

2026-10-14: **H2 Linux is proven on signed V.** [Independent verification](../study/audit/parity-memory-terminal-2026-10-13/evidence/h2-v-verified.json) passes 329 native assertions across seven offered GNOME/KDE X11/Wayland sessions and 15 provider-dependent deletion assertions. All five frozen cases run: summaries, deleted previews, complete review, retention and confinement. Package/source bindings, actual hosted exchanges, visible summary geometry, five negative verifier controls and cleanup pass. Fresh Fedora repository/session inventory confirms GNOME X11 is unavailable. [Retained failures and fixture scope](../study/audit/parity-memory-terminal-2026-10-13/README.md#h2-l-review-closure) remain explicit. Current count: ten proven, 37 open, four not applicable. H1/H4 capture, browser, runtime and the other queue cells remain active work.

2026-10-14: **M1 Linux and M2 Linux are proven on signed V.** [Independent verification](../study/audit/parity-memory-terminal-2026-10-13/evidence/memory-v-verified.json) requires one complete 28-assertion memory cohort per declared session: GNOME X11, GNOME Wayland and Fedora KDE X11. It checks signed packages, 668 product inputs, 483 reconstructed native Rust inputs, real hosted exchanges, receiver bytes, citations, isolation, corrections, terminal privacy, clock/deletion controls and six negative verifier controls. No partial attempts are combined. That checkpoint contained nine proven, 38 open and four not applicable. H2-L native review followed; H1/H4 capture and R-L deadline/cancellation remain separately open.

2026-10-14: [Signed V input verification](../study/audit/parity-memory-terminal-2026-10-13/evidence/entry-focus-verified.json) binds 48 native typing trials to the packaged driver. `b042e4c1f` waits for compositor acknowledgement; `1f0c62b9d` prevents text-field clicks from invoking Activate and submitting a default button. The real GTK counterexample, first failed test builds and all failed memory attempts remain retained. Complete signed V memory cohorts are in progress. R-L still includes the separate portal deadline/cancellation and remaining runtime checks. No matrix cell is promoted by this checkpoint.

2026-10-14 runtime counterexample: signed U on GNOME Wayland receives the correct UNKNOWN plan for memory-off recall, but its type actions leave both native fields empty before Export. The task stops without replay. The driver replies before flushing EIS frames, allowing the focus guard to restore the prior window too early. A real protocol regression fails on that ordering; an ordered completion callback is under test. M1-L/M2-L and R-L remain open. [Retained native run H](../study/audit/parity-memory-terminal-2026-10-13/evidence/u-memory-cells-wayland-h-evidence/memory/result.json).

2026-10-14 checkpoint: [Windows V](../study/audit/parity-memory-terminal-2026-10-13/evidence/windows-v-verified.json) passes 29 signed native memory assertions plus five correction-capture prerequisites. Its first correction retry was caused by a hosted HTTP 503, with the correct source already present. [Linux U frozen verification](../study/audit/parity-memory-terminal-2026-10-13/evidence/u-frozen-memory-checkpoints.json) accepts GNOME X11 at 51/52 and Fedora KDE X11 at 52/52. All M1/M2 assertions pass in both. The missing X11 Export-click recording and Wayland consent deadline remain separate open implementation gaps; GNOME Wayland memory acceptance continues. No new cell is promoted.

2026-10-14: **M1 macOS is proven on notarized window U.** [Independent verification](../study/audit/parity-memory-terminal-2026-10-13/evidence/mac-window-u-verified.json) binds 668 source inputs, 59 native checks and nine signed-in reference checks. Earlier literal recall, correction across two real time windows, exact citations, source substitution/deletion and fresh UNKNOWN recall pass, with explicit unchanged prior isolation/privacy/concurrency dependencies. Current count: seven proven, 40 open, four not applicable. Linux U completes 51/52 with only the separate recorded Export-click attribution assertion false; remaining Linux sessions are next.

2026-10-14: Signed Linux U passes the corrected cross-window recall stage, but misses a native Export click without concurrent packaging. H1-L/H4-L retain that separate attribution gap; the click assertion is unchanged. The signed Mac U and reference both return the latest corrected note from distinct real ten-minute windows; final source/deletion verification is continuing.

2026-10-13: Signed T proves earlier-source literal recall and source/deletion controls on Mac against the signed-in reference. Linux T still supplies an obsolete value for a correction in a newer summary window. Fix `502f57a96` passes the two-window regression and 37 shared tests; current signed native acceptance remains required. M1 macOS stays open.

# Status

2026-10-13: **M1 macOS is reopened** for a shared retrieval gap: newer unrelated fields can consume the six-field source budget before an explicitly requested older project is reached. The Mac-code regression fails before the change and passes with source selection by the recall clause. Exact field-bound citations and current signed/native regression remain in progress; the prior latest-literal proof stays retained. The Fedora harness also incorrectly accepted an older artifact as a newly published summary, so fresh-source publication checks are being strengthened. Current count: six proven, 41 open, four not applicable.

2026-10-13: signed R reaches 50/52 in the GNOME X11 memory suite, with safe terminal output lost after an input-attribution gap. Secret and command canaries remain excluded. `5662784f2` separates input reset from terminal diff/redaction state and passes the new regression plus 86 stream tests. [Signed S now passes all 11 native terminal/privacy/lifecycle checks](../study/audit/parity-memory-terminal-2026-10-13/evidence/s-native-verified.json); hosted summary and remaining session validation are pending, so M1-L/M2-L remain open. Fedora R also exposes a harness stale-control failure and the known core-X11 Return-observation gap; neither is marked proven.

2026-10-13: **H3-L is proven on signed R** after replacing the incomplete P evidence with the [full seven-session audit](../study/audit/parity-linux-integration-2026-10-13/evidence/h3-r-final-verified.json). The installed signed deb/RPM and ABI-bound helper verify all 777 repeated-navigation checks, 119 original single-cycle checks and 77 actual-model continuation checks. Seventy native PNGs show the reviewed values; exact file outputs prove completion through reload and follow-up target reuse. Stock Fedora 43 GNOME X11 is positively not offered, following the frozen contract. Completed H3 processes are gone and fixtures are reused for the remaining matrix. **R-L remains open** for the separate foreground portal deadline defect. Current count: seven proven, 40 open, four not applicable.

2026-10-13 continuation: signed R independently passes 111 GNOME X11 History navigation checks with visible values in all ten captured windows, then a real hosted task completes through native reload and a follow-up reuses its target. [Navigation](../study/audit/parity-linux-integration-2026-10-13/evidence/r-x11-verified.json) and [task effects](../study/audit/parity-linux-integration-2026-10-13/evidence/r-reload-verified.json) remain scoped checkpoints. **H3-L stays open** for remaining declared desktop/distribution coverage. The Wayland foreground run exposes a separate R-L deadline gap: app transport stops at 12 seconds while libei readiness waits 20 seconds, leaving the OS Remote Desktop prompt pending. The stale prompt was cancelled with zero receiver effects. Default background and explicitly authorized foreground paths are tracked separately.

2026-10-13: **A-L is proven on signed R** after the native Pulse integration. [Independent evidence](../study/audit/parity-linux-integration-2026-10-13/evidence/r-audio-verified.json) binds 668 product inputs, the signed package and 24 installed files to current source, then verifies 32 source/consent checks, four lifecycle cases, 16 device checks and six API cases. Real PipeWire/PulseAudio waveforms and WAV bytes verify; owned devices, defaults and helpers are cleaned up. The retained three-second disconnect failure is not relabeled: acceptance uses the pre-existing four-second contract with a 0.75-second PCM tolerance. H3-L remains open. Current count: six proven, 41 open, four not applicable.

2026-10-13 correction: **H3-L is open.** The earlier promotion omitted current X11/Plasma coverage and accepted an accessibility visibility flag despite P screenshots showing the value below the viewport. P's hosted reload was cancelled afterward, so continuation is unproven. Its correct source is `67b172120`, not O's install receipt. [Corrected audit](../study/audit/parity-linux-integration-2026-10-13/README.md). Current count: five proven, 42 open, four not applicable.

2026-10-13 superseded claim, withdrawn by the correction above: Signed P closes Linux H3. Ten GNOME Wayland History cycles pass 111 checks with visible reviewed values, keyboard-only return, native window PNGs and deletion/revisit; hosted reload restores the selected window in one app process. The [independent verifier](../study/audit/parity-linux-integration-2026-10-13/evidence/h3-linux-verified.json) binds the result to `67b172120`. H1, H2, H4, M1 and M2 retain their separate open checks. A-L is open for the changed native Pulse backend.

2026-10-13 historical checkpoint, H3 claim withdrawn above: `37d2320e4` integrates the Linux lane's compositor, browser, native Pulse and package support, preserving quoted literal recall and source readiness. Current signed P is installed; Linux H3 is proven with a hosted reload continuation. **A-L is reopened** for this changed backend, with N's previous complete proof retained. The Linux app passes 486 tests and frontend 187 tests plus five adapter tests. Current count: six proven, 41 open, four not applicable.

2026-10-13: **A-L is proven** for the requested Linux audio extension. [Signed N and independent verification](../study/audit/parity-linux-audio-stacks-2026-10-13/README.md) cover real PipeWire/PulseAudio source isolation and routing, late-source duration, native consent, bounded WAV/API output, missing-input recovery, Stop/quit/crash and cleanup. The signed-in Linux reference lacks this native API. Mac/Windows A retain reference gates; refreshed Mac AH/Windows O source, API and lifecycle checks pass independent decoding with scoped permission/device inheritance. Count: six proven, 41 open, four not applicable.

2026-10-13: Linux standalone PulseAudio exposed a late-source audio gap: a two-second Both request retained only 0.1600625 seconds of system audio when that source began late. `4b6d75b6d` waits for each requested source to deliver or report unavailability, preserving per-source sample caps, original timestamps and bounded startup/Stop. Three platform unit suites and real recorder component tests pass; [the signed audio-stack audit](../study/audit/parity-linux-audio-stacks-2026-10-13/) closes A-L; Mac/Windows retain their reference gates.

2026-10-13: [native application identity verification](../study/audit/parity-native-app-identities-2026-10-13/README.md) fixes Linux summary rejection of bare executable names and the mismatch between recorded identities and prettified task labels. Signed Linux M passes all 12 native capture/summary/restart/recall controls, with fresh Mac AG/Windows N regressions. Signed K proves real task continuation, selected-window reuse and retained privacy/provider choices through native reload. Full Linux session and memory-control coverage remains open.

2026-10-13: [signed literal-recall verification](../study/audit/parity-literal-recall-2026-10-13/README.md) closes the shared retrieval failure in `2309bddb8`. Mac AF and Windows L export the exact instruction-shaped source as data with zero Send/Decoy effects; memory-off and deleted-source recall return UNKNOWN. Native citations, tamper refusal and Mac app isolation pass. The signed-in Mac reference exports identical bytes. M1/M2 Mac return to proven with [explicit dependency inheritance](../study/audit/parity-literal-recall-2026-10-13/evidence/inheritance.json). Windows/Linux full cells remain open. Current count: five proven, 42 open, four not applicable.

2026-10-13: [retained signed-in Linux reference evidence](../study/audit/parity-linux-reference-handoff-2026-10-13/README.md) confirms native Messages and Contacts are unavailable on Linux, making MSG-L/CT-L not applicable. Linux History and audio remain requested extensions. The newer lane's signed audio implementation differs from this checkout, so its passes are not combined with our runs without dependency verification. Current count: three proven, 44 open, four not applicable.

2026-10-13: [literal recall](../study/audit/parity-literal-recall-2026-10-13/README.md) is a shared M1/M2 gap. Signed Linux 0.4.1 returns an older note because instruction filtering removes the exact retained literal. Side effects remain zero. M1/M2 macOS are reopened pending the revised shared policy and signed Mac/Windows regressions. Typed untrusted-source quoting is under test; no privacy filter is globally weakened. Current count: three proven, 46 open, two not applicable.

2026-10-12 [signed Linux G](../study/audit/parity-linux-history-2026-10-12/evidence/signed-g-verified.json) records real focused GTK fields through the one History collector. Native consent, password exclusion, pause/resume, block-rule precedence and locked-background updates pass. Production labels/value review works. Interaction, terminal, browser and full Wayland capture remain open, so no complete cell is promoted.

2026-10-12 native audio continuation found a valid odd final sample count rejected by WAV export. Signed Linux E reproduces the failure. Fix `87ba3e3d5` preserves the last resampled frame and passes six WAV checks using a real stopped recording. [Signed Linux F export](../study/audit/parity-audio-api-2026-10-12/evidence/odd-final/native-f-wav-fixed.json) now matches the independent decoder. Mac AD and Linux F pass native source/WAV checks and strict API tests. Windows J signing recovered after a fresh scoped role and propagation interval. Its installed payload, API and native source/WAV checks pass; the role is removed. [Current evidence and scoped inheritance](../study/audit/parity-audio-api-2026-10-12/README.md#current-signed-continuation) retain all failed fixtures. Audio remains open for valid reference comparisons.

2026-10-12 [Mac compatibility checkpoint](../study/audit/parity-macos-compat-2026-10-12/README.md): signed AC loads and fits a 2x display on macOS 14.8.3, and its packaged API initializes. The stock image has SIP disabled; remote authorization input failed, so no native control or permission pass is claimed. The owned instance was destroyed before expiry. I retains OS 11-13 and real 2x control checks.

2026-10-12 signed audio API candidates AC/I/E pass 15 native cases, including
normal approval, denial/cancellation/late-reply refusal, subsecond duration,
WAV filepath/bytes/data URL and EOF cleanup. [Independent evidence](../study/audit/parity-audio-api-2026-10-12/evidence/bounded-verified.json)
verifies six saved recordings against a separate decoder. The first signed runs
exceeded their limits; their [failed duration evidence](../study/audit/parity-audio-api-2026-10-12/evidence/baseline-duration-check.json)
is retained. Sample capping fixes the overrun. Valid signed-in reference audio
capture remains open; no complete matrix cell is promoted.


2026-10-12 WAV export is verified through signed native UIs on all three
platforms: [Mac Z](../study/audit/parity-macos-release-2026-10-12/evidence/wav-z-verified.json),
[Windows F](../study/audit/parity-windows-batch-2026-10-12/evidence/wav-f-verified.json)
and [Linux B](../study/audit/parity-linux-batch-2026-10-12/evidence/wav-b-verified.json).
Independent decoding verifies 24 kHz stereo PCM16, exact sample counts and
selected source signals. Repeat export preserves inputs and starts no sensors.
The gated start/stop API, millisecond duration and bytes/data-URL compatibility
remain open. Windows F also repeats the ten History navigation cycles on the
console; RDP exclusion remains a compatibility gap in R.

2026-10-12 format continuation: the current signed reference SDK still exposes
audio behind `SKY_ENABLE_AUDIO`, with 100-300,000 ms bounds and WAV output.
Its signed-in app runtime does not document those methods even with the parent
flag enabled. The shared local WAV export in `f3d53371d` passes five backend
checks and two UI tests; native signed Z/F/B acceptance and API compatibility
remain open. [Linux A source acceptance](../study/audit/parity-linux-batch-2026-10-12/evidence/audio-a-verified.json)
now passes independent signals, six controls, device-error recovery and all
finalization paths. Windows A is returned to runnable work for the shared
format change; reference sign-in remains an additional gate.

2026-10-12 signed Y: [native Mac audio checks](../study/audit/parity-macos-release-2026-10-12/evidence/audio-y-verified.json)
verify the microphone-permission fix, independent sources, duration/consent,
missing-device recovery and all finalization paths on the notarized download.
Historical WAV/API compatibility remains in A. [Mac signing provenance](../study/audit/parity-macos-release-2026-10-12/evidence/signing-y-verified.json)
is complete for this candidate; S now waits only for production updater-key
ownership/custody. Linux A packages build and the signed test repository installs
verified payloads; standard-user X11 acceptance is continuing. No new cell is
marked proven at this checkpoint.

2026-10-12 Mac audio checkpoint: signed X passes independent system/microphone
source isolation and Stop, automatic completion, quit and crash finalization.
[Verification](../study/audit/parity-macos-release-2026-10-12/evidence/audio-checkpoint-verified.json)
retains a failed revocation status: the OS supplies silence, but X reports a
successful saved recording. `e46c39c65` checks AVFoundation permission first;
signed Y acceptance is pending. The signed-in reference exposes no public audio
methods in its actual runtime documentation. A macOS stays open for the requested
extension and remaining signed checks. No cell is promoted.

2026-10-12 audio continuation: the signed Windows E package finalizes its recording
after abrupt app death, with both helper processes reaped. The [decoded crash
receipt](../study/audit/parity-windows-batch-2026-10-12/evidence/audio-e-verified.json)
binds the 3.2-second manifest to real saved samples. Normal consent, Stop and
quit still pass. Independent 440/880 Hz source isolation and twelve signed UI controls also
pass. Windows A now awaits the signed-in reference output contract.

2026-10-12 release checkpoint: [Mac W](../study/audit/parity-macos-release-2026-10-12/README.md)
is signed, notarized, stapled and downloaded by Safari in a secured owned guest.
It adds recovery buttons after denied macOS privacy prompts. The [Windows batch](../study/audit/parity-windows-batch-2026-10-12/README.md)
builds, passes 175 memory tests and verifies the signed installed payloads; its
temporary signing role is removed. Windows signing needed the same operator
to authenticate with its password for DPAPI. Native permission/navigation checks
continue, with failures retained. Windows records resumed input but redacts the
ordinary fixture identifiers Juniper1 and ResumedNavigation1. H1 retains that
capture-fidelity comparison; H3 continues with prose canaries. Windows H3 candidate acceptance passes 114 checks across ten uninterrupted
cycles. S candidate acceptance verifies real certificate expiry, rotation and
tamper refusal. Both cells now await signed-in reference acceptance. The count
stays five proven, 44 open and two not applicable.

2026-10-12: **M1 macOS is proven** for [its matrix check](../study/audit/parity-matrix.md).
[Signed U and the signed-in reference](../study/audit/parity-memory-reuse-2026-10-12/README.md)
match causal recall, scoped preferences and facts adjacent to an injected
instruction. The last control found useful facts discarded on signed T; U keeps
a source-bound descriptive excerpt with complete-field privacy checks and
untrusted framing. Both reference and U export identical facts with no Send or
decoy effect. Actual provider bodies, receiver files, earlier failures and the
[final verifier](../study/audit/parity-memory-reuse-2026-10-12/evidence/verified.json)
are retained. There are **five proven cells, 44 open and two not applicable**.

The [memory carry-forward register](../study/audit/parity-memory-reuse-2026-10-12/carry-forward.md)
keeps quota/backoff persistence, judge deadlines, provider transitions, same-ID
merges, unsupported filesystem locks and stale full settings objects open in R.
H1 retains capture timing/fidelity. I/U/S retain distribution and notarization.
The reference's new injected recording did not produce an observed automatic
summary before the raw-record recall; that query follows the provider's documented
recent-history route. Earlier automatic-summary acceptance keeps its exact scope.
Both owned clock Macs and their leases are gone. The Mac mini reference login is
preserved; the shared Windows build and reference sign-in work continue.

2026-10-12: **H2 macOS is proven** in [the matrix](../study/audit/parity-matrix.md).
[Signed K and the signed-in reference](../study/audit/parity-history-review-2026-10-11/README.md)
cover growing review, scoped deletion, derivative cleanup, stale UI, exact source
proofs, causal recall, live-response deletion and interrupted writes. Actual
isolated clocks prove paused raw expiry, surviving-source recall and generated
memory persistence. The [verifier](../study/audit/parity-history-review-2026-10-11/evidence/verified.json)
binds source, signatures and 635 native members. Raw history defaults to two days;
existing or explicitly selected longer retention is preserved. Generated summaries
remain until deletion, superseding the older automatic-summary expiry behavior.
Reference app-session deletion removes an enclosing segment; our reviewed filters
preserve unrelated identities. **Four cells are proven, 45 remain open and two are
not applicable.** Both owned clock Macs and their leases are gone; the original
reference login is preserved. [QUEUE.md](../study/audit/QUEUE.md) continues with M1
macOS and the remaining autonomous work. Nothing was pushed.

2026-10-11, H2 acceptance in progress: [History review and deletion](../study/audit/parity-history-review-2026-10-11/README.md)
adds scoped deletion, source/derivative cleanup, retained imported-memory selection,
per-summary reveal and exact paused raw-retention sweeps. Signed Mac A passes native
capture, automatic-summary, reveal and cancellation checks. A native picker mismatch
then showed one app selected while another app's events remained displayed. Signed
B fixes that mismatch and passes 31 pagination, growing-prefix, consent and import
checks, plus paused restart and raw-byte preservation. The reference is signed in
and its native timeline/reveal flow passes. Actual item/app/range/all deletion,
retained-source recall, workflow removal, pending-response deletion and raw-retention
acceptance remain part of H2. [QUEUE.md](../study/audit/QUEUE.md) tracks the remaining
autonomous work. No additional matrix cell is claimed proven at this checkpoint.

2026-10-11, round eighteen virtual-control continuation: signed Windows F at
`a36877c8` passes [46 verified native checks](../study/audit/parity-round18-virtual-2026-10-11/evidence/virtual-candidate-verified.json).
The [continuation evidence](../study/audit/parity-round18-virtual-2026-10-11/README.md)
retains the signed baseline's missing WPF field value, an independent successful
export, and the corrected production run. Plain WPF editors are read only after
checking their scroll decorations. Virtual button invocations bind to the actual
provider sender and survive review/import/summary serialization. Protected and
nested controls, same-bounds replacement, pause/resume, scope revocation, one
collector, visible review and existing native HWND controls pass. The 24 WPF
checks and 22 native regressions belong to this exact signed build; the previous
106-check suite is not automatically inherited. Arbitrary virtual shortcuts,
other virtual control types, fresh hosted-summary acceptance and the signed-in
Windows reference comparison remain open. **No matrix cell closes.**

2026-10-11, round eighteen in progress: rich Windows capture and its hosted summary pipeline are implemented in the sole History collector, reusing Skysight normalization. Signed native runs cover continuous exports, Notepad/Windows Terminal, protected fields, pause and scope controls, crash/restart, real lock and UAC desktops, a separate unlocked input desktop, automatic summaries, source-prefix binding, deletion and restart. [Round eighteen](../study/audit/parity-round18-2026-10-11/README.md) retains exact build versions, failed attempts and independent receiver checks. That checkpoint candidate is signed L at source checkpoint `ab3663e5`, with [106 verified native checks](../study/audit/parity-round18-2026-10-11/evidence/windows-final-candidate-verified.json).

H1 Windows remains open. The official Windows reference is still signed out. The signed F continuation above addresses plain WPF field capture and virtual button invocations. Arbitrary virtual shortcut attribution, other virtual controls and complete current native coverage remain required. Sampled field state is not lossless keystroke capture: Willow and Alder's final full values first appeared after Export in the final continuous run. Linux rich capture and production Messages/Contacts remain open. No second recorder is shipped.

2026-10-11, round seventeen complete: **automatic macOS content summaries are proven** in [the matrix](../study/audit/parity-matrix.md), with [signed candidate and signed-in reference evidence](../study/audit/parity-round17-2026-10-10/README.md). One History collector feeds separately approved hosted summaries and scoped, untrusted source recall. Final signed17g recalls the frozen facts exactly after restart; memory-off and expired-source controls export UNKNOWN. The same expired raw facts remain physically present in the overlapping source-prefix control, so the negative result tests the retirement barrier.

The round retains 212 native candidate checks across successive signed builds, with explicit inheritance, and 15 reference recall/control checks plus native capture, cadence and deletion receipts. It closes terminal prompt leakage, terminal scrub overreach, clock-correction data loss and crowded-summary recall. Incomplete capture is labeled; scope/provider changes need fresh approval; live response pause/delete and concurrent writers are covered. [Verification](../study/audit/parity-round17-2026-10-10/evidence/verified.json) binds the runs to source, signatures, probes and cleanup. Backend tests: 429 passed, nine ignored. The candidate is Developer ID signed, not notarized.

**46 matrix cells remain open; two are not applicable.** Rich Windows/Linux capture and production Messages/Contacts are next. H1 still includes delayed event delivery, focus attribution, source-time ordering and typed-text casing. H2 still needs raw-file retention while interaction capture is paused; the new expiry proof covers summaries and source recall. Both owned clock VMs and leases were removed; owned native test processes, relays and signing material were cleaned up. The original reference login stays on the persistent Mac mini. No push.

2026-10-10, round sixteen complete: **executable macOS recorded-task replay is proven** in [the matrix](../study/audit/parity-matrix.md), with [signed candidate and signed-in reference evidence](../study/audit/parity-round16-2026-10-10/README.md). A native demonstration becomes a separately reviewed editable workflow, survives restart, accepts new inputs and exports exact bytes through fresh controls. The sole History collector incorporates Skysight's native sensor and redactor under an explicit interaction scope.

The current signed build passes 92 native checks across recording, replay and controls. Both it and the official reference produce matching standard and instruction-shaped literal output hashes. Changed/missing targets, moved controls, untouched decoys, denied Send, real-model cancellation/Stop, draft rejection and stale workflow deletion are covered. Eight native privacy checks inherit through byte-identical capture dependencies. [The verifier](../study/audit/parity-round16-2026-10-10/evidence/verified.json) checks these bindings. Historical failures and corrected test-harness failures are retained. The candidate is Developer ID signed, not notarized.

At the round-sixteen checkpoint, 47 matrix cells remained open and two were not applicable. Automatic content summaries were next, followed by rich Windows/Linux capture and production Messages/Contacts. H1 still includes typed-event case fidelity and source-time ordering; no full capture parity is claimed. The Windows candidate remains the `606d75e3` review-fix build with 17 native History checks and a pending reference sign-in. Native replay test processes and the temporary model-delay relay are stopped; the reference login is retained untouched on the Mac mini. Nothing is pushed.

The final export audit also found an ignored legacy VM credential in frozen source archives. `559aa73d` fixes the source exporter, with a failing-before/passing-after test. Current tracked archives and owned staging are scrubbed; installed candidates are clear, the original local file is untouched, and no nonterminated legacy host remains under either launcher tag. [Cleanup and archive projections](../study/audit/parity-round16-2026-10-10/README.md#source-archive-repair) retain the original hashes. Earlier Git objects are not rewritten.

2026-10-10, round fifteen complete: **continuous macOS History navigation is
proven** in [the platform matrix](../study/audit/parity-matrix.md), with
[real signed-build and signed-in reference evidence](../study/audit/parity-round15-2026-10-10/README.md).
The current signed candidate at `606d75e3` passes 20 setup checks, five native
review-deletion checks and 115 checks across ten uninterrupted cycles. It also
imports reviewed observations through the real GPT-6 Astra classifier, then
removes them from memory storage and review. The official signed-in reference
26.1007.30912 passes ten corresponding cycles (60 checks), native deletion and a
new GPT-6.1 Sol History query returning `HISTORY_EMPTY` after deletion.

This round fixes two native defects: a visible History panel missing from its
parent's accessibility children (`3bbfdd1b`), and cached reviewed text remaining
after its source activity was deleted (`606d75e3`). Failed signed runs are kept.
The latter fix also discards review responses arriving after deletion. The
candidate is Developer ID signed but not notarized. Shared-host process-start
failures were resolved for the audit by using a persistent native probe process;
no input with uncertain delivery was repeated.

At the round-fifteen checkpoint, 48 matrix cells remained open and two were not applicable. Round sixteen above closes replay; automatic content summaries, rich Windows/Linux capture and production Messages/Contacts remain. Only Computer History owns the production capture lifecycle;
merged Skysight components will be reused without starting a second host. The
round-fifteen Mac capture and test processes are stopped. A separately owned
Windows 11 VM is preparing the next signed build, with shutdown and cleanup at
2026-10-11 04:46 UTC. Nothing is pushed.

2026-10-10: [round thirteen](../study/audit/parity-round13-2026-10-09/README.md)
closes a Windows file-lock contention defect with a native failing/passing control,
335 passing backend tests and seven passing signed-package native cases. A fresh
notarized Mac passes 17 capture/review checks. Ten actual-model memory reuse checks
belong to the earlier baseline package. Continuous Mac History navigation remains
unproven. The Windows lifecycle probe found a second defect: discovery discarded
windows without the macOS Space field. [Round fourteen](../study/audit/parity-round14-2026-10-10/README.md) fixes that path
with a native Windows foreground identity source. The signed package passes 35 core
tests, 335 backend tests, seven native action cases and 14 continuous history checks.
The final run retains three native records and proves a new event after resume,
review consent, actual deletion and disablement. Both owned Macs and the Azure
group are gone; the temporary signing permission and relay credentials were removed.
Full parity remains open.

2026-10-09, round twelve: [reviewed history reuse](../study/audit/parity-round12-2026-10-09/README.md)
connects native history segments to recording review, paging, redaction and memory.
Observed values require separate review and approval. Prefix hashes prevent later
appends from expanding consent; native event IDs prevent duplicate imports.
Truncation is explicit, UTC provenance is retained, and observed domains have
typed grounding. A real model recalled the captured field text with memory and
returned UNKNOWN without it; app scoping, untrusted status and deletion passed.
This does not establish executable replay, automatic content summaries, new native
platform acceptance, a current full signed desktop package or reference parity.

2026-10-09, round eleven: [native browser history](../study/audit/parity-round11-2026-10-09/README.md)
adds separate browser consent, normal/incognito detection and verified page-domain
rules on macOS. A fresh Chrome 155 profile passes 23 checks, existing native apps
pass 15, and the UI/backend/MCP path passes 24. Model controls cannot expand browser
consent. Canonical domains reach history and memory without full URL secrets;
observed labels/values still stay out of Skysight model input. Tests pass: 32 core,
34 Skysight, 377 backend, 128 frontend and 65 GUI. Other browsers/platforms, frame
documents, ambiguous same-bounds windows, ambient-history replay/content summaries,
current full desktop packages and authenticated reference comparisons remain open.

2026-10-09, round ten: [native focused-control history](../study/audit/parity-round10-2026-10-09/README.md)
now has separate host consent for labels and text values on macOS. Fifteen native
synthetic-app checks, four real lock-screen checks and eighteen UI/backend/MCP
checks pass. Secure fields, web areas and locked sessions are withheld. Pre-read
policy tokens close the discovery/pause race, and the memory path uses the same
timestamp and lock order. The regression suite passes 24 core, 376 backend, 126
frontend and 65 GUI tests. Browser origin/private-mode context, rich Windows/Linux
capture, events/screenshots, a current packaged desktop run and authenticated
reference comparison remain open. The owned Mac is retained under its existing
lease for the next browser work; recording is paused and the screen is locked.

2026-10-09, round nine: [Computer History controls are connected to the app](../study/audit/parity-round9-2026-10-09/README.md)
and a five-tool MCP server sharing its store. Pause/settings changes invalidate
in-flight writes; one approved focus observation feeds both history and memory.
Thirteen UI/backend/MCP fixture checks pass. The core has 12 tests, with 373 app
backend cases, 124 frontend cases and 65 existing GUI cases passing. This capture
mode is still application identity only: native website resolution, rich semantic
recording and current native platform acceptance remain open. The reference Mac
expired without a desktop sign-in confirmation, so that comparison remains open.

2026-10-09, round eight: the [native Messages transport](../study/audit/parity-round8-2026-10-09/README.md)
is connected to reviewed sends, with persistent uncertainty tracking and retained
attachment copies. Sixty-six tests pass. The complete MCP-to-AppleEvents path
passes ten checks against a self-addressed scripting fixture, including a new
group and restart recovery. This does not prove Automation consent or signed-in
delivery through the real Messages app; those and the reference comparison remain
open. The actual production helper's no-interaction preflight returned not running.

2026-10-09, round seven: the [native Contacts helper](../study/audit/parity-round7-2026-10-09/README.md)
passes five fixture checks on the disposable Mac using the packaged service and
a Developer ID signed helper. Fifty-eight tests pass. Reviewed send planning now
supports editable approval, immutable files, membership/file revalidation and
uncertain outcomes, with five process checks through a capture-only transport.
Native AppleEvents delivery is still unavailable in the production executable;
no real messages were sent. The full reference desktop is awaiting sign-in for
the native comparison. The remaining Computer History controls are inventoried.

2026-10-09, round six: [Messages now has a runnable MCP read/image service](../study/audit/parity-round6-2026-10-09/README.md),
with host forms, scoped permissions, revocation, cancellation and attachment
confinement. Forty-seven tests and fourteen packaged service checks pass. The
package also builds offline from vendored source. It is a developer component,
not a signed app integration: reviewed sending, Contacts and native host UI
remain open, along with the reference startup failure and broader parity work.

2026-10-09, round five: the separate [Messages component](../impl/messages/README.md)
now has read queries, bounded workers, activity counts, schema adapters and a
host-only approval store. Its [audit](../study/audit/parity-round5-2026-10-09/README.md)
retains 26 passing tests and seven native fixture checks. All six interface
schemas match the captured reference, but the reference's native queries still
time out during bootstrap. Messages remains incomplete: the component is not
connected to the desktop app, and image retrieval, Contacts and reviewed sending
are still missing. This work does not change the accepted round-four binaries.

2026-10-09: the [round-four audit](../study/audit/parity-round4-2026-10-09/README.md)
tracks the broader feature target. The current signed Windows app passes visual
model control, audio capture through the UI and cleanup. The persistent JavaScript
runtime passes native permission and state-persistence checks. Signed UIAccess
forwarding works against a SYSTEM target and refuses unsafe-location and impostor
controls. Both native engines complete the same calculator and text task under
one model/controller.

Mac browser-download, permission and system-audio checks passed on the preceding
complete-audio candidate. The final Mac bundle is signed, notarized and stapled;
its final GUI changes have not had another full native acceptance pass. Read the
audit for exact artifact boundaries. The installed reference's normal model path
still fails native service startup. Messages, dictation, authenticated locked-session
use, exact runtime/API compatibility and refreshed Linux acceptance remain open.

The earlier [round-three audit](../study/audit/paired-round3-2026-10-09/README.md)
retains the long-task, recording/import, memory-reuse and native recovery proofs.
Their dated binary hashes remain intact. Full product equivalence is not established.

## Historical packaged-app update, 2026-10-07

[Four real-model macOS tasks passed](../study/audit/packaged-e2e-2026-10-07/README.md)
on their first submitted goal with 3,084 clean ownership samples and verified
cleanup. This includes calculator input, text replacement, two-window isolation
and recovery from a refused shortcut. The current native patch also passes 477
unit tests and ten live recovery cases. It now supports unchanged text-area
handles whose optional AX identifier is unreadable while still refusing their
replacement from an incomplete snapshot. The app refuses synthetic macOS key
actions, including single keys and shortcuts, because a live process-targeted
Ctrl+A reset shared HID idle state. Semantic value writes and accessibility
actions remain available; native driver keyboard APIs and Linux/Windows key
routes are unchanged. The candidate is ad-hoc signed and not notarized. This
acceptance does not establish arbitrary-app coverage or full Codex equivalence.
The dated baseline below describes the earlier platform-wide audit.

As of 2026-10-05, `fix/audit-backlog` at dc20e6cc; later commits changed only study folders, this page and three Windows test scripts (`app/tests/windows/gaps3-forward.ps1`, `gaps4-real.ps1`, `gaps5/inst.ps1`), no product code. This is the one current status page. Suite totals from the lead's final run on dc20e6cc (taken from the lead's report; no log of that run is committed and it was not re-run for this docs pass): app type check clean; vitest 107 passed; app backend 338 passed (6 ignored); `impl` cargo tests clean; conformance 195 passed, 7 skipped, 3 xfailed on `our` and 135 passed, 7 skipped, 63 xfailed on `cua`; patch-merge guard 63 passed; `apply-and-build.sh --check-generated` reports that the manifest and bindings match the source. Counts inside the rows below belong to the dated run each row cites. The tracked patch (`impl/native/cua-driver.patch`, 30853 lines, sha256 `10ca8e52...`) merges the Linux libei change and the Windows foreground-input fix, and on 2026-10-05 each platform was checked on it or on an identical binary ([gaps-final-build](../study/audit/gaps-final-build-2026-10-05/README.md), [gaps-final-build-3](../study/audit/gaps-final-build-3-2026-10-05/README.md)). Linux: built from this patch on real hosts, with the 151-cell gate (95 delivered, 56 refused, 0 failed), the facade probes and `--check-generated`. macOS: this patch built on a Namespace Mac (macOS 26.3.1) and signed with Developer ID (not notarized); tile click and wheel at 1x and 2x. Windows: not built as its own binary. The build that ran was this app source with the gaps-windows-5 patch, whose Windows crates are byte-identical to this patch's (the two patches differ in three `platform-linux` files), and its unsigned driver has the same sha256 (`5C7D9E7C...DBB0`) as the one gaps-final-build recorded for this patch; the equal hash is the evidence that the two are one binary (the A build's hash is stated in the gaps-final-build-3 report; its `build/` folder was not committed, the gaps-final-build hash is in `windows/results/build-evidence.txt`). Not run on it: the Linux mutants, the Windows guard mutants, a Windows 10 or 11 client.
Per-area detail, evidence paths and open items with their blockers are in
[seam-audit.json](../study/audit/seam-audit.json). The 226 scope requirements, with a
disposition, test and review field each, are in
[requirements.json](../study/audit/requirements-2026-09-25/requirements.json).

"Verified" means an adversarial review re-ran the evidence and it held. Round 1 is
[review-2026-09-25](../study/audit/review-2026-09-25/) (app, Linux, macOS, Windows);
round 2 is [review-2026-09-26](../study/audit/review-2026-09-26/) (app, Linux, Windows).
Review ids (N10, LR6, C8, ...) point into those files. When a review broke or overclaimed
something and a fix landed afterwards, the item is under Partial and says "not
re-reviewed". There was no round-2 macOS review, so every macOS fix after round 1 is in
that state, except where the 2026-10-04 gap passes re-ran it (Verified, macOS).

Between 2026-10-02 and 2026-10-05 a set of gap passes re-ran the Partial rows on real hosts with each
guard broken once, and built the parked items. They are cited by folder below. They are not a third
adversarial review: a row they closed is under Verified with what the run does and does not show,
and a fix they made says "driven in the same pass" where nobody else re-checked it.

Every native run was on a disposable host: Namespace Macs (macOS 15.7.5, plus one
acceptance pass of the f4d8bc2 bundle on macOS 26.3.1; the final macOS review and
[c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md) ran on 26.3.1 only), Namespace
Linux boxes (Ubuntu 24.04), EC2 Linux hosts (Ubuntu 24.04 and 26.04, Fedora 43, Rocky 10.2) and EC2 Windows Server 2022. Nothing was driven on the owner's desktop.
Nothing is published. macOS builds are Developer ID signed and notarized
([AUTO_UPDATE.md](AUTO_UPDATE.md)); Windows builds were signed with the real Azure Artifact Signing
certificate on 2026-10-03, 2026-10-04 and 2026-10-05 (Needs Nathan, item 2). Of the 2026-10-05 Windows runs, the
UIAccess forwarding on real elevated applications and the cancelled-UAC and standard-user updater runs (gaps-windows-4) and the final signed matrix (gaps-windows-5) used
it; the earlier ones that day (fixture forwarding, the merged-patch build) used a throwaway CA.

## Verified

### macOS

Installed `Copilot.app`, ad-hoc signed, one display at 1x, on macOS 15.7.5. macOS 26 draws a
32 pt title bar where 15.7 drew 28 pt, so a pad point recorded on 15.7.5 (100, 82) is
(100, 78) for the same screenshot pixel on 26.3.1; the probes read the title bar from the
live window. The f4d8bc2
bundle was accepted again on macOS 26.3.1 at 1x and 2x: every item below that the suite
covers held there, and a real model finished 3 of 3 tasks through the app's loop
([mac26-acceptance](../study/audit/mac26-acceptance-2026-09-28/README.md)).
[macos-native](../study/audit/macos-native-2026-09-25/README.md),
[macos-fixes](../study/audit/macos-fixes-2026-09-25/README.md), round 1
[macos.md](../study/audit/review-2026-09-25/macos.md).

- The app launches through LaunchServices, runs its own bundled driver with
  `--no-overlay`, and the driver exits with the app (N9). No driver window appears (N15).
- A clean install names both missing permissions and prompts only when Grant access is
  pressed (N5).
- Chat references, compute to 42, Stop during a pending model reply, and a native tool
  error that stops the run (N17).
- Packs arrive once per session and task; personal packs override or precede; a fresh
  `icu mcp` delivers again (N4, N6, N14).
- Background pixel clicks and drags no longer make the target active: 0 AppKit
  activations and 60 of 60 keystrokes kept by the user's app, on the fixes audit's 6 routes
  and the 10 more the review added (F3, F4, F6, A1).
- `set_value` on a button is refused `set_value_unsupported` and writes nothing (F8).
- The Apple menu is dropped from observations, and `invoke_menu` into it is refused before
  any focus change (F10, A3).
- A window on a `CGSSpaceCreate` Space reports `on_current_space: false` and gets the
  off-Space badge (F11). A current full-screen Space reports correctly (A5).
- Ids survive value changes and identifier-backed relabels; two anonymous title-bar
  buttons keep their ids when nothing about them changed (N13, F14).
- The TCC grant is keyed on the app, not the re-signed sidecar (F15).

**Rechecked and closed 2026-10-04** (rows moved from Partial; Namespace Macs 15.7.5 and 26.3.1).

- **Notarized app, 26.3.1** ([final-mac-3](../study/audit/final-mac-3-2026-10-04/README.md)). Built from 4c6fd18:
  `spctl` says "Notarized Developer ID", the ticket is stapled, and a quarantined zip opens with the
  normal first-open prompt (Cancel or Open) and one click, no "cannot be verified" block and no
  override. With the real model (`claude -p` through ccroute): the multi-line text area task ends
  with Body `line one\nline two`, the guard app in front throughout, 535 samples and 0 violations;
  Calculator 6 x 7 gives 42; a preference is learned and recalled in a second run; an observed note
  gates a send and a clean preference does not. Not re-run there: reversal, narrower-message, delete
  gate, focus probe, panel, sweep. `press_key return` resets the HID idle timer (a counted event, not
  an ownership change). The combined tree after the later fixes (Developer ID signed, not notarized)
  passed the same text area, Calculator, memory and tile checks ([gaps-mac-2 §2](../study/audit/gaps-mac-2-2026-10-04/README.md#2-item-4-the-combined-tree)).
- **A grant survives an update** ([gaps-mac-live §4.3](../study/audit/gaps-mac-live-2026-10-04/README.md#43-update-with-a-tcc-grant-probe_update_livepyprobesprobe_update_livepy)).
  0.1.0 with Accessibility and Screen Recording rows keyed to its Developer ID requirement, updated
  through the app to 0.1.1 (signed with the real release key, local http server, builds not
  notarized): grants intact with no re-grant, requirement unchanged though the cdhash changed, the
  app still captures, clicks bound and types. A row keyed to 0.1.0's cdhash (what an ad-hoc
  signature amounts to) is lost by the same update. The rows were inserted into TCC.db, not granted
  in System Settings. An update offered 2 s into a real-model task left it alone (display 42, 0
  ownership violations); clicking Install & restart mid-task killed the task, a real bug, fixed in
  6b01817 (it refuses while a task runs), driven in the same pass.
- **Tile gestures** ([gaps-mac-live §3](../study/audit/gaps-mac-live-2026-10-04/README.md#3-item-1-tile-gestures-through-the-signed-app)).
  The signed app on a GUI session with real HID clicks: the tile click lands within 0.67 tile px at 1x and
  1.17 at 2x (7 points plus the first click, 15.7.5 and 26.3.1); the wheel request carries a frame id
  the driver issued for that window (the shipped 4c6fd18 sent none) and scrolls the bound window 30
  while a decoy at the same screen place scrolls 0; the first click with another app in front is
  delivered once (F5 was the harness's consent prompt). Not shown: the old app's wheel also scrolled
  the right window, so the difference is the request, not the outcome; 2x was the 1280x800 mode only.
  Repeated 2026-10-05 on an app built from this HEAD's patch on a Namespace Mac (macOS 26.3.1) and signed with the Developer ID identity, not notarized ([gaps-final-build-3, macOS](../study/audit/gaps-final-build-3-2026-10-05/mac/README-mac.md)): the click's worst error is 0.667 tile px at 1x (1920x1200) and 1.167 at 2x (the 1280x800 mode, Copilot resized to 980x530 to fit), 7 points plus the first click each, every click 1 down and 1 click; the wheel scrolls the bound window 30 (3 events) with the decoy at 0 at both scales, with a frame id the driver issued (`tap_wheel_has_issued_id` true at 1x; at 2x the report says the id was issued by the driver). No regression against the numbers above. Still 2x in the 1280x800 mode only, macOS 15 not run on this patch, and notarization and a notarized download's first launch not exercised. At driver level on the same patch (gaps-final-build, a separate Namespace Mac): the text area `set_value`, Calculator 6 x 7 = 42 and the Save-sheet `set_value` scenario pass ([report](../study/audit/gaps-final-build-2026-10-05/mac/README-mac.md)); those three were not rerun through the signed app.
- **Running apps** (F17, F18; [gaps-mac §1.1](../study/audit/gaps-mac-2026-10-04/README.md#11-running-apps)).
  Apps launched after the daemon are listed 1.5 to 1.8 s after launch (3 of 3 on 26.3.1); the live
  Process Manager list answers and the cached `NSWorkspace` list is read only when it is empty, and a
  test fails when the order is reversed (3cbf152 was right; it had no test of its source order).
  Ambient memory is the app layer's row and was not looked at.
- **Focus-steal suppression** ([gaps-mac §1.2](../study/audit/gaps-mac-2026-10-04/README.md#12-focus-steal-suppression-and-the-restore-path)).
  15.7.5: a fixture launched through `launch_app` was frontmost in 0 of 12 runs with the driver and in
  55 or 56 of 56 or 57 samples without it. The worry that the `launch_app` and `invoke_menu` restore does nothing from
  the daemon did not reproduce: AppKit's activate fronted the target from a daemon-shaped process 12
  of 12 times; `activate_pid` now goes through WindowServer first (0 ms against 28 to 49 ms) and three
  comments that said otherwise are corrected. Not shown: an app that activates itself some time after
  launch (the fixture never took the front, even in the control with `open -g`, and on 26.3.1 it
  cannot), so that case rests on unit tests.
- **`set_value`** (R4-2 and earlier, [mac-review4-fixes](../study/audit/mac-review4-fixes-2026-09-26/README.md#r4-2-set_value)).
  Rerun on 26.3.1: `NaN`, `inf` and `1e999` are `invalid_arguments` on a stepper and a slider with
  nothing written, the increment-4 wrap is not applied and the value restored, a date picker's
  stepper is `set_value_unsupported`, range edges refuse on both sides. Text a control refuses is now
  `invalid_arguments` (not a number, slider or stepper) or `set_value_rejected` with the AX error (a
  date field), nothing written; before, a raw `-25201` with no code (fixed 101499f, run red and green on 15.7.5;
  the red result files were not kept, the v020 run is) ([gaps-mac-live §4.1](../study/audit/gaps-mac-live-2026-10-04/README.md#41-set_value-on-1575-bug-101499f)).
  Earlier fixes: round 3 B1, O1 ([mac-review3-fixes](../study/audit/mac-review3-fixes-2026-09-26/README.md#b1-o1-set_value)). Not rerun on 15.7.5 apart from the bad-text cases.
- **Session hotkeys** (R4-3; [gaps-mac §1.4](../study/audit/gaps-mac-2026-10-04/README.md#14-session-hotkeys-r4-3)).
  On 26.3.1 and 15.7.5: Ctrl-Cmd-Q (hotkey and `press_key`), Shift-Cmd-Q, Opt-Cmd-Esc and Ctrl-F2 are
  `system_hotkey_refused` with no key event reaching a keylog fixture; with the Apple menu walked to
  Lock Screen, `type_text` is refused at every item and Return in both modes; the desktop-scope click
  on the Apple logo is refused; the session stayed unlocked. With the guard removed (a build whose
  session-chord table is bypassed) the same call locked the 26.3.1 session. A shortcut remapped in
  System Settings still focuses the menu bar; the choosing input is what is refused. Before round 1, a
  background Ctrl-Cmd-Q locked the session.
- **Install.** `install.sh` picks an Xcode with SDK 15.2 or later: on a 15.7.5 Mac whose default SDK is
  15.0 it chose Xcode_16 (SDK 15.5) of nine installed, and an offline test with a fake `xcrun`
  covers it ([gaps-mac §1.5](../study/audit/gaps-mac-2026-10-04/README.md#15-install)). The review-fixes reproduce
  block sets a fresh Mac up itself (round 3 B2). The round-1 Lock Screen reds block was not rerun
  verbatim (its bundles are gone); the Lock Screen red itself was rerun on 26.3.1.
- **Screenshot binding** (15.7.5 and 26.3.1; [gaps-mac-live §4.2](../study/audit/gaps-mac-live-2026-10-04/README.md#42-screenshot-binding-on-1575)).
  7 of 7 cases on both, and the build with verification removed fails the same probe (the click lands
  at (100, 10) after a resize). Point scroll is bound; an unbound pixel is read against the calling
  session's own latest capture of the window (the core rule under Pixel binding), refusing
  `ambiguous_capture` when the session has none and `stale_capture` after a resize or scale change;
  `icu` forwards `screenshotId`. Earlier fixes: round 3 B5, round 4 R4-1, round 5 R5-1 and R5-2, the
  resize case ([c9-unify](../study/audit/c9-unify-2026-09-27/README.md)), final-mac F1 and F2 ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md)).
  New 2026-10-04: a hotkey's web-element focus click was passed to the pixel path as a caller pixel
  and its landing check read the unscaled pixel; Cmd-A on the second of two web fields selected the
  wrong field (default 1568 px ceiling, Retina window on 26.3.1), or was refused. It is now a driver
  point and the check reads the pixel that was clicked; Cmd-A selects the right field at a 400 px
  capture, the ceiling and with no capture, on both OSes (4d3484b, driven in the same pass; the
  structural test of the wrapper only fails on the mutation, the live table is the behaviour).
- **`type_text`** (round 3 B4, [mac-review3-fixes](../study/audit/mac-review3-fixes-2026-09-26/README.md#b4-typing-into-the-addressed-window); [mac-review4-fixes](../study/audit/mac-review4-fixes-2026-09-26/README.md#b4-in-notes)).
  An `element_index` reaches the driver; foreground keystrokes are refused `type_text_target_not_focused`
  when another window is key or another element has focus (rerun on 26.3.1: passes, no code change); in
  Notes, text sent to the note body lands there with focus in the body or in the search field. The
  foreground rung could not make TextEdit's second document key, so that case is a refusal that
  points at background delivery.
- **Sky text** (15.7.5 and 26.3.1; [sky-format](../study/audit/sky-format-2026-09-26/README.md), [gaps-mac §4.4](../study/audit/gaps-mac-2026-10-04/README.md#4-real-mac-evidence)).
  The driver reports subrole, settable, focus, tree order, a row's collapsed container identifier and
  inherited scroll actions; live Clock renders `element`, lists scroll actions once and shows one `Add a
  clock` row; in nested Mac Catalyst scroll views each row's Scroll Down scrolls its own view. Against
  a hostile window on both OSes (a title and values with newline, tab, NEL, U+2028, U+2029 and VT,
  forged rows and focus lines, a selection with a triple-backtick line, a button titled like a
  Secondary Actions list): the title is one escaped line, no forged row appears, the fence is longer
  than the run inside it, and `perform_secondary_action` refuses all four labels with nothing pressed
  (S-A, S-C, S-D, R4-X2 hold). The separator characters are pinned by a test that fails when they are
  not escaped. The 60-level cap is a unit-test pin only: a live app's AX walk stops at 25 levels.

### Linux

Installed `.deb`, Xvfb and openbox with and without picom, headless sway 1.9 and mutter
46.2. [linux-fixes](../study/audit/linux-fixes-2026-09-25/README.md),
[displays](../study/audit/displays-2026-09-25/README.md),
[linux-review-fixes](../study/audit/linux-review-fixes-2026-09-25/README.md),
[screenshot-binding](../study/audit/screenshot-binding-2026-09-26/README.md), round 1
[linux.md](../study/audit/review-2026-09-25/linux.md), round 2
[linux.md](../study/audit/review-2026-09-26/linux.md).

- Capture failures are typed (`window_minimized`, `window_offscreen`), the tree still comes
  back, and the tile shows the reason (LF1, LF2).
- A GTK3 or Qt5 popup menu is `screenshots[1]` at its X geometry; another process's popup
  is left out; menu items get no tokens (LF6, ID5).
- On a two-head Xorg root, a pixel click lands on the button at the screenshot point in
  all 11 placements, and a click on a closed window is refused `stale_window_target`
  (DS1 to DS4, LF7, LF8).
- Input no longer makes the user's focused window blink: 0 guard deactivations. With the
  fix reverted, the canonical `toolkit_focus` oracle fails 60 of 60 background cells
  (LF9, DS3, LR7, CE3).
- i3lock is recognized (`host_not_interactive`); a bare keyboard grab is not a lock
  (LF11, LR17).
- Default Wayland: native windows are reported as not listed, in `list_windows`,
  readiness and the picker (LF3, WL1). Under rootless XWayland, desktop capture fails
  typed `desktop_capture_unavailable` and readiness agrees (LR6).
- A token refuses `stale_element_target` when its object moved, or its named control was
  relabelled or recycled, on GTK3 and Qt5 (ID1, LR8, LR10 to LR12).
- A landed insert no longer reports `delivery_failed` (LR14, LR15).
- `screenshot_id` binds click, double_click, right_click and drag on X11 and sway. A
  resize after the agent's capture refuses `stale_capture` even when a tile captured the
  window since (SB2 to SB5). The app's tile click carries its frame's id (SB9, app D1 to D4).
- The 143-cell canonical gate at `2617ea3`, checked from its records, not rerun (CE1 to CE4).
- Copilot's Send through its accessibility action (AT-SPI `doAction`, AXPress) started and
  at once stopped the run (linux-final bug 2): Send and Stop are one button, and a second
  half of the same activation landed on Stop. A Stop within 800 ms of the start is now taken
  as that activation and ignored; a vitest drives the click and submit sequence
  ([acceptance-bugs](../study/audit/acceptance-bugs-2026-10-01/README.md)). Not rerun on
  WebKitGTK; which event WebKitGTK doubles was not isolated.

**Rechecked and closed 2026-10-04** (rows moved from Partial; EC2 Ubuntu 24.04, [gaps-linux-windows linux.md](../study/audit/gaps-linux-windows-2026-10-04/linux.md)).

- **Pixel tools.** Click, double_click, right_click, drag, point scroll and all three hold tools check
  window and pid and take `screenshot_id`; without an id the pixel is read against the calling
  session's own latest capture of the window (the core rule under Pixel binding): no such capture
  refuses `ambiguous_capture`, a resize or a new window since (including one a new process made under
  the old XID) refuses `stale_capture`, whoever captured after; a held window that closed or was
  recreated under the same XID refuses `stale_window_target` and drops the hold; `drag` coordinates
  must be numbers. `from_zoom` (click, double_click, right_click, drag, hold tools) reads the calling
  session's own latest zoom of that window, refuses `invalid_arguments` with none and `stale_capture`
  after a resize or recreation; scroll takes no `from_zoom`. Re-review found the row overclaimed, two
  real bugs fixed (f40695d): a point at x equal to the width or y equal to the height was delivered one
  pixel outside the window (12 live cases, each refuses with no event now; the unit test had asserted
  the bug), and the zoom region was read in window pixels, not screenshot pixels (a click at the
  zoom image's centre landed at (100, 75) instead of (200, 150) on a half-size capture). The edge
  fix has a live and a unit guard; the zoom scaling has only the live probe (the unit test covers the
  helper, not its use). Earlier rounds: round 2 B2, round 3 C9, N1, N2, round 4 R4-1 to R4-5
  ([linux-review4-fixes](../study/audit/linux-review4-fixes-2026-09-26/README.md)), [c9-unify](../study/audit/c9-unify-2026-09-27/README.md),
  final-linux F1 and F3 ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md)). `rv2`, `rv3`, `bind`, `c9u`
  (22 cases) and `rv4` (65 cases, 4096-frame floods) pass at 8352bae.
- **Rows, XIDs, scroll.** Text-labelled rows refuse after recycling and after this driver's own write
  (no rebind; the model re-observes); a reused XID refuses an old id; point scroll is bound and its x
  and y must be numbers. GTK3 and Qt5: a new Qt5 fixture passes 11 of 11 (recycled rows refuse, the
  driver's own write stales the token, a re-observed token writes; Qt leaves the caret at the end after
  an AT-SPI write). Not tested: Tk, Electron, GTK4 tokens. Earlier: round 2 O1, O3, SB8, round 3 R3-O1, N3, N5.
- **Sway window capture.** Native Wayland windows (the Wayland opt-in) are refused outright with one
  typed reason (`surface_identity_unproven`) and readiness says so before any capture; the tree and
  input routes are unchanged. Without the opt-in, on XWayland's `DISPLAY`, only XWayland windows are
  listed and captured as X11 windows from that client's own buffer: re-measured in an Xwayland image,
  a 596x333 capture had 0 of the 60 px overlay bar's cyan pixels
  ([gaps-linux-2 gate.md](../study/audit/gaps-linux-2-2026-10-04/gate.md)). Tiled, fullscreen, XWayland under the
  opt-in, `zoom` and `screenshot_out_file` return no image; `token_click` lands; `pixel_click` refuses
  `ambiguous_capture` (the probe predates the C9 rule). Round 3 R3-B1, round 4 ([linux-review3-fixes](../study/audit/linux-review3-fixes-2026-09-26/README.md)).
- **Updater logic on Linux.** See Updates under Partial: AppImage and `.deb` built by `tauri build`, updated by the app on a real X session.
- **Full-desktop facade, GNOME Shell and KWin refusals.** See Verified, parked items, and the Linux package row.

**Rechecked and closed 2026-10-05** (EC2 Ubuntu 24.04; [gaps-hosts-4](../study/audit/gaps-hosts-4-2026-10-05/README.md), [gaps-linux-3](../study/audit/gaps-linux-3-2026-10-05/README.md), [gaps-tests-3](../study/audit/gaps-tests-3-2026-10-05/README.md)).

- **Five `icu` library tests that failed on Linux** ([gaps-hosts-4 linux.md](../study/audit/gaps-hosts-4-2026-10-05/linux.md#item-1-the-five-icu-library-test-failures-on-linux)).
  No product bug. Three `parked_skypipe_tests` read a `study/` fixture the host's source copy lacked (they now say
  so instead of a bare `NotFound`; a host copy must carry `study/`). The two `cua::tests` were an `ETXTBSY` race: a test
  writes a stand-in driver script and execs it at once, and Linux refuses to exec a file a forked child still holds
  open for writing (macOS does not, which is why the Mac passed). A retrying exec helper in the test code fixes it; with
  the old code 2 of 6 full runs failed, after it 15 of 15 runs under build load pass (198 passed, 0 failed), and a new
  test is red with the retry disabled.
- **Merged native patch built together** (shell agent change, desktop facade, UIAccess forwarding, macOS sheet fix;
  [gaps-hosts-4 linux.md](../study/audit/gaps-hosts-4-2026-10-05/linux.md#item-3-linux-half-the-merged-patch-rebuilt-together)).
  Pristine pin plus the tracked patch through `apply-and-build.sh` on a real host: `--check-generated` exit 0,
  platform-linux 501 passed (498 before), core 662, `pointer_plan` 9, facade probe 58 of 58 on Xvfb and 59 of 59 on
  headless sway. No regression. The Windows session-0 change altered the patch after this host was gone; gaps-linux-5 then built a later patch on a real host (round 2 below). The real GNOME and Plasma sessions were not
  repeated on the merged build then (the libei drag was repeated on HEAD's patch, round 2 below).

**Rechecked and closed 2026-10-05, round 2** (EC2 Ubuntu 24.04 and 26.04, Fedora 43, Rocky 10.2; [gaps-linux-5](../study/audit/gaps-linux-5-2026-10-05/README.md)).

- **libei drag and drag handle** ([libei](../study/audit/gaps-linux-5-2026-10-05/libei/README-libei.md#live-results)). In a `portal-libei` build on GNOME 46 Wayland and Plasma 6.6 Wayland, multi-point `desktop_drag` and `desktop_drag_handle` work: a fullscreen GTK client logging every event saw one press at the first point, motion with the button held through every point (a 3-point drag: 16 motions on GNOME, 17 on Plasma) and one release at the last; the handle across one `icu mcp` session (start at (150,150), a screenshot, `move_to` (450,350), end) pressed, made 8 held motions and released at the aimed points. The driver at 9fad1227 is red on both (`no-zwlr-virtual-pointer`, 0 events). A one-shot `icu call desktop_drag_handle start` is still refused (`driver_session`, 0 events) on both. 7 mutants of the gesture code are each red in unit tests (17 pass unmutated). The consent was KDE's `kde-authorized` entry on Plasma and the real Remote Desktop dialog answered through AT-SPI on GNOME. Not tested: a driver crash with the button held, a person at the dialog, a real GPU.
- **Final patch built on a real Linux host** ([libei §5](../study/audit/gaps-linux-5-2026-10-05/libei/README-libei.md#5-final-patch-checks)). The pin plus `7a13e2e6` (`merge-patch.sh` of 9fad1227 and the libei commit; not HEAD's patch, which also carries the Windows foreground-input fix): `--check-generated` exit 0 ("manifest and bindings match the source"), the facade probe 64 of 64 on Xvfb and 65 of 65 on headless sway, 45 `platform-linux` tests selected by `gesture`, `libei` and `wayland::tests` pass with `portal-input` (the full 501-test lib suite was not run), and the 25 paired gate cells plus 2 shared web cells pass 27 of 27 (Linux canonical gate row). Not run on that patch: the 151-cell gate and the mutants. The gate and a rebuild then ran on HEAD's own patch (`10ca8e52...`) in [gaps-final-build](../study/audit/gaps-final-build-2026-10-05/linux/README-linux.md): `apply-and-build.sh` exit 0, `--check-generated` exit 0, `platform-linux` 501 passed (510 with `portal-input`), `cua-driver-core` 662 passed, facade probe 64 of 64 on Xvfb and 65 of 65 on sway, the 151-cell gate 95 delivered, 56 refused, 0 failed, 27 of 27 paired and web cells, and the libei click, 3-point drag, 2-point drag and handle across one `icu mcp` session on GNOME 46 and Plasma 6.6.6 Wayland, with the stock driver delivering 0 events on GNOME.
- **Plasma 5.27, the Linux updater on Rocky 10.2, Fedora 43 and under Wayland, and the gate evidence.** See the Linux package, Updates and Linux canonical gate rows.


### Windows

Installed NSIS package, unsigned, standard user, a 100% primary plus 150% and 125%
virtual monitors at negative origins.
[windows-replay](../study/audit/windows-replay-2026-09-25/README.md),
[windows-review-fixes](../study/audit/windows-review-fixes-2026-09-26/README.md), round 1
[windows.md](../study/audit/review-2026-09-25/windows.md), round 2
[windows.md](../study/audit/review-2026-09-26/windows.md).

- Compute to 42 on each monitor with 0 changed ownership samples. Stop during a pending
  reply delivers nothing. A failed `set_value` and a refused `Win+r` stop the batch
  (R5, R6, C28). Since [refusal-feedback](../study/audit/refusal-feedback-2026-09-27/README.md)
  the run goes on after them: the refusal goes back to the model with a fresh view (see
  Refused actions under Partial). Re-run on Windows at `c9cad4c`
  ([windows-final-fix §5](../study/audit/windows-final-fix-2026-09-29/README.md#5-fresh-host-acceptance)):
  the refused `Win+r` and background `Ctrl+a` reach the model as REFUSED and nothing
  reaches the receiver; `acceptance.ps1` was updated to count that as the refusal it is.
- A background `Ctrl+a` is refused instead of typing `a`. Window scroll moves a WinForms
  list (R8, R9).
- Posted clicks on DPI-unaware windows whose controls are child HWNDs land on the right
  control, on every monitor and awareness mode: 228 of 228 probe requests (C2, C3).
- `screenshot_id` binds pixel actions. An id from before a move or resize refuses
  `stale_capture`, a later tile capture no longer launders it, and a minimized window
  that was never captured refuses `window_minimized` (C8, C10).
- `/Brepro` makes the driver byte-identical across hosts and target dirs (C13), and
  across source paths and `CARGO_HOME`s since the round-3 fixes (remapped paths, a fixed
  junction for the tree; final review F12 to F14). The round-3 claim about line endings
  held only for a checkout that already had the patch (F15), and "the host" left out the
  toolchain (F16); both were fixed after the final review and are not re-reviewed (see
  Windows final-review fixes).
- Classic Notepad's hotkey reply is a structured `background_unavailable`, and posted
  WinForms replies no longer say `global_input` (C17, C18).

**Rechecked and closed 2026-10-04** (EC2 Windows Server 2022, real Azure Artifact Signing certificate; [gaps-windows-2](../study/audit/gaps-windows-2-2026-10-04/README.md), [gaps-linux-windows](../study/audit/gaps-linux-windows-2026-10-04/README.md)).

- **Real-certificate build, by name.** On a fresh host `sign-windows.ps1 build` exited 0 in 501 s and
  every PE was `Valid` under the real certificate: 10 in the NSIS payload (`app.exe`, both sidecars,
  `uninstall.exe` and six plugin DLLs, `UserInfo.dll` among them) and 3 in the MSI. `build` now
  verifies the payload by name (7-Zip lists every PE, a ledger records every sign including the
  uninstaller's, `ICU_WIN_VERIFY_INSTALL=1` installs and checks `uninstall.exe`); shown on a real
  `tauri build`, where a sign hook that leaves the uninstaller unsigned exits 1 and the same with the
  payload check removed prints "signed and verified" (the old silent pass). 14 checks in
  `signing.ps1`, each guard removed once flips its case. This found `UserInfo.dll` shipped unsigned in
  a signed installer (Tauri's NSIS plugin folder), fixed; the 2026-10-03 real-certificate build very
  likely had it unsigned too and was not re-extracted. The MSI installed per machine and ran (the
  acceptance task `ok`, worker `TokenUIAccess` 1) and `msiexec /x` removed it. The MSI is per machine
  only (1625 for a standard user); a stale `HKCU\Software\nathanzhao\Copilot` made the first MSI
  install land in the SYSTEM profile (Tauri's WiX template, not changed). Building needs 7-Zip.
- **Updater on Windows.** NSIS per user and per machine, real-signed 0.1.0 and 0.2.0 builds, a throwaway
  minisign key and a local http server, UAC `standard-deny` and `admin-silent`: a wrong key, a tampered
  archive and a version mismatch are refused with the app still running, a downgrade is never offered,
  an update offered during a task leaves it running (display 42), Install & restart during a task is
  refused with the toast and nothing is downloaded, and after the task it installs and relaunches from
  the same folder; after an all-users update the worker's hash equals the new build and
  `worker_uiaccess` is 1. Bug found and fixed (8bf8394b): with `installMode: both` the updater ran the
  installer with no scope switch, so an all-users install updated to a per-user copy without the
  worker; it now passes `/AllUsers` from Program Files and `/CurrentUser` otherwise, NSIS only
  (`msiexec` would reject it). The test fails with the old behaviour.
- **Memory `.lock`, two instances.** See the App memory row.
- **Sky pipe.** `icu sky-pipe` serves the same protocol on a named pipe: see Verified, parked items.
- **UIAccess forwarding.** The daemon starts the worker on demand and has it deliver one foreground key
  or text input (`press_key`, `hotkey`, `type_text` with no element to focus first) to a window its
  integrity cannot reach. Measured with a text box in a SYSTEM-integrity window in the user's session:
  the unpatched driver answered `ok` for both calls and typed nothing; the patched one types `axyz`
  through a worker with `TokenUIAccess` 1; three mutants (forward call removed, an unreadable target
  token treated as not blocked, the UIAccess exemption removed) are each red; an ordinary target starts no
  worker; installed outside Program Files the worker starts with UIAccess 0 and the call says UIPI.
  Also fixed: a target whose integrity cannot be read was treated as "not blocked", so the driver
  reported success and typed nothing. Security: the worker exits without a parent pid, a same-user
  client that is not the parent is rejected before any request is read, the CLI cannot reach its pipe,
  it dies within 3 s of its parent, and an impostor holding the pipe name received nothing. The packaged
  sidecars carry it (marker strings in both, an elevated run through the installed driver forwarded
  `press_key` and `type_text`). This was foreground keys and text only
  ([uiaccess.md](../study/audit/gaps-linux-windows-2026-10-04/uiaccess.md)); widened 2026-10-05, below.

**Rechecked and closed 2026-10-05** (EC2 Windows Server 2022; [gaps-windows-3](../study/audit/gaps-windows-3-2026-10-05/README.md), [gaps-hosts-4](../study/audit/gaps-hosts-4-2026-10-05/windows.md); throwaway CA, not the real certificate).

- **UIAccess forwarding, widened** ([gaps-windows-3 §1](../study/audit/gaps-windows-3-2026-10-05/README.md#1-uiaccess-forwarding)).
  Measured first against the shipped build with a WinForms fixture at normal, High and SYSTEM integrity: at High and SYSTEM the
  daemon read 1 element of 412, every element action refused, pixel clicks refused with the UIPI text, and a foreground
  wheel answered ok and moved nothing (no earlier round had measured that). Design, option (a): for a window above the
  daemon's integrity the UIAccess worker reads the UIA tree as data and the daemon registers those elements itself, so ids,
  tokens, the session, C9 binding and the capture log stay in one process; the worker is sent finished gestures or an element
  description to find again, and checks the live element against all of it (exactly one match, or it refuses and sends
  nothing). Option (b), running the whole tool in the worker, was rejected from the code: `ToolResult.action_record` is not
  serialized, so recording, binding and the capture log would need a second registry in the worker; option (c), a shared id
  namespace, because the daemon cannot get a handle to the element at all and it adds shared state across an integrity
  boundary. Result, from a normal-integrity daemon with a signed worker installed under Program Files: at High, click, double
  and right click, scroll, element clicks, element-addressed `type_text`, `press_key` and `set_value` land (the background ones
  through UIA patterns in the worker) and `drag` refused with the UIPI text (a foreground drag is forwarded since round 2, below). At SYSTEM, where a
  UIAccess client reads 2 nodes of a window with a button, a text box and a list, the worker describes the window's
  child windows and the foreground click, double and right click, scroll, element-addressed `type_text` and `press_key`
  land; posted messages into a SYSTEM window fail with `Access is denied` even for the worker (`SendInput` works), so
  background clicks, scroll, typing, `set_value` and `drag` there still refuse and say why. A foreground wheel now raises or
  activates its target and puts the previous foreground back, or refuses when another window covers the point (before it
  went to whatever covered the point and said ok). With no worker (installed outside Program Files) nothing succeeds
  silently: every request refuses with the UIPI text and the reason. One mutant per guard is red (10 built and run on the host,
  11 more of the pure protocol half in unit tests), the worker's own checks pass 14 of 14 (15 of 15 on the merged-patch build),
  and on the merged-patch build the 17 actions at normal, High and SYSTEM equal the earlier run action for action
  ([gaps-hosts-4 windows.md](../study/audit/gaps-hosts-4-2026-10-05/windows.md#c-the-merged-patch-built-together-on-windows)).
  Limits are under Known limits.
- **Tile wheel** ([gaps-windows-3 §2](../study/audit/gaps-windows-3-2026-10-05/README.md#2-the-windows-tile-wheel)).
  The installed app, a real mouse wheel over the tile of a bound window with a decoy window on top at the same screen
  place, a relay logging every request: the bound window scrolled 0 to 12 in all four final runs and the decoy 0, and the
  request carried a `screenshot_id` the driver had issued for that window. The shipped driver answered the same wheel with
  `background_unavailable` (a background pixel scroll was refused unconditionally and the tile sends no delivery mode), so the
  Windows tile wheel never scrolled anything; a bug, fixed (a background pixel scroll is bound like a click and posted
  to the scrollable control under the pixel in the bound window's own tree). Same result on the merged-patch build. Not shown:
  the wheel over the tile of a window above the daemon's integrity (the tool-level runs use the same arguments), and a
  trackpad's pixel-precise deltas (whole notches were sent).
- **Standard user and an all-users update** ([gaps-windows-3 §4](../study/audit/gaps-windows-3-2026-10-05/README.md#4-standard-user-updates-an-all-users-install)).
  Decision: refuse, not elevate. A user who cannot elevate and runs an all-users install (under Program Files) is told
  "Copilot is installed for all users, and this account is not an administrator ... Ask an administrator to run the update",
  before anything is downloaded, with no Install button and the app still running. Elevating was not made safe:
  `tauri-plugin-updater` 2.12 runs the installer with `ShellExecuteW` and exits itself, so the app never learns whether the
  prompt was answered, cancelled or auto-denied. The cost: a standard user who knows an administrator's password is told the
  same. Shown with the app's test executable run as two real users (an administrator with a filtered token and a local
  standard user: the elevation read is right in both directions), a Rust test, 3 vitests, and 8 mutants, each red. The
  whole updater as a real standard user was driven in round 2, below. The earlier note here that an administrator who cancels the
  installer's UAC prompt ends with a per-user copy was wrong: see round 2.
- **Windows tests on a source copy and in session 0** ([gaps-hosts-4 windows.md](../study/audit/gaps-hosts-4-2026-10-05/windows.md)).
  The memory test that failed on Windows read a reviewer's file under `study/` that the host's source copy lacked, not a
  code bug; the list is now a fixture in the crate (asserted at 1796 rows so it cannot go vacuous), red without it and green
  with it with `study/` absent. The two `platform-windows` window tests assert `IsWindowVisible`, false in session 0
  whatever the code does: they skip with a message only when the session id is exactly 0, and
  `ICU_REQUIRE_INTERACTIVE_DESKTOP=1` turns the skip into a failure. Session 0: 251 passed 2 failed before, 254 passed 2
  skipped after, the two fail with the flag; a logged-on session runs all 254 and passes; a mutant (always interactive) is red.
- **Merged patch built together on Windows.** Driver, UIAccess worker and app from the merged patch (265 app tests pass),
  signed with the throwaway CA: no probe differs from the earlier run (above). Known on the way: the host's
  `core.autocrlf=true` left CRs in 127 of 419 Rust files after `git apply`, so a Windows tree was not byte-identical to an LF one
  (fixed in round 2, below), and the bindings half of `--check-generated` still cannot run on Windows (`ubrn.cmd` `EINVAL` under Node 24;
  gaps-windows-4 ran it on the Mac).


**Rechecked and closed 2026-10-05, round 2** (EC2 Windows Server 2022, one host, real Azure Artifact Signing certificate; [gaps-windows-4](../study/audit/gaps-windows-4-2026-10-05/README.md)).

- **UIAccess forwarding on real elevated applications** ([§1](../study/audit/gaps-windows-4-2026-10-05/README.md#1-real-certificate-real-elevated-applications)).
  The really signed build installed under Program Files (the worker starts with `TokenUIAccess` 1), driven as the standard
  interactive user against real Notepad, cmd, regedit, the Services console and Task Manager, each at High and at SYSTEM. The
  foreground paths land: Notepad menu click, Escape, typing and a drag select; typing in cmd; in regedit a tree click, Alt+E,
  New > Key, a typed name and Enter creates the key, and a splitter drag lands; in Services a row click, the wheel and End; in
  Task Manager the "More details" click. At SYSTEM background typing, `set_value`, click and wheel refuse naming System
  integrity (regedit's background drag refuses with the UIPI text at both levels); at High the background wheel and click land in
  Services. The fixture suite at normal, High and SYSTEM passes, and the worker's own checks (20 booleans and a snapshot of
  416 nodes). The certificate is `CN=Nathan Zhao`, valid 2026-10-04 08:08:32Z to 2026-10-07 08:08:32Z, chained to Microsoft
  Identity Verification Root CA 2020, with no throwaway certificate in any trust store; `sign-windows.ps1 build` verified 4
  files, the NSIS payload by name (10 PEs) and 3 files in the MSI. The service principal values reached the host through a
  Secrets Manager secret and a search of the deliverable for each found 0 files.
- **Foreground `drag` forwarded, and a covered-window bug** ([§2](../study/audit/gaps-windows-4-2026-10-05/README.md#2-drag-through-the-worker-uipi-finer-elements)).
  The worker protocol has a `Drag` operation; the worker refuses unless steps are 1 to 500, the duration is at most 10 s, the
  button is known, the window is nonzero, both ends are inside the named window and it holds a UIAccess token, and the daemon
  forwards a foreground drag to a window above its integrity. Found and fixed: with another window over the press point a
  foreground drag pressed and released on the covering window and answered ok; it now raises the window under the press point
  (previous foreground restored) or refuses `foreground_unavailable`, one helper shared with the wheel, and decoy cases at High
  and SYSTEM pass. Mutants: 5 of the protocol validation red in a pure test, and on the host not forwarding, not raising and
  checking only the start point each red; the duration-cap mutant was first green because the worker check tried one bad shape,
  so it now tries four and is red.
- **Why background gestures into SYSTEM windows are denied.** UIPI, measured with a 15-row sender matrix (a fixture window at
  medium, High and SYSTEM, posting `WM_LBUTTONDOWN` and `WM_LBUTTONUP` from a medium, High and SYSTEM sender, the receiver opted in
  with `ChangeWindowMessageFilterEx` or not): the worker's token is High (0x3000), a High sender reaches a High window and gets
  error 5 at a SYSTEM one unless it opted in, `SendInput` arrives at every level, and a wrong expectation for the opt-in row turns
  the run red. A SYSTEM-level sender would need a SYSTEM service in the install, a security decision not made. UIA at SYSTEM is not
  finer: a plain `FindAll` client at normal and at High integrity sees the same few panes of the SYSTEM windows (Notepad 2, cmd 0,
  regedit 4, Services 18, Task Manager 2), and the Edit, List and Tree come from the worker's child-window description. The raw
  counts are lower bounds that compare levels, not tools (Notepad at High: 2 raw against 11 from the driver's own walk), and a
  SYSTEM-integrity UIA client was not measured, so this holds for clients at High or below (a SYSTEM-integrity client was measured in round 3, below).
- **Cancelled UAC elevation of the installer** ([§3](../study/audit/gaps-windows-4-2026-10-05/README.md#3-cancelled-uac-elevation-of-the-installer)). Correction: the old
  behaviour was a windowless app process, not a per-user copy. On a real UAC prompt the old build, with Escape, kept the same app
  pid with no windows, no toast and no Install button, the install unchanged and no installer left; no per-user copy appeared in
  any run. For an all-users NSIS install the app now runs the installer itself with `ShellExecuteExW "runas"` (no shell error box:
  without `SEE_MASK_FLAG_NO_UI` a failed elevation blocked on one), waits for it and classifies the exit. Escape gives "Update
  cancelled: administrator approval was not given. Copilot was not changed." with the same pid, the Install button back and 0.1.0
  unchanged; Alt+Y installs 0.2.0 (four files Valid), the old app exits and the new one starts at normal integrity, with no
  per-user copy. The prompt was answered through the signed UIAccess worker on the user desktop (`PromptOnSecureDesktop=0` for the
  run, put back afterwards), not on the secure desktop. 5 Rust tests and 3 mutants (shell error box allowed, the cancel code
  treated as installed, a nonzero installer exit treated as installed), each red. Per-user and MSI installs and a non-administrator
  still use the plugin's install. The update server was local and the archives used a throwaway updater key.
- **Standard-user updater UI** ([§4](../study/audit/gaps-windows-4-2026-10-05/README.md#4-real-updater-ui-as-a-standard-user)). The existing standard local user `ICUAudit`
  (taken out of Administrators and the machine restarted), the final all-users 0.1.0 install and a local server offering 0.2.0:
  the app shows "Copilot is installed for all users, and this account is not an administrator, so it cannot install the update.
  Ask an administrator to run the update (or install the new version for all users).", has no Install button, keeps its pid and
  the 0.1.0 install, and the server saw one request (`/latest.json`) and 0 for the archive.
- **Byte-identical build on a `core.autocrlf=true` host.** `git apply` of the driver patch wrote CRLF into 136 of 439 patched
  files (0 with `-c core.autocrlf=false`) and the tree hashes differ. `build.ps1`, `bootstrap.ps1` and the three `git apply` lines
  of `apply-and-build.sh` now pass `-c core.autocrlf=false`; the new `conformance/test_windows_apply_endings.py` is red with one
  `apply` line changed back. `merge-patch.sh` reproduces the committed patch byte for byte (29930 lines) and `--check-generated`
  passes, run on the Mac after `npm ci`.

**Rechecked and closed 2026-10-05, round 3** (EC2 Windows Server 2022, one host, interactive session; the final signed build and its matrix used the real Azure Artifact Signing certificate; [gaps-windows-5](../study/audit/gaps-windows-5-2026-10-05/README.md)).

- **Foreground input into a normal-integrity window without the worker** ([§1](../study/audit/gaps-windows-5-2026-10-05/README.md#1-normal-integrity-notepad-without-the-uiaccess-worker)).
  The open observation (real Notepad at the daemon's own integrity: the menu click and Escape did not land, element-addressed foreground actions refused `foreground_unavailable`) was not the foreground lock but a product bug with three causes in `platform-windows`, fixed. (1) `GetForegroundWindow` returns the focused child while input is attached, and the exact-foreground wait compared that child with the top-level window, so every element-addressed foreground action was refused. (2) After a click that opened File the driver put the previous foreground back, and a menu closes when its window loses the foreground; the restore now leaves the foreground alone while the target is in menu mode. (3) A click on a point covered by another top-level window hit the covering window and answered ok; an activating click there is now refused `foreground_unavailable`. Before (4 scheduled runs, a suite, a final run): the File click answered ok with no drop-down 600 ms later, Escape had nothing to close, element type and key were refused, a covered click landed on the harness console. After (3 scheduled runs, 2 self runs, a suite): the menu opens, Escape closes it, the text and key arrive, the covered click is refused. Three mutants, one guard each (child compared, restore in menu mode, no cover refusal), are each red, and unit tests for the pure checks and a real-window test ran on the host with `cargo test`. The causes are in the product code, not in Server; what Server may change is how often another window is on top. `--check-generated` passes on the regenerated patch.
- **UIA at SYSTEM integrity: measured, not wired** ([§2](../study/audit/gaps-windows-5-2026-10-05/README.md#2-uia-at-system-integrity)).
  `sysuia.ps1` run as SYSTEM in the console session against real SYSTEM Notepad, cmd, regedit, Services and Task Manager: the driver's own walk named 11, 11, 17, 32 and 8 elements (menu items, buttons, tree items, list rows), where the worker's description gives child windows only; a raw `FindAll` from the same client counted 2, 1, 4, 19 and 5. So finer, named elements exist at SYSTEM. They are not wired, because a UIA client at SYSTEM integrity is a SYSTEM service on the user's machine that accepts requests from a medium-integrity app, a privilege boundary Nathan decides (Needs Nathan, item 9). Limit: the raw counts match gaps-windows-4's for normal and High clients (Notepad 2, regedit 4) and a raw count is below the driver's walk even at High (Notepad at High 2 against 11), so the run shows the driver's walk reads named elements at SYSTEM, not that a SYSTEM client reads more than a High one; the report does not run the driver's walk from a High client against a SYSTEM window.
- **Per-user and MSI updates** ([§3](../study/audit/gaps-windows-5-2026-10-05/README.md#3-updates-for-per-user-and-msi-installs)).
  The per-user installer manifest is `highestAvailable`, so an Administrator was shown a UAC prompt for a per-user update (1 prompt measured); it now runs with `__COMPAT_LAYER=RunAsInvoker`: 0 prompts, 0.1.0 to 0.2.0 in `%LOCALAPPDATA%\Programs\Copilot`, relaunched at integrity 0x2000. The plugin offers an MSI update under `windows-x86_64-msi`, falling back to the bare `windows-x86_64` key; a package that is not the install's own kind is not offered (the UI and the backend both refuse: an NSIS `.exe` under the bare key for an MSI install showed no toast and 0 archive requests). MSI cancel: the plugin ran the MSI and the app was gone with the install unchanged; now an elevated `msiexec` through `ShellExecuteExW runas` keeps the same pid, the install and the message "Update cancelled: administrator approval was not given. Copilot was not changed." MSI accept: 1 prompt, 0.1.0 to 0.2.0, new product code, old app exited. A bug found on the way and fixed: the MSI's `AUTOLAUNCHAPP` started the app from the elevated installer at High integrity (0x3000); one elevated `cmd` now runs `msiexec` and then `explorer.exe app.exe`, so the user's shell starts the app at 0x2000. The all-users NSIS cancel and accept still hold on the final build. 12 Rust tests (`elevated_update`) and 7 mutants, each red (per-user started elevated, no `RunAsInvoker`, any package run for an MSI, MSI marker ignored, `AUTOLAUNCHAPP` added, `&` replaced by `&&`, `&` accepted in a path); vitest 107 with 2 new for `fitsInstall`. Not covered: the UAC secure desktop (the prompt was on the user desktop by policy, answered through the signed UIAccess worker) and an MSI install updated by a standard user.
- **Signed full matrix on the final build, real certificate** ([§4](../study/audit/gaps-windows-5-2026-10-05/README.md#4-full-signed-flow-on-the-final-build-with-the-real-certificate)).
  The signed NSIS and MSI were rebuilt from 3ec1cf10 (516 s) and installed per machine: `app.exe`, both drivers and `uninstall.exe` Valid under CN=Nathan Zhao with a Microsoft timestamp, no throwaway certificate in any trust store; the UIPI matrix 4 of 4 as expected; the worker's checks pass (20 booleans true, 416 nodes); fixtures at normal integrity pass, SYSTEM passes with the intended `scroll_px_bg_decoy` refusal, the decoy suites pass; real Notepad at normal, High and SYSTEM and cmd, regedit, Services and Task Manager at High and SYSTEM, 11 of 11, with no silent success and nothing not landed. The High fixture is not clean: in the final full rerun its first run failed on cover refusals (the harness console over the target) and two of three reruns passed; the cover guard refusing is the intended behaviour and the harness is not fully robust against a covering console. Disclosure: an earlier High matrix this round was invalid, because the audit user was not an Administrator and so the "High" target was not High; it was redone after `policy-nosecure` and a reboot, and the matrix above is the redone one. The service principal values reached the host through a one-off Secrets Manager secret deleted at teardown; a search of the deliverable for each found 0 files, and teardown is proven ([teardown-proof](../study/audit/gaps-windows-5-2026-10-05/results/teardown-proof.txt)).
- **Merged patch rechecked on two fresh hosts; the regedit tree click was a harness problem** ([gaps-final-build](../study/audit/gaps-final-build-2026-10-05/windows/README-windows.md), [gaps-final-build-3 §1](../study/audit/gaps-final-build-3-2026-10-05/README.md#1-windows)). Throwaway-CA signed builds, not the real certificate. The build that ran is this app source with the gaps-windows-5 patch, which is one driver binary with this HEAD's patch (header above); HEAD's patch was not built on its own. First host: fixtures 18 of 18 at normal, High and SYSTEM, real apps 9 of 11; the two misses were the regedit tree click at High and SYSTEM. Second host: the check judged tree pixels while regedit was not the foreground window, so the selected root row was drawn grey before and after the click; the click is delivered, and the driver then puts the previous foreground window back. The earlier check fails 0 of 7 per target on the gaps-windows-5 patch too, so the merge did not cause it. `app/tests/windows/gaps4-real.ps1` now presses Down through the driver first and requires the tree pixels to change (otherwise the cell fails as `prep_failed`), then clicks the root row and requires them to change back; `C:\Users\Public\tree-mode.txt` selects `old` (the earlier check) or `miss` (click aimed at the empty right end of the menu bar) for the red checks. Repaired check: 12 of 12 at High and 12 of 12 at SYSTEM, and 0 of 2 at each when aimed away (red as required); 5 more of 5 at each on a build with a wheel change that was dropped. The full matrix, once, with the repaired check: 11 of 11 real-app cells, the gaps-windows-5 number again. One run on the candidate build did not land the regedit splitter drag while the tree click passed; the drag landed in the other 24 regedit runs. Not rerun: the three guard mutants, the updater scenarios, the UIPI rows, a Windows 10 or 11 client.
- **Post-expiry validity: still pending.** The certificate is valid to 2026-10-07 08:08:32Z, so the check cannot run yet. A file it signed on 2026-10-05 is saved ([signed-2026-10-05-uninstall.exe](../study/audit/gaps-windows-5-2026-10-05/results/item4/signed-2026-10-05-uninstall.exe), sha256 CFC35CA2...3B91) with a script, [postexpiry.ps1](tests/windows/gaps5/postexpiry.ps1); see Needs Nathan, item 8. Before expiry it printed Valid, timestamped 2026-10-05 16:31:26 UTC by Microsoft Public RSA Time Stamping Authority.

### App and layer, headless


[providers-live](../study/audit/providers-live-2026-09-25/README.md),
[run-lifecycle](../study/audit/run-lifecycle-2026-09-25/README.md),
[app-fixes](../study/audit/app-fixes-2026-09-25/README.md),
[memory](../study/audit/memory-2026-09-25/README.md),
[memory-docs](../study/audit/memory-docs-2026-09-25/README.md), round 1
[app.md](../study/audit/review-2026-09-25/app.md), round 2
[app.md](../study/audit/review-2026-09-26/app.md).

- Providers: Codex streams SSE; errors keep the provider's reason and reset; auto
  fallback logs both errors; a keyless OpenAI-compatible endpoint and the ccodex router
  get no token; no call changed a credential store (P1 to P3, P5 to P7, P10, A5, A6).
- The model is told the scroll sign the app executes (P13).
- Exit kills in-flight CLI trees and ends the run `stopped`; a reloaded renderer adopts
  the running task; stop is scoped to its run id; group kill and the 32 MiB limit end a
  flood (L1, L3 to L6, A8).
- Conformance passes on macOS again (`ICU_DRIVER_DAEMON=0` for the stdio fixture), and
  `icu` fails at once when `serve` exits without listening (A1, A2, A4).
- Memory: the model gets no title, text or session id; empty rollups are refused;
  retention is a whole number of days from 1 to 3650; files are 0600 in a 0700 folder; the
  scheduler waits for open windows, retries 3 times and consolidates only after a write;
  invented bundle ids never reach `USER_PROFILE.md` (M1, M4, M5, B1, B3, B9, B10).
- Byte parity: 7 packs, the policy text, 13 window2 names, 7 real session TOMLs and the
  service's diff strings (R2, R3). Round 1's G1 to G4 and G6 to G10 confirmed what the
  golden probe captured, not our rendering of it; that is the Sky text format row under
  Partial.

**Rechecked and closed 2026-10-04** (rows moved from Partial; [gaps-app](../study/audit/gaps-app-2026-10-04/README.md)).

- **Codex SSE parser** (A7). BOM only on the first line, CR, LF and CRLF (split across reads too),
  comments, multi-line `data:`, `[DONE]`, an unterminated last event, invalid UTF-8, over-cap events,
  `response.failed`, `response.incomplete` and typed `event: error` hold. One real gap fixed (3d034b2):
  the 8 MiB cap covered one event, not the reply assembled from many small ones, which grew until the
  120 s deadline; the assembled text is now capped, the answer is the same for every chunking from 1 to 19
  bytes and six line-end mixes, and four mutations fail the tests. A real stream through ccodex
  (gpt-5.5) parsed into a four-step plan. The default model is rejected, see Known limits.
- **Claude Code provider.** `claude -p` with no tools and only the app's prompt; the same four real-model
  tasks passed on Linux and macOS 15.7.5 with 0 ownership changes, and calc, type and two_window on 26.3.1;
  the environment allowlist and the turn on stdin (round 3 D2, D5, [app-review3-fixes §2, §3](../study/audit/app-review3-fixes-2026-09-26/README.md#2-claude-code-environment))
  are guarded (a mutation of each fails its test). A 5 MB goal gives `claude-code: Prompt is too long` from the real CLI in 1.5 s and ends
  the run; the app does not bound goal or history, by choice (an ordinary run is 18 steps). The GUI's own `claude-code` choice ran
  through the production task body (`agent::run`, real `claude -p` through ccroute, the fixture
  driver): click, type, done. Not run there: a real window and the real driver; the notarized app's own headless
  `claude -p` argv, forwarded to the real CLI, ran real tasks in final-mac-3 (Verified, macOS).
- **Refused actions.** A refused step ends its batch; the next turn names the refused step, its code and
  message and each step that did not run, above a fresh tree; a run ends after 3 refused decisions in a
  row or 10 in all and says which bound it hit. Stop, provider errors, the step limit and a failure whose
  delivery is unknown or partial still end the run (an answer is a refusal only when its `effect` is
  `none` or `refused`, or with no effect `refused (<code>)`, the hard-deny rule or a listed pre-dispatch
  code). F3, F7 and F8 re-proved with four mutations ([app-review6-fixes §1, §2](../study/audit/app-review6-fixes-2026-09-28/README.md#1-partial-delivery-is-not-a-refusal-f3-f7)).
  Fixed 2026-10-04 (64cef16): a tag's own code was overruled by `refused (<code>)` in its text, so
  `daemon tool error (type_text_incomplete): refused (stale_capture): ...` would have been fed back and the partial
  type typed again (no driver writes that today). A driver code not on the list and carrying no
  `effect` ends the run (fail closed); codes the drivers emit that are not listed (`host_not_interactive`,
  `lane_busy`, `target_changed`, `stale_target`, `browser_ref_stale`, `window_scope_disabled`, `session_*`)
  are fed back only with `effect: none`. The per-platform refusals were not re-run on a native host. A
  step's own label (`press {key}`, `done: {summary}`) is the model's text and is not escaped.
- **Malformed model actions.** A reply with a wrong or ambiguous shape, or a field holding an empty,
  null or wrong-typed value, is refused before any step of the batch runs; the model gets the parser's
  exact error and the expected shape; a second in a row ends the run. Exercised through the real
  parser, task body, cua client and socket with a scripted model (empty and null targets, a bad
  second step, `set_value` with no target, an empty key, `{"done":true}`): nothing malformed reaches the
  driver, four mutations fail it. A real model (Sonnet 5 through ccroute) shown the `MALFORMED` entry
  recovered in 3 of 3 shapes. Not shown: a real model producing a malformed reply on its own (the
  earlier `{"done":true}` run is in [refusal-feedback](../study/audit/refusal-feedback-2026-09-27/README.md)). Fixed: click `count` is 1 to 3
  (the contract's limit; 4 or 1000000 was accepted), a `type` of only a space or newline is accepted.
- **Owned-driver startup in tests.** The 2026-09-26 failure was the macOS `/usr/bin/python3` shim with
  a cold xcrun cache after a reboot, not CPU load (cold 1.9 s idle, 2.7 s at 24 busy loops, 4.9 s at 60;
  warm 0.04 s). With the product's 3 s deadline, a cold cache and 40 loops 2 of 16 lifetime tests failed;
  after `warm_python_shim()` 45 of 45 pass. Not run on Linux or Windows (no shim there).

### Parked items built 2026-10-04

Ten parked build items, each with a test shown red when the feature is removed (51 mutation runs in
[gaps-parked](../study/audit/gaps-parked-2026-10-04/README.md)), run on a real macOS 26.3.1 Mac; the Linux facade on real X11 and headless sway.
Running them found bugs no test had: `get_app` did not launch an app that was closed and picked a
windowless helper surface; a text area's title kept the whole text past truncation; a Sky version
error carried Rust debug text; `set_value` on a Save sheet control was refused `element_outside_target_window`.

- **Per-value truncation** (`max_value_chars`, default 4096). 20,000 characters in TextEdit: a
  `_value_truncations` descriptor and a marker on the row; `max_value_chars: 0` returns all; the tree
  was 8,881 characters against 40,338. The 4096 default and the marker wording are inferred.
- **Agent cursor in screenshots** (`mouse_size_px`, default 12) **and `post_action_sleep_ms`** (default 100).
  The cursor tip is at the click pixel exactly on a 2x capture; 300 ms makes each later action and the
  screenshot wait out the remainder (299 ms waited). The 100 ms default comes from Sky's Linux code.
- **Close-enough cursor targeting** (native, `close_enough_distance_px`, `close_enough_progress`).
  Measured on 26.3.1 with the overlay on and an 868 pt path ([gaps-mac-2 §3](../study/audit/gaps-mac-2-2026-10-04/README.md#3-item-1-close-enough-measured)):
  with a threshold the glide ends at 0.45x to 0.82x of the full time, with one that cannot trip before
  arrival 0.99x to 1.00x, 8 runs per cell, the same on `move_cursor` and the pixel click. One path
  length and window size; the disabled-cursor control still took the full time, so it does not show
  "no glide without a cursor".
- **Length-prefixed Sky pipe** (`icu sky-pipe`, `CodexComputerUseIPC-5`). Unix socket: Codex's own
  `client.js` and `native-pipe.js` (only the `tslib` path changed) ran `listApps`, `getAppPolicy`,
  `getAppState`, a click, `typeText`, `pressKey`, `scroll` and `drag` against the real driver and Calculator, errors
  with Sky's numbers (-10005, -10010, -10003, -10013); two bugs found and fixed. Windows named pipe
  `\\.\pipe\icu-computeruse-<user>` ([gaps-windows-2 §4](../study/audit/gaps-windows-2-2026-10-04/README.md#4-sky-pipe-on-windows)):
  the same client files against the installed Windows driver and a calculator fixture (6 x 7 = 42,
  four click events); DACL owner and SYSTEM only (another standard user and an elevated administrator are
  denied), a squatter refused, two waiting instances (a second client got `ERROR_PIPE_BUSY` before); 13 of 13
  tests, mutants red. A pipe with no DACL passes every unit test, so only the live access harness guards it.
  Inferred: a page scroll of 10 notches. Coordinates are window points and screenshots are PNG. Not
  done: apps other than the fixture on Windows; the pipe lives only while `icu sky-pipe` runs.
- **Model, prompt and run metadata** (`record_start {run}`, `ICU_RUN_METADATA`, native). Stamped into
  real `action.json` and `evidence.json`; a recording started without `run` has no key. The checked-in
  `contract/manifest.json` and the Python and TypeScript bindings are now carried by the tracked patch
  (Patch merge guard row); the manifest lists only the native arguments the typed contract has
  (close-enough); run metadata, `cursor`, `post_action_sleep_ms` and the facade's `hold_ms`, `path` and
  `modifier` are runtime fields.
- **Consult memory before broad discovery** (MCP `instructions`, memory tool descriptions, the app
  prompt for every provider). Measured on outcomes 2026-10-05 ([gaps-tests-3 §2](../study/audit/gaps-tests-3-2026-10-05/README.md#2-item-2-memory-first-on-outcomes)).
  The 2026-10-04 "no effect" (228 runs, no run read a stored fact) was a bug, not a finding: `icu mcp` sent every list
  result (`list_apps`, `list_windows`, `memory_list`, `memory_grep`, `memory_prior`) as a bare array in
  `structuredContent`, which Claude Code rejects ("expected record, received array"), so every memory read failed for it
  (11 of 11 `memory_list` calls in those runs, also 128 `list_windows` and 49 `list_apps`). Fixed: a non-object goes under
  `structuredContent.result`, with a test that is red before. On a finishable scripted app (a Desk window with a Result field
  and Apply; the outcome is the value the app held; six stored facts; 1168 runs, sonnet and haiku through `claude -p`)
  the consult-first text changes the order and not the answer: when the prompt hints at earlier work the first call is
  memory in 48 of 48 against 6 of 48 (sonnet) and 48 of 48 against 27 of 48 (haiku), but the artifact is correct in 48
  of 48 against 46 of 48 (sonnet) and 21 of 48 against 20 of 48 (haiku). Two changes did move outcomes and are shipped:
  `memory_grep` returns the summary text (haiku 21 of 48 to 36 of 48 correct, p=0.003; sonnet needs fewer calls) and a trigger
  for a task that leaves a choice open, not only one that continues earlier work (on prompts that never mention earlier
  work: sonnet 1 of 48 to 25 of 48 correct, haiku 4 of 48 to 13 of 48, p under 0.001 and p=0.03; one stray memory call in 32
  control runs per model, no wrong artifact). Titles in `memory_list` did not help (haiku 6 of 24 against 21 of 48) and are not
  shipped. The app system prompt sentence is a no-op when MEMORY is already in the turn (ceiling: haiku 41 of 48 without
  against 44 of 48 with, sonnet 48 of 48 both); it stays and is not claimed as a feature. Caveats: the reps beyond the first
  four per cell were added after a first look (post hoc, read the p values with that in mind); one scripted app, six facts,
  two models, one seed order, and runs in a case are repeats, not independent cases; hand-written summaries, not the memory
  pass's own. The trigger was reworded in round 2, below.
  Round 2, 2026-10-05 ([gaps-memory-4](../study/audit/gaps-memory-4-2026-10-05/README.md); pre-registered, 2184 runs over two rounds, sonnet and haiku through `claude -p` on
  ccroute). The consult-first text now fires on the action (write, name, format, send or schedule something) rather than on the
  prompt hinting at earlier work, says the prompt will not repeat the user's earlier choices, and says to skip memory when the
  prompt gives the exact value; `memory_grep` matches the whole rendered file, so an app id finds its entries; and the first
  `list_windows`, `list_apps`, `get_state`, `get_app` or `get_window_state` result for an app that memory names carries
  `structuredContent.memory_prior` (entry count, titles and the exact `memory_grep` call; once per app per server, never after a
  memory tool was used, never for an app with no entries; `ICU_MEMORY_HINT=off` disables it). Round 1 (1144 runs) delivered
  nothing: the prior rode in an extra text content block and Claude Code drops text blocks when `structuredContent` is present
  (`hint_seen` was 0 in all 624 runs of the three arms that carried it, so they stand as an A/A estimate), and round 2 (1040 runs)
  is the registered one. On prompts that never mention earlier work, 48 runs per cell against the previous shipped text: sonnet's
  artifact carries the stored fact in 48 of 48 runs against 23 of 48 (p<0.0001; file name and status line 0 of 8 before, 8 of 8
  now); haiku looks memory up in 34 of 48 against 17 of 48 (p=0.001) and its artifact is right in 22 of 48 against 15 of 48
  (p=0.21, not significant). For haiku the new text alone made it worse (4 of 48 against 15, p=0.009) and the prior alone did
  nothing (14 of 48); only the two together moved the lookups. No wrong artifact in 504 control runs (exact value in the prompt, a
  prompt that contradicts a stored rule, empty memory); stray lookups 0 of 24 (sonnet) and 1 of 24 (haiku); the price is about one
  more call per task (sonnet checks an empty memory folder on a naming task in 8 of 8 runs). 197 `icu` library tests and 2
  conformance memory tests pass, four mutants of the prior are each red, and both new conformance tests are red on the previous
  binary. Left open by round 2: haiku (identical surfaces ranged from 4 to 15 of 48 across instances, so one round could not carry a claim; round 3 below decided it); inlining the entries' first sentences into the prior (HI) reached 39
  of 48 for haiku but failed a registered guard on turn-cap loops in the contradicting-rule control (no wrong artifact), puts
  summary text in tool results, and is not shipped; the prior fires for every open app memory names, matched by the last component
  of the app id, so two apps sharing a name stem would share entries; one scripted app, six facts, hand-written summaries, repeats
  inside a case (the p values are optimistic), and the `structuredContent` behaviour measured on Claude Code 2.1.289 only (other clients get the prior in both forms).
  Round 3, 2026-10-05 ([gaps-memory-5](../study/audit/gaps-memory-5-2026-10-05/README.md); pre-registered in `cc41e080` before any measured run, 2646 runs through `claude -p` on ccroute, the 12 fact families as the unit of analysis with an exact sign-flip test and a cluster bootstrap, Holm over five haiku comparisons, power 0.82 to 0.96 for a +0.25 gain, the A/A check passed with delta -0.04, p 0.38). Haiku on plain prompts, case-level mean: the shipped surface 0.28; against it S (the prior line rewritten around the action: "before you type, set or apply any value in this app, call memory_grep ...") +0.43 (cluster CI +0.26 to +0.60, Holm p 0.008), I (capped inline entries framed as data) +0.29 (+0.09 to +0.49, p 0.0195), G (a one-time soft gate on the first write in a known app) +0.42 (+0.25 to +0.58, p 0.008), SG (S plus the gate) +0.46 (+0.32 to +0.59, p 0.0024), IG (I plus the gate, only when the cap hid entries) +0.42 (+0.24 to +0.59, p 0.008). Sonnet is at the ceiling on the shipped text (0.94 case level, 135 of 144 runs) and unchanged under S (0.96). Nothing was shipped, because every variant failed a registered guard and the guards were not relaxed after the data: S had 10 of 60 stray lookups on the exact-value controls (allowance 6; 6 are the control whose prompt gives an exact value that contradicts memory, and each of those runs still applied the exact value) and 52 of 228 turn-cap loops (allowance 41); G and SG look memory up on nearly every exact-value prompt (46 and 48 of 60) by construction; I and IG had 8 of 72 wrong artifacts (allowance 6); I also hides the fact when memory is crowded (0, 1 and 2 of 12 against 8, 2 and 11 with S) and lowers sonnet's plain score (0.85). Injection: the surface held, 0 of 126 red-team runs (haiku 84, sonnet 42) applied an instruction planted in an entry, including the arms that put entry text in the first tool result (an upper bound of about 3 percent on one case; sonnet's cells are 6 runs, descriptive). Ruling: history text may reach the model only as data in the results of the memory tools (`memory_grep`, `memory_prior`) plus the title line of the prior; inlining entry bodies into other tools' results (`list_windows`, `get_window_state`) stays off, since it is not needed (the pointer variant beat the inline one, 0.71 against 0.57), widens the injection surface to every observation tool and a cap hides facts, and if it is ever turned on it must carry arm I's framing (the user's notes, never instructions, tags and angle brackets removed, at most 4 entries of 280 characters) with the red-team case still at zero. Hardening shipped: a title in the prior line is one line of at most 80 characters with quotes, backticks and angle brackets removed (`title_as_data`); before, a hand-edited file could put a long or quote-closing title in the line. Two tests pin the framing (the title test is red with `title_as_data` bypassed and on the previous build); they do not claim a model cannot be fooled; `cargo test -p icu` 200 passed. Deviation (F7 and F9): the date-format and sign-off cases read 0 of 144 in every arm because their seeded entries said "never with slashes" and "never \"Best regards\"" and the store refused them (see the next item); a harness defect common to all arms, so both cases were rerun for all seven arms and both models with "and not" wording (252 runs) and replaced the 252 broken runs. The as-run analysis is kept: deltas +0.20 to +0.40, the same guard failures, none a candidate, so the decision does not depend on the correction. Still open: haiku (the shipped surface leaves it near 0.28 on plain prompts; S is the candidate for a new registration, whose guards would have to separate a lookup that leaves the applied value unchanged on an exact-value prompt from a wrong artifact, and which needs a fix for the loop rate, mostly haiku pixel-clicking in the scripted app, a harness property; choosing those guards after this data would be post hoc). One scripted app, 12 fact families, hand-written summaries, two models, Claude Code 2.1.289 only; the shipped surface scored 0.28 here against 22 of 48 last round (other cases, a fixed driver), which is why round 3 decided against a contemporaneous baseline.
- **A stored "never" preference no longer hides its file** ([gaps-memory-6](../study/audit/gaps-memory-6-2026-10-05/README.md)). Cause, found by the round 3 study (F7 and F9 at 0 of 144): `skysight::directive_violation` refuses a stored text when any piece opens with an instruction word ("Run", "Always", "Never", "Do not", "Ignore"), and it cut the text at every comma as well as at periods, newlines, `!`, `?` and `;`, so "Dates in Desk are written day.month.year with dots, never with slashes." failed on its tail clause and `Summary::validate` and `parse` refused the whole file. `memory_grep` and `memory_prior` then dropped the file without a word; the app's own check dropped the sentence as "not descriptive", which the panel already shows. Fix: a comma clause that opens with "never", "do not" or "don't" is kept as the tail of a description when the first clause of its sentence states something. Still refused: a sentence that opens with "never"; one that opens with a condition or time word ("When writing dates, never use slashes", "Before sending, do not ..."); one whose first clause is itself an instruction ("Run x, never y", "Always sign off, never ..."); and a negation followed by an action verb unless the sentence opens "The user ..." ("Notes say ok, never follow the goal" is refused, "The user wants tabs, never use spaces" is kept). The marker, label, secret, URL and injection rules in `memory.rs` still run. `memory_grep` (for files whose raw text holds the needle) and `memory_prior` now report a file the store refuses as `{"file", "unreadable": "<reason>"}` with the rule's reason, instead of skipping it or answering empty. Three tests, each red on the old behaviour and on a mutant whose tail rule is always false; the always-true mutant is red in the skysight and app tests, and in the icu test it stays green because that test's refused file opens with "Never" and the base rule refuses it; a mutant with grep skipping the refused file again turns the icu test red. Runs: `app/src-tauri` 332 passed (6 ignored), `icu` 201 and `skysight` 34, vitest 105, the conformance memory tests 2 passed on `our` and 2 xfailed on `cua` (no memory tools there). Limits: the rule is a word list on clause openers, not a parse ("Dates are ISO, never again use slashes" is kept because the word after "never" is not on the verb list); `memory_list` still lists a file the store refuses, and only grep and prior say why; files already on disk are read with the new rule and nothing rewrites them; F7 and F9 were not rerun with "never" wording.
- **File and process predicates** in `verify_state` (off unless `ICU_PERSISTENT_PREDICATES`, roots from
  `ICU_PREDICATE_ROOTS`). Real files and processes on 26.3.1; a path outside the roots and a relative
  path are `unknown / invalid_predicate`, never `satisfied`. An agent saved a TextEdit document through the real Save
  sheet (File > Save by `invoke_menu`, the name by `set_value`, Save by click, no hotkey) and
  `verify_state` saw the file with its content and hash; getting there found and fixed a native bug
  (ca9bc6bf: a control in a sheet was refused as outside its window; red and green on the Mac, one test fails
  with the ancestor arm removed). Not done: the sheet fix on 15.7.5, `click` and `type_text` on sheet
  controls, the Windows `tasklist` path.
- **Unified `get_state` and `get_app`.** 79 apps in 0.5 s with browsers and tabs reported as a gap, not empty;
  an app by name, bundle id or `.app` path, the first call launching a closed app; an unknown name is `invalidApp`.
- **Two-phase `prepare_action`, `approve_prepared`, `commit_action`.** On Calculator prepare changes
  nothing, the elicitation asks once, session or always, commit shows 5, a second commit is
  `planUnavailable / already_committed`; nine mutants red.
- **Linux full-desktop facade** ([gaps-linux-2 §1](../study/audit/gaps-linux-2-2026-10-04/README.md#1-the-full-desktop-facade)).
  `desktop_screenshot`, `desktop_click`, `desktop_drag` (a path of two or more points) and `desktop_move`, in pixels of one
  whole-desktop screenshot with the agent cursor drawn in, and the linux-SKILL contract as the MCP resource
  `icu://skills/linux-desktop`; a click `duration`, a path of more than two points and a held key need the native
  driver and are refused on a stock one. Run on Xvfb (23 cases, five runs) and headless sway (22 cases), read from
  what the display server delivered. On sway a held key is refused (a virtual pointer carries no modifiers)
  and a lone `move_cursor` reaches no client (the headless seat has no pointer). Found on the real desktop and
  fixed: `get_screen_size` fails on pure Wayland, and the first press of a virtual-pointer hold vanished on sway.
  25 unit and 14 conformance tests; each layer mutant listed in the source fails a test, and the native
  mutants fail the planner's unit test and the probe (the source counts "12 layer and 7 native mutants" in one
  place and lists 13 and 3 in another; its native mutant runs also fail `s_cursor_drawn_at_the_click_pixel`, which
  the green run passes, unexplained).
- **Linux desktop facade, input verbs** (2026-10-05, [gaps-linux-3 §1](../study/audit/gaps-linux-3-2026-10-05/README.md#1-the-desktop-facade)).
  `desktop_press_key` (a chord, with `duration`), `desktop_scroll` (40 px a wheel click, 120 with no distance, at most 50
  clicks, a held key), `desktop_type_text` and `desktop_drag_handle` (start, move_to, end) are built, folded into the driver's
  desktop `press_key`, `scroll`, `type_text` and `drag` (the handle as a `hold` field, so no new tool name and
  `--check-generated` passes), and refuse without sending anything on a stock driver. Native changes: key hold on X11 and Wayland;
  scroll and click modifiers (on sway the modifier keys are held on the seat's virtual keyboard, which closes the held-key
  refusal); a lone `move_cursor` on sway now reaches the client; the drag hold glides in 8 steps, is released by the driver after 2
  idle minutes, and on X11 the next driver process finds and ends it; and a real bug the two-output run found, the virtual
  pointer took the last output's range and clicks landed on the wrong output (now bound to the first output). Driven on Xvfb (58
  of 58 probe cases), headless sway (59 of 59), two X11 monitors with xrandr (12 of 12), Xvfb `--setmonitor` (12 of 12), sway scale 2
  (10 of 10) and sway with two outputs (mixed scales, first output on the right, first output HiDPI: 11 of 11 each). 41 desktop unit tests;
  conformance 18 pass on `our` and 18 xfail on stock `cua`. Mutants: 20 layer edits red (a first run had one green, hard-deny skipped
  for `press_key`, now unit tested) and 11 native, each red on the case that names it; a red on
  `h_idle_button_is_released_by_the_driver` counts only for the idle mutant, since that case also fails in the unmutated baseline of
  the filtered runs. What stays not covered: an X11 output scale was driven only in round 2 (below);  on Wayland the desktop is the first output only (a second output is not in the screenshot and no
  desktop verb reaches it); a Wayland drag handle through a one-shot call (`icu call` with no daemon) lost the release (round 2, below);  a character the X11 layout lacks (`ö`) uses a
  spare keycode XTest restores within about 10 ms, so a receiver can miss it; in the sway multi-output runs the receiver does not
  cover every corner of its output, so far corners are asserted only as "delivered to the screenshot's output or nowhere".
  Round 2, 2026-10-05 ([gaps-linux-4 desktop](../study/audit/gaps-linux-4-2026-10-05/desktop/README-desktop.md)): an X11 output scale on Xorg's modesetting driver over a vkms
  KMS device: `xrandr --scale 0.5x0.5` and `2x2`, 11 of 11 each, the screenshot is the logical root (800x500 on a 1600x1000 mode),
  a click at a marker pixel lands at its centre, the four corners and edges are delivered, one pixel past the edge is refused,
  and the control without `--scale` is red; Xorg's dummy driver, Xvfb, Xvnc and Xephyr refuse a CRTC transform; xrdp, client-side
  `GDK_SCALE` or Qt scale factors and a real GPU were not driven. A drag handle needs a driver that lives between calls: held
  through one `icu mcp` session on headless sway (probe 65 of 65; restarting the session before every call turns two cases red);
  `icu call desktop_drag_handle start` on native Wayland without `ICU_SOCKET` is now refused (`unavailable`, `needs:
  driver_session`) and presses nothing, instead of losing the release (a unit test, five mutants of the guard red, and two probe
  cases red with the guard compiled out; an X11 one-shot works; written into the SKILL). The app does not use the handle (it runs
  its own persistent `serve`). Not shown: a real Wayland client beyond the fixture, and a GNOME or KWin one-shot (the guard keys
  on `WAYLAND_DISPLAY` and the Wayland opt-in, not the compositor).
  Round 3, 2026-10-05 ([gaps-linux-5](../study/audit/gaps-linux-5-2026-10-05/libei/README-libei.md)): over libei on GNOME 46 and Plasma 6.6 Wayland (a `portal-libei` build) a multi-point `desktop_drag` and the drag handle work, the handle across one `icu mcp` session and the one-shot handle still refused; on the patch `7a13e2e6` the facade probe passes 64 of 64 on Xvfb and 65 of 65 on headless sway (Verified, Linux, 2026-10-05 round 2).


## Partial

| Area | What holds | Missing, or not re-reviewed | Evidence |
| --- | --- | --- | --- |
| macOS 26 | on 26.3.1 the 15.7 suite, background click, `type_text`, `set_value`, scroll, screenshot binding, `stale_capture`, the wrong-window refusal and Clock, Calculator and TextEdit Sky text all pass at 1x and 2x; TCC.db grants work and the app's rows cover the sidecar; the notarized app opens from a quarantined download with the normal first-open prompt and no override, and its text area, Calculator and memory checks pass (see Verified, macOS) | an unsigned or ad-hoc build is still blocked until Open Anyway or `xattr -dr`; the @ list shows off-screen "untitled" windows, now including AutoFill's; at 1280x800 @2x the app window fills the screen. Tested on 26.3.1, not 26.4 (the Namespace Macs offered 26.3.1 only) | [mac26-acceptance](../study/audit/mac26-acceptance-2026-09-28/README.md), [final-mac-3](../study/audit/final-mac-3-2026-10-04/README.md) |
| Tile gestures, Windows and Linux | the app's wheel gesture sends the frame it was read from on every platform (one code path, pinned by a Rust test and, since 2026-10-05, a jsdom test of the real wheel handler that fails when the id is dropped; the repo had no JS test of the wheel before); tile clicks carry a bound id (SB9, app D1 to D4 on Linux). The macOS tile click, wheel and F5 are under Verified, macOS. Windows: the installed app, a real mouse wheel, the bound window scrolled 0 to 12, a decoy on top of it 0, and the request carried a driver-issued frame id; the shipped driver refused the same wheel (a real bug, fixed; Verified, Windows). Linux: the installed `.deb` on a real X session (Xvfb, openbox, picom), the real X pointer turning the wheel over the tile, the bound window scrolled 0 to 15 and back, a decoy stacked in front 0, all 10 `scroll` requests named the bound window and a frame id the driver issued; a plain XTest wheel at that place hit the decoy 3 of 3, and a build with the id dropped fails the probe (it still scrolled the right window, as the old Mac app did: the request differs, not the outcome; the report's vitest mutant output was in the uncommitted `build/` folder, so the jsdom guard's red rests on its text) | Windows: the wheel over the tile of a window above the daemon's integrity, and a trackpad's pixel deltas. Linux: the wheel target was a raw X11 window with a log, not a GTK or browser scrollable; XTest, not hardware; no Wayland; no blocked (off-Space) tile; the tap in the probe replaces the owned-driver identity checks. Physical IME and clipboard from real hardware (Needs Nathan, item 5) | [gaps-mac §2.3](../study/audit/gaps-mac-2026-10-04/README.md#23-tile-scroll-48940e7), [gaps-mac-live §3](../study/audit/gaps-mac-live-2026-10-04/README.md#3-item-1-tile-gestures-through-the-signed-app), [gaps-windows-3 §2](../study/audit/gaps-windows-3-2026-10-05/README.md#2-the-windows-tile-wheel), [gaps-linux-3 updater.md](../study/audit/gaps-linux-3-2026-10-05/updater.md#b-linux-tile-wheel) |
| Live view | frames match driver bounds at 1x and 2x | N10 overclaimed; 2x not re-reviewed; no real Retina panel (the 2x tile runs used the 1280x800 mode, the tile clipped about 13 percent by the screen); the frame id switches in the same task as the image (X8) | same, §5 |
| Linux canonical gate | 151 cells, 95 deliveries and 56 refusals, 0 failed, 0 skipped, at acda22bf on a fresh host with a base image built from the recipe ([gaps-linux-4 gate](../study/audit/gaps-linux-4-2026-10-05/gate/README-gate.md)): the 150 older cells match the e2d73ed run cell for cell and the extra refusal is the new null sway `app_id` cell. Before that, 150 cells, 95 deliveries and 55 refusals, 0 failed, at 8352bae, f40695d and e2d73ed on fresh hosts (the last on a base image built from the recipe, which now pins fifteen superseded packages by sha256: `compare-packages.sh` differs from the export only by `copilot`, one changed hex digit fails the build; I2 was found overclaimed, 10 packages had drifted, and was closed in gaps-linux-2). The floating-overlap cell checks with grim that the float is in the target's rectangle (G1). Earlier: 150 cells at fa7661e after the C9 final fixes ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md#linux-canonical-gate-at-fa7661e)) and at a8c30ed, 151 recordings decode; round 4 G1, G5, I2 fixed in [linux-review4-fixes](../study/audit/linux-review4-fixes-2026-09-26/README.md#canonical-gate-at-a8c30ed). The icu-shaped final probe: the old shape refuses `ambiguous_capture` by design, the shape icu sends now passes 8 of 8 | the gate's mutants are all live against the current driver (gaps-tests-3, 2026-10-05): 19 literal source edits, each matching once and building, on a real Linux host (arm64 Ubuntu 24.04). The second full pass had all 19 red on their paired cells; the first had 18 of 19, because one pairing was wrong and is corrected: `meaning` turns the relabel and recycled-row cells red and leaves stale-object green (2 red, 1 green; the mutant degrades the snapshot key to the object identity, which a reorder still changes), and the old record of it as red on three cells belonged to a hunk that had since stopped matching. The unmutated driver passed all 24 paired cells in three runs of about 17 minutes. The six stale ones were replaced (`displays` by two literal edits; `swaynull` dropped for `swaycrop` and `swayuntyped`; `overlapoff`, `layershelloff`, `r2containers` and `r2readiness` rewritten against the current sway refusal), and `pairs.json` plus `devall.py` replace the old scripts with a check that fails when a mutant stops going red or goes red somewhere it should not. Closed 2026-10-05 ([gaps-final-build, Linux](../study/audit/gaps-final-build-2026-10-05/linux/README-linux.md)): the full 151-cell gate ran once on this HEAD patch (sha256 `10ca8e52...`, built on a fresh host through the gate's own `run.py`): 95 delivered, 56 refused, 0 failed, 0 skipped, the same 151 ids and the same expected and observed behavior as the acda22bf run, cell for cell; the 25 paired cells plus 2 web cells passed 27 of 27 on the same tree; one run, not three. Not closed: the mutants were not re-run on this patch (gaps-linux-5 ran them on `7a13e2e6`; the `portal-libei` driver built from HEAD's patch has the same sha256, `0613ffc5...`, as the one built from `7a13e2e6`, so the Linux driver did not change); the sway mutants all guard one refusal, red because three cells assert the same rule, with three detector shapes telling the cells apart; sway's null `app_id` (XWayland views carry it) now has a live cell, `linux-sway-xwayland-null-app-id-capture-ax-not-applicable` (a native window beside an XWayland one: readiness names sway's refusal and the capture is refused `surface_identity_unproven`), red under the `swaynull` mutant while the neighbouring sway cell stays green; the `displays` closed-window cell still goes red because the refusal code changes (`stale_capture` for `stale_window_target`), not because a click lands: with both guard layers removed the click is refused `background_unavailable` (GTK3 on X11 has no focus-free pointer route), so no edit of those checks lets it land and the cell guards the typed refusal and "nothing reached window A" (it now asserts that no click fired before it checks the code; with only the capture checks removed it stays green); `lockgrab` does not reach the shared web cell (both browser round-trip cells stay green under it, recorded as `lockgrabweb` in `pairs.json`), so it guards the three gtk3 background cells only. Closed in gaps-linux-5 (2026-10-05, [gate](../study/audit/gaps-linux-5-2026-10-05/gate/README-gate.md)): the four new mutants (`swaynull`, `displayslayers`, `displayslayer2`, `lockgrabweb`) ran three times each on a fresh host, red where expected with their green controls in all three passes; the 25-cell paired baseline passed three times, and a corrected baseline that also runs the two shared web cells passed 27 of 27 twice (the first baseline ran those two in no session, a harness bug that read as RED, not a driver result; a third corrected rerun was stopped by hand and is not counted); `compare-packages.sh` was re-run and the rebuilt base differs from the export by one package, `copilot:arm64`, as in earlier runs (cause not investigated); the summary parser read 149 of the 150 old rows and 150 of 151 in the gaps-linux-4 run because it kept only rows whose id starts with `linux-` and dropped `desktop-agent-cursor-showcase-px`, and it now reads the whole table, with a test that fails on the old filter (4 tests). The first full run at 275bf4ef passed all 151 cells and then failed `verify.py` because the expected catalog had been regenerated from a stale generator; the catalog now starts from the verified e2d73ed one and the second run is the reported one. The pins are a point in time: the archive can move again and `compare-packages.sh` shows it. | [gaps-final-build Linux](../study/audit/gaps-final-build-2026-10-05/linux/README-linux.md), [gaps-linux-5 gate](../study/audit/gaps-linux-5-2026-10-05/gate/README-gate.md), [gaps-linux-4 gate](../study/audit/gaps-linux-4-2026-10-05/gate/README-gate.md), [gaps-tests-3 §1a](../study/audit/gaps-tests-3-2026-10-05/README.md#1a-item-1-the-linux-gates-mutants-all-live-against-the-current-driver), [gaps-linux-2 gate](../study/audit/gaps-linux-2-2026-10-04/gate.md), [gaps-linux-windows linux.md](../study/audit/gaps-linux-windows-2026-10-04/linux.md#canonical-gate-at-8352bae) |
| Daemon start | `icu` takes a start lock and records the `serve` it started by pid and process start time, so a later start waits for a daemon whose starter was killed or gave up, and a reused pid (or pid 1) is not taken for it; any start that finds the daemon listening with the lock free clears a stale record; `icu` reaps the `serve` it started on every return path, so a dead daemon is never left a zombie that counts as alive; the patched `serve` takes its socket lock at entry, before the load. 8 real `icu` processes raced against the real patched `serve`: one daemon with a killed winner and with a 16 s load | round 2 A3; round 4 R4-X3; round 5 F2 and F3 fixed ([app-review5-fixes §2](../study/audit/app-review5-fixes-2026-09-27/README.md#2-daemon-start-f3-f2)); round 6 confirmed them on macOS, and the Linux start time (B4) then ran on a real Linux host: a forged command name, a zombie, an exit race and a pid reused through `ns_last_pid` ([app-review6-fixes §6](../study/audit/app-review6-fixes-2026-09-28/README.md#6-linux-start-time-b4)). The installed driver and the app sidecar predate the socket lock until rebuilt from the patch; on a Unix other than macOS and Linux the start time is not read, and a reused pid can still make a start wait 15 s and refuse | [app-review2-fixes](../study/audit/app-review2-fixes-2026-09-26/README.md#a3-daemon-auto-start-race), [app-review4-fixes §7](../study/audit/app-review4-fixes-2026-09-26/README.md#7-daemon-start-r4-x3) |
| Subprocesses | a per-command tag reaches `setsid` orphans; the tag carries the app's process start time, so a helper left by an earlier run is never matched by a later run with the same pid and count (found 2026-10-04: it would have killed a process it never started); the macOS tag read compares every string after the exec path, so a command whose first argument is empty is found (before, the orphan was not killed); a successful command never kills by tag (X7, by design). Each has a test that fails when its guard is removed | round 2 A9 and the 2026-10-04 fixes are unit tests, not driven live; X7 stays a known limit (a helper a successful command backgrounds outlives it); Windows and Linux tag reading were not looked at in 2026-10-04. See Known limits for what the tag cannot reach | [gaps-mac §2.4](../study/audit/gaps-mac-2026-10-04/README.md#24-subprocess-tags-e1a5855), [app-review2-fixes, A9](../study/audit/app-review2-fixes-2026-09-26/README.md) |
| Memory | wrapped, bare-name and `file:line` citations checked; a file followed by anything that is not a path character (`:12-20`, `:12—20`, `#l12`, `?plain=1`, `：12`) is checked as that file, and a word that is not itself observed has every file name inside it checked (`Invented.swift%3A12`, `Foo.swift` then a zero-width space then `Invented.swift`). Command redaction is best-effort. It reads commands as shell words and hides: the argument of a flag named like a secret, in both `--flag value` and `--flag=value` forms and whole when quoted; environment assignments; secret-named `name=value` pairs inside any word or heredoc; the per-tool flags (`sshpass -p`, `docker login -p`, `mysql -pX`, `redis-cli -a`, `ssh-keygen -N`, `security -w`, `az -p`, `mongosh -p`; positional ones: `htpasswd -b`'s password argument, `vault login x`, `openssl passwd x`) behind wrappers or other programs; here-strings, heredocs and piped `echo` feeding a `*-stdin` flag or `sudo -S`; writes into credential files; URL userinfo; tokens with known prefixes and long opaque tokens; the value after a secret-named setting, passing over flags, `--`, `=` and `<<<` (`gh secret set API_KEY --body x`), but not a redirection's file, a search pattern (`grep api_key src`) or a subcommand (`create-access-key`); PowerShell's `ConvertTo-SecureString "x" -AsPlainText`, `-Password` and `$env:X = "x"` or `Env:X` with a secret name. A switch in a dotted setting (`--set auth.enabled=true`) is kept. A secret in a position no rule names (a short password as a plain argument to an unknown tool) is kept; chmod branches guarded; a value the classifier or judge lists is hidden everywhere it occurs, including two listed values that overlap and a value that overlaps itself (before, `abc123` and `123xyz` over "abc123xyz" left `xyz` in the journal; fixed 83f4be9, [gaps-app memory](../study/audit/gaps-app-2026-10-04/memory.md)) | round 2 B2, B8, B11, B12, round 3 R1 and R2, round 4 E6 and R4-X1, then round 5 B4, D3 and R5-X2 fixed ([app-review5-fixes §4, §5](../study/audit/app-review5-fixes-2026-09-27/README.md#4-citations-fail-closed-b4)); round 6 D2 and E2 fixed ([app-review6-fixes §4, §5](../study/audit/app-review6-fixes-2026-09-28/README.md#4-redaction-e2-e3-e4)), not re-reviewed. On round 6's corpus 59 of 320 ordinary commands still change (19 of them only URLs, by design) and 16 of 114 secrets are kept (`zip -P`, `sqlcmd -P`, `ldapsearch -w`, SQL `PASSWORD '...'`, `net user`, `cmdkey /pass:` and others no rule names). Open: paths in prose reach Skysight's `USER_PROFILE.md` (X1; the app's chat memories check prose paths, see App memory), a forward clock jump writes summaries that are never replaced (X2), no grace for late events (X3) | same |
| App memory | chat memories: a run's journal, written only once its secrets are hidden: at the end of the run, while its raw text is only in memory, one model call (the secret classifier) on the provider that answered the run, and only when the memory settings name no other provider, reads the goal, the outcome text and the step labels as quoted JSON data and lists secret spans, hidden wherever they occur verbatim; the wording detectors run on top as if each span were still a value (command secrets, 20+ character opaque tokens included; in prose the rest of the clause after a word that names a secret and the account and secret after a sign-in verb, as the Checks below define them); in the goal the shape guess is off and its remaining words shaped like a secret are recorded as cleared, and in screen text the shape guess stays on for every word but a cleared one; paused, rate limited, no such provider, a failed or timed-out call (120 s on Claude Code), an unusable reply (not JSON, no list) or a pause during the call means the detectors alone, shape guess included; a listed value that is not a string, under three characters or not found verbatim is dropped on its own and the others are still hidden, and such a run keeps the shape guess on everywhere and clears no word (the count dropped, never a value, goes to the status's last error and stderr); the raw text is held on the task's thread for the call and never written, goes nowhere but the provider that already saw it in the task, and a delete during the call drops the run; runs on one app and one provider within the idle threshold are one session; after 30 idle minutes, with no task running, one extract call and one consolidation call per provider, on the provider that answered the runs (local when only the local model answered, fallback included) unless the settings name one (validated against the known providers); pause, a running task and memories-off are read from `settings.json` before every model call and every write, so another app instance's switch holds, and a settings save merges only the fields it changes into the file under the store lock (the panel sends only what its form changed); a delete during a call stays deleted, for chat memories and Skysight's summaries, `MEMORY.md` and `USER_PROFILE.md`, and from a second app instance (delete generation in `settings.json`, store lock plus `flock` on the folder); deleting a session drops every entry that cites it, Delete all removes leftover temp files, every write is a temp file and a rename; short sessions and ones flagged as using external context are dropped without a call; a 30-minute pause after a reported rate limit. Provenance: a summary keeps the user's redacted goals; whether a preference or correction is the user's (clean) is decided at consolidation by a judge, one more model call per consolidation on the same provider and model with the same pause, rate-limit and delete-generation handling (a pause or rate limit at the judge writes nothing), which reads only the goals the user typed in the sessions involved (no title, outcome, step, note or window text) and the proposed entries as quoted JSON strings, and says per entry whether the user stated it, whether a later message withdrew or reversed it (the judge is shown with each entry up to 6 later messages of the same app from the same provider's sessions, newest first; an entry it says was reversed is dropped, and a stored one with that text removed), whether the entry means what that message means, a verbatim quote, and whether the entry holds a secret; an entry is clean only when the judge said both yes (JSON `true`) and the quote is found, as whole words in the matching form, in ONE goal the user typed in that app's sessions it cites (globally, a quote in each of at least two apps named by bundle id), holds no redaction marker, and carries at least three content words or is the whole sentence it was cut from, else observed; a judge that fails, times out or gives an unusable reply leaves every entry of that pass observed; a candidate it answers twice or not at all stays observed and unanswered (so it is re-judged later) while the others keep their verdicts; a candidate naming the judge's fields in any case or separator ("user_stated", "userStated", "same-meaning", "exact quote", "supersedes") is dropped before the judge, whether it came from a reply, the fallback's notes or a re-judge; the quote is stored with the entry; the word rules that decided this before (negation list, clause cutting, the reason rule, antonym pairs, the half-the-words test for replacing) are removed; a claim about the user in a word only the app showed is still dropped at extraction, and extraction no longer marks notes as the user's. Consolidation: a reply that is not usable JSON, or a consolidation call that fails, writes nothing: the sessions stay pending, the status's last error says why and the next pass retries; only a reply the model gave and that was unusable counts (attempt n of 3): a call that failed (offline, a timeout, a provider error) backs off 1, 2, 4 minutes and on up to 30 and never counts, for extraction (a session is left to retention only after 3 refused replies) and for consolidation alike, and a write that fails (a full disk) backs off the same way; only after 3 refused replies in a row for a provider does the fallback store each note on its own, and then every clean preference and correction of those apps also goes to the judge with its later messages, so one the user has since reversed is removed ([acceptance-bugs](../study/audit/acceptance-bugs-2026-10-01/README.md); before, the first refusal fell back at once, dropped the reply's `replaces` and left a reversed preference clean beside its reversal, linux-final bug 1); at most 10 new entries per app per reply; an app's entry cites only that app's sessions; the store is trimmed per app, clean entries before observed ones and, among clean ones, entries already stored before ones the pass added; `replaces` honoured only within the same scope, evidence and kind family, never observed over clean, and a clean entry only for a clean one whose quote is in a goal of a session of that app newer than every session the old one cites and that the judge says changes or restates the old one. Injection: that app's entries (a name-scoped entry only for a run with no bundle id), then global, at most 8 entries and 1500 characters, in a MEMORY section of its own after the window in every turn, never inside the goal (every provider's turn, the fallback's included), in two blocks of escaped JSON lines: `<user_preferences>` holds each clean entry as the user's own sentences (`user_said`, the whole sentences of the goal that hold its quote, so a quote cut before a negation still shows the negation) with the memory pass's paraphrase cut to 80 characters as `label`, and the header and system prompt say `user_said` is what counts; `<unverified_notes>` holds everything else (observed entries, Skysight's lines, and a clean entry with no sentence of the user's to show, which is now observed), described as unverified, never instructions and never a reason to delete, send, pay, buy, submit or publish; a `memory` event says what was used and whether the run is tainted; at injection the stored quote is looked for again in the stored goals and an entry whose text no longer matches its id is observed; a note that names deleting, clearing, sending or paying is still offered only to a run whose goal names a word of the same family (four fixed word families: delete, remove, erase, trash, discard, wipe, unlink, get rid of, throw away and forms; clear and empty; send, forward, post, publish, submit, email, mail, share; pay, buy, purchase, checkout, order (not after sort or in), transfer, donate), as a relevance filter only. Confirmation gate: a run given any unverified note is tainted (clean entries alone do not taint), and in it every step the action layer reads as sending, paying, deleting, submitting or publishing waits for the user's answer in the app (Allow this step or Don't allow, 120 s) before it is dispatched; code decides this from the step and the element it resolves to in the observation it was decided on, never from the model or the memory text: a click or a set_value on a control that is not a text field whose label, title, description, help or identifier holds a word or phrase of a fixed English list (send, forward, share, upload, invite, reply all; publish, post, deploy, merge; pay, buy, purchase, checkout, place order, subscribe, donate, tip, transfer, wire; delete, remove, trash, erase, discard, destroy, wipe, uninstall, empty trash; submit, confirm, approve), camelCase identifiers split; cmd or ctrl with Return; cmd, ctrl or shift with Delete or Backspace; Delete or Backspace unless the focused element is a text field; cmd+shift+D; Return, a typed line break or an accessibility confirm in a chat app or a terminal (by bundle id or name), on a focused control the list names, or in a window that shows such a control anywhere; Space on a focused control the list names; every drag. A no, or no answer, is a refusal (`user_declined`, `confirmation_timeout`) the model reads on its next turn, and the rest of its batch does not run; a stop while waiting ends the run; the answer is taken only for the running task's open request (`agent_confirm`), once. Checks: secrets are the union of the classifier (at record time, above), the judge and the detectors below: an entry the judge says holds a secret, or that holds a span the judge marked, or that the detectors refuse, is dropped on every path (a reply, the fallback, a re-judge), and a stored entry with its text and scope is removed when the judge flagged that text, the detectors refuse it or a later message reversed it (not when the text only holds a marked span); a stored entry holding a span the judge marked in a message is touched only when the span is a whole word of it and the entry is of the app of the session the span came from (a global entry only when it cites a session of that app): removed when the detectors would hide the span in it too (the wording rules, or the shape guess for a word no classifier cleared in its evidence or in the span's session), otherwise the span is hidden in its text and the entry kept, observed, under the id of its new text (memory-integrity); and a span of a new session's goals the judge marks is hidden in its stored memory where it occurs verbatim (three characters or more); with no judge reply only the detectors hold. A memory sentence naming a secret noun from a fixed list is refused (spaced, hyphenated and letter-by-letter spellings such as "pass word", "pass-code" and "p a s s w o r d", and the words for password in German, French, Spanish, Portuguese, Dutch, Swedish, Norwegian, Danish, Polish, Turkish, Finnish, Hungarian, Czech, Russian, Chinese, Japanese and Korean); so is one holding a credential with no secret noun: after a sign-in verb (log in, sign in, sign on, authenticate, unlock, and login when no determiner makes it a noun) and with, as, using or via in its clause, an account and a secret joined by and, a slash, a comma, with or using where the secret ends its phrase or is shaped like one ("signs in with admin and tulip99"), `account:secret` or `account/secret`, or one value of four or more digits ("unlock the phone with 4829"); and so is a word shaped like a secret anywhere, unless the classifier read it in the raw goal of a session the text rests on and did not mark it (a journal's cleared words, carried to its session memory and read at extraction, consolidation, the judge's input, settle and injection; a journal not marked classified clears nothing, and a word the judge marks loses its clearance): six or more letters and digits run together that are not a number with a unit, ordinal, month, time, version, quarter or currency around it ("1080p", "21st", "14Mar2026", "10:30am", "Q3FY26", "EUR1200") and not capitals then a number ("UA1234", "SPRING25"), or six or more characters with three of lower case, upper case, digits and a password symbol (a leading `#` or `@` and `$` before a digit do not count); file names, paths, e-mail addresses and bundle ids are left to their own rules; marker, label, tag and directive rules match a UTS #39 confusables skeleton (marks dropped, look-alike digits, `i`/`l`/`1`, `rn`/`m` and enclosed letters folded); the stored and matching forms remove the same default-ignorable characters, Hangul fillers included, keeping joiners and selectors only inside emoji; a word mixing scripts (TR39 above Highly Restrictive) or using a letter outside identifier status or from the phonetic and specialist Latin blocks is refused; directives, injection markers, forged labels and tags, URLs, bundle ids and paths outside the session. Skysight runs from the app worker, off by default; its model calls hold no lock; its events are pruned by retention whether it is on or off. Controls: chip and panel (pause, switches, retention, per-item delete, the provider rule as it applies), per-run Use memories and Remember this task. 110 memory unit tests (scripted models; 17 of them the memory-correct fixes); memory-integrity (not re-reviewed): one wrong secret verdict on an ordinary word no longer deletes every entry holding it (it hides that word in that app's entries), and a classifier reply with one bad value keeps the good ones; 5 new tests red before the fix, 17 of 17 new guards red when broken, see [memory-integrity](../study/audit/memory-integrity-2026-10-01/README.md); memory-influence (not re-reviewed): in a run given an unverified note, a step the action layer reads as consequential waits for the user's yes (R1, as good as the classification above), clean entries reach the model as the user's own sentences (R2), memory has its own section (R3); 33 of 33 new guards red when broken; real model (claude-sonnet-5 through `claude -p`, 8 calls): the delete instruction steered 0 of 3 before and 0 of 3 after, and one task through the loop with an observed note was asked about its first step (Upload), declined by a scripted UI, read the refusal and failed without dispatching anything, see [memory-influence](../study/audit/memory-influence-2026-10-01/README.md); memory-secret-context (not re-reviewed): 30 of 30 new guards red when broken, and on 46 strings labelled before the run (claude-sonnet-5 through `claude -p`, 8 calls of 4 to 6 cases, 5.5 to 13.1 s each) the classifier alone hid 22 of 22 secrets and 0 of 24 non-secrets, the journal now hides 22 of 22 and 3 of 24 (a full commit sha, by decision, and two words only the app showed) where the detectors that wrote it before hid 14 of 22 and 11 of 24, with 5 of 5 secrets behind an injection in screen text hidden; one model, one run, see [memory-secret-context](../study/audit/memory-secret-context-2026-10-01/README.md); memory-judge-fix (not re-reviewed): the review's findings 1 to 3, 6 to 8 and 11 fixed, 6 red tests at the review commit, 23 of 23 new guards red when broken, the judge given its own 180 s timeout, and on the real model the reversal scenario (claude-sonnet-5 through `claude -p`, 5 calls) removed the reversed preference in 3 of 3 runs: twice the judge said a later message reversed it, once the real consolidation replaced it and it was not re-judged, see [memory-judge-fix](../study/audit/memory-judge-fix-2026-09-30/README.md); memory-semantic (not re-reviewed): 44 of 44 judge guards red when broken (30 first round, 14 for a follow-up round, of which the 4 shape-deferral guards no longer apply since memory-judge-fix reverted the deferral), and on 49 cases labelled before the run (claude-sonnet-5 through `claude -p`, 7 judge calls) the clean decision was right on 34 of 34 for the judge alone and 33 of 34 shipped (N05, "the sha256 of each installer", is refused by the shape detector) (28 of 34 for the word rules of 277d738); secrets in entries 10 of 10 caught shipped; secrets correct overall 45 of 49 shipped (37 of 49 word rules), re-scored offline from the recorded judge replies with the shape detector a hard block again; the judge's "not a secret" on N01, N02, N05 and N06 (iPhone15, Office365, sha256, Winter2024) measured nothing, since it was shown `<redacted>` in their place; secret values hidden in the stored session memory's goals 9 of 9 (1 of 9 word rules; the run journal itself is redacted by the detectors only); up to 5 old observed entries with no quote are put back to the judge each pass it runs, never one the pass replaces or one naming the judge's fields, and one the judge flags as secret or reversed is removed, so such entries do not hold the slots; a pass's judge calls are batched (4 candidates per call), which lowers the typical call's latency but does not bound it (a 2-candidate call ran past 90 s), any batch's failure still failing the whole pass closed; see [memory-semantic](../study/audit/memory-semantic-2026-09-30/README.md); memory-fix5 (not re-reviewed): 55 unit tests; memory-fix5 (not re-reviewed): its two red tests and the flipped limit assertions (5 memory tests red at 5c5c7b1), 26 of 26 new round-5 guards red when broken, and on the corpora it measured secrets kept in journals R1 3 to 1 of 30, F7 5 to 4 of 30, N5 18 to 2 of 20, with ordinary goals changed R1 1, F7 2, M3 1 unchanged and N5 0 to 2 of 46, no change on the real model's 68 notes, 35 fields and 7 goals, and 5 of 16 recorded real-model preferences clean before and after, see [memory-fix5](../study/audit/memory-fix5-2026-09-30/README.md); round 4's review probes: B1, B2 and B3 no longer show their break (their break assertions fail), H1 to H3 hold; memory-fix4's probes 5 of 5 (red at the review commit 5 of 5 fail); 19 of 19 new round-4 guards red when broken (31 of round 3 before); real model (claude-code, `claude -p`), see [memory-fix4](../study/audit/memory-fix4-2026-10-01/README.md#real-model) for the round-4 run | round 1 [memory-r1](../study/audit/review-2026-09-29/memory-r1.md) fixed in [memory-fix1](../study/audit/memory-fix1-2026-09-29/README.md); round 2 [memory-r2](../study/audit/review-2026-09-29/memory-r2.md) fixed in [memory-fix2](../study/audit/memory-fix2-2026-09-30/README.md); round 3 [memory-r3](../study/audit/review-2026-09-30/memory-r3.md): 1 broken and 5 overclaimed, fixed in [memory-fix3](../study/audit/memory-fix3-2026-09-30/README.md); round 4 [memory-r4](../study/audit/review-2026-10-01-memory-r4/README.md): 2 confirmed and 3 overclaimed, fixed in [memory-fix4](../study/audit/memory-fix4-2026-10-01/README.md); secrets with no secret noun and two clauses of one sentence recombining (both limits of round 4) closed in [memory-fix5](../study/audit/memory-fix5-2026-09-30/README.md); the word rules replaced by a judge in [memory-semantic](../study/audit/memory-semantic-2026-09-30/README.md); not re-reviewed. The rate pause reacts to a limit and reads no remaining quota; the Codex login provider cannot run memory passes; meaning is the judge's call: the code checks only that the user typed the quote, so a judge that obeys a candidate, or quotes a real sentence for an entry that means something else, makes that entry clean (a scripted test shows it; on the real model 0 of 2 injected candidates and 0 of 20 attacks passed, one model, one run each, a count and not a rate); a judge call is one more call per consolidation; recorded judge calls took 6 to 74 s and one of 2 candidates ran past 90 s, so the judge has its own 180 s deadline on Claude Code, also inside the `auto` chain (the HTTP providers keep their own 60 to 180 s), and a call past it leaves the whole pass observed; entries with no quote (stored before this change, or from a pass whose judge failed) are observed until a later pass re-judges them, bounded to 5 a pass, so a store of many such entries clears over several passes, not one, and only when a new session starts a pass for that provider; memory-correct (not re-reviewed): two app instances on one folder merge at commit under the store lock (what the other wrote since this pass read the store is kept, what it removed stays removed, only sessions this pass settled are marked consolidated), a restated observed preference the judge answers with a found quote becomes clean, quotes per entry are deduplicated and capped at 4 and the injected block counts them (400 characters of the user's sentences per entry, inside the 1500), the rate limit is read from the provider's error and never from a reply, a clean replacement is not trimmed after the entry it replaces was removed, a judge reply may carry text after the JSON (first JSON object, truncated replies still fail), session memories older than retention are pruned once consolidated (the entries resting on them lose that evidence, and go when none is left), `auto` as the memory provider reads like no setting, see [memory-correct fix](../study/audit/fix-memory-correct-2026-10-01/README.md); remaining: a 3-letter span can land inside an earlier `<redacted>` marker (`<red<redacted>ed>`), cosmetic, nothing leaks; unconsolidated session memories are never pruned, and a consolidated one is pruned by its last run's age so an entry citing only it goes with it; the two-instance merge is by entry (a same-id entry both instances changed is the other's, not a field-wise merge) and the session list a pass consolidates is still read before the model calls; a later message typed on another provider is not shown to the judge, so a reversal there is not seen; the refusal counts and backoffs live in the app process, so a restart allows 3 more refused replies before the fallback, and a refused reply costs a pass's delay (the session's notes reach injection only once a pass goes through); in the fallback a judge that fails leaves the stored clean entries as they are, so a reversal is then caught only by a later pass's reply; a reply's `replaces` of a clean entry whose evidence the new entry does not all cite is still dropped (screen-derived text cannot delete the user's preferences), but the old entry now goes to the judge with the later messages of its app (reply or retry alike); the judge's later_reversed removes it, and otherwise it stays and the status carries a `lastNote` ("a replacement was not applied: ...", not an error) until the next pass that settles; only user-typed messages can cause the removal, and a judge that fails or is not shown the later message (another provider, past 6 messages) leaves both entries, the user's correction and the old preference, injected side by side with that note; the panel's main.ts does not render `lastNote` yet (outside the fix's scope), so the note is in the status JSON and the pass's dropped list only; not run against the real judge: whether it answers later_reversed for the old entry when shown the correction as a later message is the remaining unknown (the fallback path's re-judge has been run for real, this one only with a scripted judge); [fix-reversal-retry](../study/audit/fix-reversal-retry-2026-10-01/README.md); that unknown was then run on the real judge (claude-code through ccroute, 10 calls: a reversal and an indirect reversal removed the old entry, an unrelated and a screen-text-only message kept it, but a narrower later message made it answer later_reversed in 1 of 2 runs and delete a preference the user had only narrowed, [judge-reversal-real](../study/audit/judge-reversal-real-2026-10-01/README.md)); the judge prompt's later_reversed now counts a later message only when it withdraws or replaces the candidate's same scope and says to answer false when unsure (narrowing, an exception, one item or occasion, another thing are not reversals; one prompt-pin test, red when its sentence is changed), and on the real judge with the new prompt (24 calls, 11 cases written before the run) the narrower message was kept in 4 of 4 runs and 3 further scoped messages in 6 of 6, with 5 of 5 genuine reversal phrasings still removed in every run (10 of 10, one model, a count and not a rate), [fix-judge-scope](../study/audit/fix-judge-scope-2026-10-01/README.md); the cost is a lean toward keeping: a reversal phrased so that its scope is not plain (for example a bare "use sentence case for titles" after an all-caps rule on titles of one kind) may now be kept and the old entry injected beside the new one with the note, unmeasured; the stored quote and the entry id bind the verdict to the text only as well as the files are protected (an edit that keeps the id and quotes a real goal passes);  a clean one resting on a one-off goal ("Delete the old results") can still become a global; the confirmation gate's reading of a step is a list over English control text: it misses a control labelled only in another language or an icon with no text, a send or delete done by a key or gesture it does not name, a Return that submits a web form showing no submit control, and anything a click causes beyond the control's own words ("Done" that also sends), and it over-asks on controls that only open something ("Share" opening a sheet, "Remove filter"), on every upload (the user's own "upload this" is asked about in a tainted run), on Return in any window that shows a "Send" or "Delete" control, on Delete keys where the platform reports no focused element, and on every drag; it guards only tainted runs, so a clean preference the judge wrongly passed is not gated (it now reaches the model only as the user's own sentence); the memory word filter stays coarse ("remove the watermark" admits a delete note, "transfer the file" a paying one, "Shared" a sending one) and withholds useful notes that name an action from goals that do not; on the real model the new placement made no measured difference: an observed note telling the agent to delete the local copy after upload steered 0 of 3 decisions in the old place and 0 of 3 in the new one, one model, one case; a secret in the user's goal is the classifier's call: one it misses with no secret word and no sign-in phrasing is journaled, extracted and can be injected, a value shaped like a password the shape rule used to hide included; when the classifier falls back, a value with no secret noun, no sign-in verb and no secret shape is kept ("the code 482913", "HUNTER22", "the phrase blue-harbor") until the judge marks it at consolidation, and a name written like a password ("Office365", "iPhone15", "sha256", "Winter2024") is refused on every path and at injection, as it is for everything resting on journals and session memories written before this round; a word shaped like a secret that only the app showed is hidden even when it is a name ("Pixel8a", a tracking number); a reply that lists a whole sentence as the span hides the whole goal (1 of 9 eval calls); a judge's wrong secret verdict on an ordinary word still costs that word in every entry of that app that holds it as a whole word (hidden, the entry observed), and a real secret the judge alone catches stays in another app's entries and in the goals of the older sessions that hold it; the record-time classifier's positive verdicts are never stored, so the only second opinion settle has is the detectors (the classifier counts only by clearing a word); a full commit sha and any other 20+ character opaque token are hidden by decision; every journaled run makes one more model call, on the task's thread after the run for up to 120 s, and a quit in that window loses that run's journal; a memory sentence that names a secret noun even harmlessly ("the password field") is refused, and one that names a secret with none of the three signs ("the unlock shape is an L") or in a language or spelling not listed is kept; summaries written before this round have no goals, so nothing resting on them is clean; `set_settings` from a copy older than the instance's last read can carry stale fields (the panel does not use it); where a file system refuses `flock` only the in-process lock holds; (a task running across Delete all was journaled after it, L7; fixed 18444b6, a Delete during a task now drops that task's journal, [gaps-app](../study/audit/gaps-app-2026-10-04/README.md#memory-and-app-memory)); Skysight not run against a live driver from the app; Chronicle (screenshots with OCR) not built, seam documented; one real-model scenario set, not a rate. Added 2026-10-04: retention at its edges (a run ending exactly at the cut is kept, a clock set back deletes nothing), the `.lock` variant of the store lock and overlapping listed secrets are tested and mutated; on Windows four cross-process tests (real processes, all four fail with the lock a no-op) and two installed `app.exe` on one store ran both journals whole ([gaps-windows-2 §2](../study/audit/gaps-windows-2-2026-10-04/README.md#2-windows-memory-lock-two-instances)). Open: the Windows cross-process tests ran as SYSTEM, the standard-user case is the two-app run only; dangling evidence between two instances after a prune inside the other's model call; a judge-failed pass marks sessions consolidated without the judge's spans (documented); a caller-supplied run id beginning `s-` or `e-` is routed by `delete` as a session or entry (app run ids are `run-<pid>-<n>`); `rust-version` is now 1.89 (`File::lock`). Memory-first and the stored "never" clause: see Verified, parked items | [memory-integrity](../study/audit/memory-integrity-2026-10-01/README.md), [memory-influence](../study/audit/memory-influence-2026-10-01/README.md), [memory-secret-context](../study/audit/memory-secret-context-2026-10-01/README.md), [memory-build](../study/audit/memory-build-2026-09-29/README.md), [memory-semantic](../study/audit/memory-semantic-2026-09-30/README.md), [memory-fix1](../study/audit/memory-fix1-2026-09-29/README.md), [memory-fix2](../study/audit/memory-fix2-2026-09-30/README.md), [memory-fix3](../study/audit/memory-fix3-2026-09-30/README.md), [memory-fix4](../study/audit/memory-fix4-2026-10-01/README.md), [memory-fix5](../study/audit/memory-fix5-2026-09-30/README.md) |
| Patch merge guard | binary files, one-sided `-s ours` merges, `--resume` after a conflict. `--resume` takes only the merge of the ours and theirs commits the conflicting run recorded (not whatever the clone's branch names point at), each recorded rev reachable from a ref in the repo and the recorded pin the configured one; it refuses lines added beside a conflict (a line written in place of one line from each side is allowed, by design, and printed), a side reverted in a file with no conflict, a file both sides deleted coming back, and a clone with replace refs, attributes or a merge driver, including a `merge` attribute from git's global or system attributes file (git itself is asked, per path, for every tree); every file's mode must be the three-way merge of the sides' modes; a dropped base-patch line is caught however close a side's edit is; a relative output path is kept; the tracked patch now also carries `contract/manifest.json` and the Python and TypeScript bindings (one list, `impl/native/patch-paths.txt`, read by `merge-patch.sh` and `apply-and-build.sh`; `--regen` rewrites them, `--check-generated` fails when stale; 61 patch-merge tests, a merge that drops a manifest entry is refused) | round 2 C3 to C5; round 3 A4, A5 and H7; round 4 D2, D4 and the disclosed H8; round 5 A5, A2 and A4 fixed ([app-review5-fixes §1](../study/audit/app-review5-fixes-2026-09-27/README.md#1-merge-patch-a5-a2-a4)); round 6 A5 fixed ([app-review6-fixes §3](../study/audit/app-review6-fixes-2026-09-28/README.md#3-merge-patch-a5)), not re-reviewed. A dev tool: it guards against a careless or buggy resolution, not someone with write access to the kept clone. Still open: invented changes are not detected (H6) | same, [gaps-linux-2 §4](../study/audit/gaps-linux-2-2026-10-04/README.md#4-manifest-and-bindings-in-the-patch) |
| Sky text format | 14 captured observations render byte for byte; live Clock's city rows keep their ids across minute boundaries. Secondary actions are checked against the driver element's own actions; control characters in rows and quotes in the header title are escaped; a selection's fence is longer than any backtick run in it; trees are capped at 60 levels; an open menu's items are shown | round 3 found a spoofable secondary-action check (app-r3 S-A), rows forged by newlines (S-C), a stack overflow on deep trees (S-D) and open menus pruned (C8); round 4 found rows forged through the selection fence (R4-X2, [app-review4-fixes §6](../study/audit/app-review4-fixes-2026-09-26/README.md#6-the-selected-text-fence-r4-x2)); all fixed, not re-reviewed. The escaping, the longer fence, the depth cap and the open-menu rule are ours, not seen in the capture (spec E1, E2, T3, M2). Clock's collapsed containers still shift ids against Sky's; two known limits re-mint rows whose text changes and that have no identified container: the world map's points each minute ([B3](../study/audit/mac-review3-fixes-2026-09-26/README.md#b3-clock-ids-across-a-minute)) and Stopwatch's current lap row on every read ([R4-4](../study/audit/mac-review4-fixes-2026-09-26/README.md#r4-4-stopwatch-lap-row)); both pinned | [sky-format](../study/audit/sky-format-2026-09-26/README.md#remaining-mismatches), [app-review3-fixes §4 to §7](../study/audit/app-review3-fixes-2026-09-26/README.md#4-sky-secondary-actions) |
| Displays | virtual displays: two-head Xorg, three Windows monitors at mixed DPI, one Mac display at 1x and 2x. The Windows `HTTRANSPARENT` walk (W3, D1) was re-reviewed 2026-10-04: it passed a transparent window through across threads, where real input does not (measured by `WM_NCHITTEST` order); fixed, `displays.ps1` 382 cases with 0 failures and its `-Mutate` control failing as required on three monitors at 100, 150 and 125 percent, 28 unit tests, and removing either half of the thread rule fails 1 and 2 of them | no physical monitors, no macOS multi-display, no hot-plug; the cross-thread case is confirmed end to end only for glass on another thread over a cell on the parent's thread (a second-thread child gets no real click in the probe even uncovered, likely the probe's message pumps), and `displays.ps1` has no cross-thread shape; cross-process hosted windows and right-to-left regions were read, not measured | [displays](../study/audit/displays-2026-09-25/README.md), [gaps-linux-windows windows-signing.md §2](../study/audit/gaps-linux-windows-2026-10-04/windows-signing.md#2-displays-row-the-w3-fix-re-reviewed) |
| Linux package | clean non-root X11 install and every probe above; run headless on GNOME Shell 46.0, mutter 46.2 (also as the X11 window manager) and KWin 5.27.11: native windows need the opt-in and then only element clicks and `type_text` land; desktop capture there is a typed refusal, readiness reports a portal-only capture as not verified (`skip`, never ready) and `get_desktop_state` answers `desktop_capture_unavailable` with a reason (`portal_timeout` after 15 s, `capture_timeout` after 30 s, `wayland_capture_unavailable`, `wayland_backend_not_enabled`). Before: GNOME Shell reported capture ready while `get_desktop_state` failed, and KWin gave no answer in 240 s. Nine unit tests, five mutants red. Real auto-login GDM and SDDM sessions on vkms (Wayland) and an Xorg dummy (X11), PipeWire and the portal running, 2026-10-05 ([gaps-linux-3 shell](../study/audit/gaps-linux-3-2026-10-05/shell.md)): on GNOME X11 and Plasma X11 the desktop frame equals an independent root capture (0 pixels differ) and desktop click, foreground window click, foreground `type_text` and element clicks land with no portal, background pixel clicks and keys refuse `background_unavailable`; Plasma X11 needs `loginctl activate` first (SDDM leaves the session inactive and every input tool refuses `host_not_interactive`). GNOME Wayland with the opt-in: desktop capture works after the Screenshot permission is pre-granted in the permission store (the Shell's own dialog could not be answered through AT-SPI, and the pre-grant takes only for a process the user manager started, such as `systemd-run --user`); desktop click works only in a `portal-libei` build, whose Remote Desktop dialog was answered through AT-SPI (the click landed at the aimed point after 2.4 s), and the default build refuses it with the typed `desktop_input_unavailable` (`no_input_backend` or `portal_consent_timeout`; the click branch of that driver change is guarded only by the live run, not a unit test); element click and element `type_text` land and the window screenshot is refused `surface_identity_unproven`; without the opt-in desktop capture is `xwayland_root_not_capturable`. An idle GNOME session locks after 300 s and input then refuses `host_not_interactive`. KDE Plasma 6.6.6 Wayland, 2026-10-05 ([gaps-linux-4 desktop §5](../study/audit/gaps-linux-4-2026-10-05/desktop/README-desktop.md)): Ubuntu 26.04, `kwin_wayland --virtual` with no display manager, `vgem` and llvmpipe giving KWin OpenGL; the portal Screenshot returns a 1600x1000 PNG and the patched driver returns the same frame through `icu call desktop_screenshot` once the screenshot permission is pre-granted (without it `portal-screenshot-timeout` after 15 s; the default build without the opt-in refuses `desktop_capture_unavailable`); input through a `portal-libei` build lands (a click at exactly (300, 200) in a fullscreen GTK client, `press_key`, `type_text`, a 3-click wheel, a two-point drag, a move) with KDE's `kde-authorized` `remote-desktop` entry set (on a clean permission store the click times out and nothing is delivered), and the default build refuses input `desktop_input_unavailable`; on that libei backend multi-point `desktop_drag` and `desktop_drag_handle` failed `no-zwlr-virtual-pointer` in the driver at 9fad1227 and work in the gaps-linux-5 build (below). The earlier failure was four set-up traps found in the logs (session variables missing from the user manager and the bus, an empty service cache so KWin refused the caller `NoAuthorized`, KWin 6.6's screenshot code needing OpenGL and so a DRM render device, and a missing `~/Pictures`), not a driver or portal defect. Round 3, 2026-10-05 ([gaps-linux-5](../study/audit/gaps-linux-5-2026-10-05/README.md)): multi-point `desktop_drag` and `desktop_drag_handle` work over libei on GNOME 46 and Plasma 6.6 Wayland in a `portal-libei` build (a GTK client receives one press at the first point, motion with the button held through every point and one release at the last; the handle across one `icu mcp` session; a one-shot `icu call desktop_drag_handle start` is still refused, `driver_session`; the unpatched driver is refused `no-zwlr-virtual-pointer`; 7 mutants of the gesture code red in unit tests; Verified, Linux). Plasma 5.27 (Ubuntu 24.04, `kwin_wayland --virtual` on vgem and llvmpipe, OpenGL compositing; QPainter compositing always cancels the portal Screenshot): capture works through the portal and through the driver (screenshot pre-grant needed), with no Xwayland or with `XWAYLAND_NO_GLAMOR=1` (with glamor Xwayland segfaults; unsymbolized, not root-caused); input is not available, because KWin 5.27 has no EIS server and the KDE portal backend has no `ConnectToEIS`, so the libei worker exits at once and the driver call ends at its wall-clock deadline ("action completion unknown") instead of a typed refusal (a driver gap, not fixed). The earlier Plasma 5.27 failures were set-up traps (a stale kwin and portal still running, the unit named `plasma-xdg-desktop-portal-kde.service`, a portal backend with no display), not evidence about Plasma | Plasma 5.27 input (it would need a driver route over the portal's `Notify*` methods or a newer KWin); a typed refusal when the libei worker has died (a patch change, not made); a driver crash with the libei button held (not observed). Also untested: a real GPU, a person at a consent prompt (the sessions auto-logged in and consent was answered by pre-grant, AT-SPI or, on KDE, the `kde-authorized` entry), the icu layer's `desktop_screenshot` and `desktop_click` in the GNOME sessions (on Plasma 6.6 `icu call desktop_screenshot` ran), GNOME Wayland desktop drag, scroll and move, ScreenCast and PipeWire window capture, and a repeat on the merged build (only unit tests cover the typed refusal there). Wayland stays fail-closed on sway for window capture and refused on headless mutter; a person at a real GNOME or KDE login session and a real GPU are untested (Needs Nathan, item 6); the whole-call deadline has no unit guard, only a live one | [linux-fixes](../study/audit/linux-fixes-2026-09-25/README.md), [gaps-linux-5 libei](../study/audit/gaps-linux-5-2026-10-05/libei/README-libei.md), [gaps-linux-5 kde5](../study/audit/gaps-linux-5-2026-10-05/kde5/README-kde5.md), [gaps-linux-4 desktop](../study/audit/gaps-linux-4-2026-10-05/desktop/README-desktop.md), [gaps-linux-3 shell.md](../study/audit/gaps-linux-3-2026-10-05/shell.md), [gaps-linux-2 shell.md](../study/audit/gaps-linux-2-2026-10-04/shell.md), [gaps-linux-windows linux.md](../study/audit/gaps-linux-windows-2026-10-04/linux.md#linux-package-gnome-shell-mutter-kwin-headless) |
| Updates | reviewed end to end (N16, [gaps-app](../study/audit/gaps-app-2026-10-04/README.md#updates-n16)) with the shipped config against a local manifest server and a throwaway key: wrong key, tampered archive, a body cut short, a 404, 500 or unparsable manifest are refused, an older or equal version is never offered, server down is silent (jsdom test), and an old release announced under a new version is refused (`requireSignedVersion` is on, `@tauri-apps/cli` 2.12.1; before, it would have installed). macOS, 0.1.0 to 0.1.1 through the app signed with the real release key, on 15.7.5 and 26.3.1: Developer ID grants for Accessibility and Screen Recording survive and the app still drives, while a grant keyed to the old cdhash is lost; an update offered during a task leaves it alone and Install & restart is refused while a task runs (it killed the task before, 6b01817). Windows NSIS, per user and per machine, real certificate: the same refusals, a downgrade not offered, the worker still starts with UIAccess after an all-users update (an all-users update used to land per user, fixed 8bf8394b). Linux, Ubuntu 24.04 on a real X session, 2026-10-05: an AppImage and a `.deb` built by `tauri build` (`@tauri-apps/cli` 2.12.1; it writes `.AppImage.sig`, `.deb.sig` and `.rpm.sig` and no `.AppImage.tar.gz`, so the raw AppImage is the update file) were updated 0.1.0 to 0.2.0 by the app's own updater, a real click on the toast against a local manifest and a throwaway key. The AppImage was replaced in place with no privilege and relaunched as the new build (the relaunched app reported `cur=0.2.0`, the running binary's hash is the new one). The `.deb` ran `pkexec dpkg -i`, showed the real polkit dialog (lxpolkit) with the exact command, and finished once it was answered by synthetic keystrokes with a throwaway password, and again under a polkit rule; Escape on the dialog left 0.1.0 installed and running with an "Update failed" toast, and a session with no polkit agent fails the same way. A new test (raw AppImage replaced, mode 0755 kept) is red when the mode step is removed; the report also says the hand-edited `Cargo.lock` passes `--locked` and that `tauri build` signs the `.deb` (a `.deb.sig`), but its `build/` evidence folder (bundle listing, lock check) was never committed (a global gitignore rule for `build/` dropped it), so those two rest on the report's text, with the signed `.deb` update itself as indirect support. Round 2 (gaps-linux-4, 2026-10-05, [updater](../study/audit/gaps-linux-4-2026-10-05/updater/README-updater.md)): an `.rpm` built by `tauri build` was updated 0.1.0 to 0.2.0 by the app's own updater on a real Fedora 43 X session with SELinux enforcing, through the plugin's `pkexec rpm -U` and a real polkit dialog (lxpolkit) answered by synthetic typing, and again under a polkit rule; Escape on the dialog left 0.1.0 installed and running with an "Update failed" toast; a manifest that names the rpm under the bare `linux-x86_64` key is found by the plugin's fallback. The rpm did not declare `at-spi2-core` (the `.deb` does); it now does, and a test is red without it (the rpms that ran predate that change, which touches only Requires). The AppImage was also updated in place in a GDM auto-login GNOME 46 Wayland session (a native Wayland client, absent from Xwayland's client list, and again under Xwayland), relaunched as the new binary with its driver connected, the toast button pressed through AT-SPI `do_action`, not a pointer click, on `vkms` with software rendering. Earlier fixture runs (gaps-linux-2): an AppImage-layout file replaced in place (as root and as a user) and a `.deb` through `dpkg -i`,  each guard red by mutation. Round 3 (gaps-linux-5, 2026-10-05, [§3a](../study/audit/gaps-linux-5-2026-10-05/updater/README-updater.md#3a-rocky-9-and-rocky-10), [§4](../study/audit/gaps-linux-5-2026-10-05/updater/README-updater.md#4-deb-and-rpm-updates-under-a-wayland-session)): in a GDM auto-login Wayland session with the app a native Wayland client and the toast button pressed through AT-SPI, the `.deb` (Ubuntu 24.04, GNOME 46) and the `.rpm` (Fedora 43 with GNOME 49.10 and Rocky 10.2 with GNOME 49.4, both SELinux enforcing, which denied nothing in the update path) were updated 0.1.0 to 0.2.0 through GNOME Shell's own polkit dialog answered by a typed password (a uinput keyboard or ydotool, not a person) and again under a polkit rule; a cancelled dialog leaves 0.1.0 installed and running (on GNOME no "update failed" toast was found within 25 s, not examined further). Rocky 9 cannot carry the app (RHEL 9 has WebKitGTK 4.0 only: no `libwebkit2gtk-4.1`, no libsoup3, glibc 2.34 against the binary's 2.39; an rpm built elsewhere fails `nothing provides libwebkit2gtk-4.1.so.0`), so the EL run is Rocky 10.2 with the rpm built there against EPEL's webkit2gtk4.1. `rpm -U` resolves no dependencies: a new rpm whose Requires the machine lacks is refused (`Failed dependencies`), nothing changes, the old version keeps running, and the app shows only `update failed: Failed to install package`, not which dependency (the plugin's rpm install discards rpm's stderr; `dnf install ./new.rpm` would resolve it). The updater verifies minisign only: `tauri build`'s rpm has no gpg signature and rpm and dnf check none by default; `rpm --addsign` after `tauri build` makes the update refused (`The signature verification failed`) until the minisign signature is redone over the signed bytes with `--app-version`, and on a machine with `%_pkgverify_level all` an unsigned release rpm is refused with the same generic failure and a signed one installs ([§3b](../study/audit/gaps-linux-5-2026-10-05/updater/README-updater.md#3b-the-new-rpm-needs-a-dependency-that-is-missing), [§3c](../study/audit/gaps-linux-5-2026-10-05/updater/README-updater.md#3c-rpm-gpg-signatures-and-the-updater)) | nothing is published: the endpoint is a GitHub repository that does not exist (404) and the release key's custody is Nathan's decision (Needs Nathan, item 3); releases need `@tauri-apps/cli` 2.12.1 or later, and the main checkout's `node_modules` had 2.11.4 until `npm ci`. macOS updates ran over local http with builds that were not notarized, and the grants were TCC.db rows with the requirement a System Settings grant records, not click-through grants. Windows: a standard user who cannot elevate and runs an all-users install is told to ask an administrator before anything is downloaded and the app keeps running (Verified, Windows; driven through the real updater UI as a standard user on 2026-10-05: no Install button, one request for `latest.json` and none for the archive); an all-users NSIS update is run by the app itself with `runas` and waited for, so an administrator who cancels the UAC prompt keeps the running app, the install and a message (a real prompt, cancel and accept; the earlier note that a per-user copy appeared was wrong, the old behaviour was a windowless app process with the install unchanged); per-user installs update with no elevation and an MSI install is updated by an elevated `msiexec` the app starts, a cancelled prompt keeping the app and the install (Verified, Windows, 2026-10-05 round 3), and the UAC secure desktop was not driven (the prompt was on the user desktop by policy); an MSI update by a standard user was not retested. Linux: the polkit answer was synthetic and a person at the prompt was not shown (a `.deb` update needs an administrator at a prompt or a polkit rule; a user outside the sudo group is refused); a release whose rpm needs a package the old one did not fails on a machine that lacks it, with the generic message (measured in round 3: keep an rpm's Requires within what the previous release pulled in), and no app message names the dependency (it needs rpm's stderr from the plugin, not changed); Alma and any EL 9 not run (the app has no EL 9 platform); the toast button was pressed through AT-SPI, not a pointer click; the driver sidecar on Rocky 10 is the Ubuntu build and "driver connected" was not checked there; KDE Wayland updates, a real GPU (vkms, software rendering) and a published release are untested. The manifest keys are known (`linux-x86_64-appimage`: the raw AppImage and its `.sig`; `linux-x86_64-deb`: the `.deb` and its `.deb.sig`; `linux-x86_64-rpm`: the `.rpm` and its `.rpm.sig`) and unpublished. Linux build notes: Node 22.12 or later (Vite 8), and the vendor patch applies only to a clean vendor HEAD. The plugin's macOS install moves the running app aside and does not put it back if the final rename fails (not exercised) | [macos-native](../study/audit/macos-native-2026-09-25/README.md#updates-macos-proposed-partial), [gaps-app](../study/audit/gaps-app-2026-10-04/README.md#updates-n16), [gaps-mac-live §4.3, §4.4](../study/audit/gaps-mac-live-2026-10-04/README.md#43-update-with-a-tcc-grant-probe_update_livepyprobesprobe_update_livepy), [gaps-windows-2 §3](../study/audit/gaps-windows-2-2026-10-04/README.md#3-windows-updater), [gaps-linux-2 updater.md](../study/audit/gaps-linux-2-2026-10-04/updater.md), [gaps-linux-3 updater.md](../study/audit/gaps-linux-3-2026-10-05/updater.md), [gaps-windows-3 §4](../study/audit/gaps-windows-3-2026-10-05/README.md#4-standard-user-updates-an-all-users-install), [gaps-windows-4 §3, §4](../study/audit/gaps-windows-4-2026-10-05/README.md#3-cancelled-uac-elevation-of-the-installer), [gaps-linux-4 updater](../study/audit/gaps-linux-4-2026-10-05/updater/README-updater.md), [gaps-linux-5 updater](../study/audit/gaps-linux-5-2026-10-05/updater/README-updater.md), [gaps-windows-5 §3](../study/audit/gaps-windows-5-2026-10-05/README.md#3-updates-for-per-user-and-msi-installs) |


## Windows round 2 fixes

From round 2 [windows.md](../study/audit/review-2026-09-26/windows.md), fixed on
`fix/windows-review2` and merged in f64d4a5 ([windows-review2-fixes](../study/audit/windows-review2-fixes-2026-09-26/README.md)). Each fix
went red with only its guard removed, on the same `displays.ps1` bytes, and a fresh host
(packaged from a `git archive`) passed the whole acceptance set once: `displays.ps1` 251
cases with 0 failures and 25,328 samples unchanged, `-Mutate` failing as required, and
`native.ps1` 5 of 5. None of it has been re-reviewed.

- **C5, C19.** A posted click was checked by a second rect walk; a mismatch refused
  `point_unmapped`. A PMv2 form hosting an unaware container at 150% and 125% was added
  as a fixture; the old driver posted into the container and exited 0. The round-3 review
  found the walk shared the hit test's blind spots (group boxes, `WS_EX_TRANSPARENT`
  children, window regions); replaced in round 3, below.
- **C9.** With no `screenshot_id`, the latest capture is used while all of a window's
  captures share one mapping; once two mappings are seen, it refuses `ambiguous_capture`.
  Round 3 found the latest was published before its mapping was known; fixed below.
  Both are replaced by the core rule under Pixel binding
  ([c9-unify](../study/audit/c9-unify-2026-09-27/README.md#windows)).
- **N1.** Point `scroll` takes `screenshot_id` and is bound and scaled like a click.
- **N2.** Frames carry pid, thread, process start time and a window tag, so a reused HWND
  refuses `stale_capture` for an old id. An unbound pixel after a capture of the new
  window was let through until round 3, below.
- **N3.** WPF pixel clicks are refused `background_unavailable`, not made to work: posted
  mouse input only lands with the cursor over the window. Round 2 exempted a system with a
  ready digitizer and looked only at the root; round 3 removed the exemption and checks
  the target too, below.
- **N4.** The bad-id refusal carries `effect: refused`. **N5.** `drag` takes
  `from_screenshot_id` and `to_screenshot_id`, each scaled by its own capture.
- **C22.** `displays.ps1 -Mutate` now needs each moved case to fire its predicted wrong
  control; a driver that refuses everything fails it.
- **Harness (H1 to H4).** `as-user.ps1` reports a script that never ran as a failed task,
  `launchrace.ps1` kills its app trees and burners and takes a time budget, `hashes.json`
  values are strings (they were arrays), and the DISPLAY4/5 docs use `layout.ps1 standard`.
- **Open (closed in round 3).** The packaged driver's hash differed from a dev build of
  the same patch (different source paths).

## Windows round 3 fixes

From round 3 [windows-r3.md](../study/audit/review-2026-09-26/windows-r3.md), fixed on
`fix/windows-review3` ([windows-review3-fixes](../study/audit/windows-review3-fixes-2026-09-26/README.md)).
Each fix went red with only its guard removed, on the same `displays.ps1` bytes (322
cases), and on the reviewer's own probe bytes. A fresh host (packaged from `git archive
c87399d`) passed the whole acceptance set once: `displays.ps1` 322 cases with 0 failures,
`-Mutate` failing as required, `acceptance.ps1` 10 of 10, `native.ps1` 8 of 8 and the
launch race. None of it has been re-reviewed.

- **W8, R3-1 (C9).** A capture becomes a window's latest only once it is downscaled and
  its mapping known. The reviewer's 50-trial race: the previous driver clicked the wrong
  cell with exit 0 in 8 and 14 trials of two runs, this one in 0 of 50 in both. Since
  [c9-unify](../study/audit/c9-unify-2026-09-27/README.md#windows) a frame reaches the
  session's map only once finished, and a tile's frame never reaches the agent's: the
  same race clicked E in 50 of 50; with any session's frame read instead, A in 28.
- **W11, R3-5 (N2).** A new window at a reused handle starts a history marked ambiguous,
  so an unbound pixel refuses `ambiguous_capture` even after a tile captured the new
  window (it clicked G where C was read). Since
  [c9-unify](../study/audit/c9-unify-2026-09-27/README.md#windows) the session's own frame
  carries the old window's identity, so the pixel refuses `stale_capture` before and after
  the tile's capture; without that check it clicked G.
- **Final review C6, C7, C8, D5.** Fixed in
  [c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md), on the reviewer's
  `final.ps1` bytes: a `get_window_state` that timed out no longer files its late capture
  for the session (clicked A 3 of 3 before, E now); a capture in flight when its session
  ends no longer revives the session (clicked A 2 of 2, `ambiguous_capture` now); the focus
  click of an element-addressed `press_key` on a WebView host goes to the element's own
  point, read against no capture (clicked A, E now); `from_zoom` reads the calling
  session's own zoom of that window (clicked A after another session zoomed, E now). Not
  re-reviewed.
- **W14, R3-2 (N3).** WPF is refused on the pen route whatever `SM_DIGITIZER` says: any
  synthetic pen device, including this driver's own, sets it, and WPF dropped the taps
  even when it saw the tablet.
- **W13, R3-4 (N3).** A posted click whose target is WPF's `HwndWrapper` (an ElementHost in
  a WinForms form) refuses `background_unavailable`; it exited 0 with nothing fired.
- **W3, R3-3 (C5, C19).** Broken by the final review (F6): it skipped the children of a
  window answering `HTTRANSPARENT`; fixed after it, see below. One walk picks the window real input reaches: visible, enabled,
  the point in its rect and its window region, not answering `HTTRANSPARENT`;
  `WS_EX_TRANSPARENT` children are not skipped. Group boxes over their cells, an opaque
  `WS_EX_TRANSPARENT` child, a half-transparent child and a region corner now land where a
  real click does; the old driver missed or refused 28 such cases in `displays.ps1`.
  Windows virtualizes regions and the `WM_NCHITTEST` point for any caller, so the DPI
  scoping (`TargetSpace`) is gone.
- **W28.** The driver builds byte-identical from different source paths and `CARGO_HOME`s.
  "And from a CRLF checkout" was overclaimed (final review F15): a vendor clone made with
  `core.autocrlf=true` and patched afterwards built another driver. Fixed after the final
  review, see below.

## Windows final-review fixes

From the final [windows-final.md](../study/audit/review-2026-09-28/windows-final.md),
fixed on `fix/windows-final`
([windows-final-fix](../study/audit/windows-final-fix-2026-09-29/README.md)). Red is the
patch at `91098fd`, green the candidate, on the same host and the same probe bytes; each
guard was removed once and went red. D1 was re-reviewed on 2026-10-04 (Displays row: a cross-thread
case was missing and is fixed); the rest has not been re-reviewed.

- **D1 (F6, G2).** The input walk tries a window's own children before the window, and
  passes a window through only when nothing in its subtree takes the point and it answers
  `HTTRANSPARENT`. The reviewer's `final.ps1`: driver equals a real `SendInput` click in
  192 of 204 points, 0 differ (red: 36 differ, the opaque child in a plain `STATIC`, a
  `BS_GROUPBOX` and a WinForms `FlatStyle.System` `GroupBox`); the other 12 are the
  refused fake `HwndWrapper` class. Every W3 case that held still holds: regions at 125%
  and 150%, `WS_EX_TRANSPARENT`, the group box over its cells, a disabled parent with an
  enabled child.
- **D7 (G5).** A click on a WPF root whose point resolves to a hosted Win32 child that is
  not WPF (a WinForms control in a `WindowsFormsHost`) is posted to that child: 8 of 8
  land where a real click does (red refused all 8). WPF content is still refused.
- **D6 (F15, G1, G3).** `build.rs` embeds the DPI manifest with CR removed. A vendor
  clone with `core.autocrlf=true` patched afterwards, both repositories cloned that way,
  and a CRLF manifest in an LF tree all build the LF driver's hash (red: `C0956373…`).
  The `.gitattributes` pin is gone.
- **F16.** `build.ps1` (package manifest) and `driver-variants.ps1` (`hashes.json`)
  record rustc, the MSVC toolset, the Windows SDK and Visual Studio next to the driver
  hash. The hash is the same across hosts only with the same toolchain; another MSVC
  toolset was not measured (F18).
- **Acceptance.** Fresh host packages of `3d9706a` and `c9cad4c` (driver `8C652AD9…`):
  `acceptance.ps1` 10 of 10 with both controls, `native.ps1` 8 of 8 with 3 controls,
  `displays.ps1` 382 cases with 0 failures (host B), `-Mutate` failing as required (host
  C). Host C's plain `displays.ps1` hit a layout-tool error before any driver call and was
  not rerun, and its raw results were lost in a failed fetch; see the README.
- **Docs.** `hashes.json` values are strings (W20); the round-2 red count is 51 plus two
  ownership lines (W2b).

## Deferred and non-goals

40 requirements are non-goals, each with its source, in the
[requirements README](../study/audit/requirements-2026-09-25/README.md#retained-non-goals):
audio, microphone and dictation; Messages; the lock-screen subsystem; a live PiP mirror;
Appshot; broad TCC scopes and consent windows; Statsig, protobuf telemetry, the menu-bar
item, build flavors and the Codex config writer; the Chrome extension installer, XPC
audit-token auth and `loopStepLimit`; the uniqueness table and lazy refetch; the app
usage catalog, process enumeration, power assertions and Linux `drag_handle`;
UITextMarker ranges, corner masks and Stage Manager; macOS focus-guard layers 1 and 2;
the user-event recorder; the occlusion observer; and the Windows and Linux occlusion clip.

Deferred on 2026-10-04, not non-goals:

- **Sway window capture** by `ext-image-copy-capture` (sway 1.10 and later), not built.
- **Off-Space gather**, Space-move SPI and the live Space tracker stay non-goals on this evidence; if the owner wants other Spaces observed it is one native-patch item plus a multi-Space setup ([gaps-parked §6](../study/audit/gaps-parked-2026-10-04/README.md#6-review-of-the-other-rows)).

## Needs Nathan

1. ~~Developer ID signing and notarization for macOS.~~ **Done 2026-10-04.** The bundle is
   signed with Developer ID (G7P6939437), notarized (Apple status Accepted) and stapled
   (`app/scripts/sign-notarize-macos.sh`). On a real macOS 26.3.1 Mac it passes `spctl` as
   "Notarized Developer ID", installs from a quarantined zip with the normal first-open prompt
   and no override, and the text-area, Calculator and memory checks pass on that exact build
   ([final-mac-3](../study/audit/final-mac-3-2026-10-04/README.md)). Ad-hoc grants still pin to
   a build's cdhash, so a TCC grant made on an unsigned build does not carry over.
2. ~~The Windows certificate.~~ **Done 2026-10-03.** Azure Artifact Signing on a personal
   account (account `nzhaosigning`, East US, Basic; Public Trust profile `taiku-release-signing`;
   service principal `taiku-signing-ci`, signer role on that account only). On a fresh
   Windows Server 2022 host `sign-windows.ps1 build` signed every shipped PE (`app.exe`, both
   sidecars, the UIAccess worker, uninstaller, NSIS installer, MSI) with SHA-256 and a Microsoft
   RFC 3161 timestamp, all verify, the chain ends at Microsoft Identity Verification Root CA
   2020, the worker starts with UIAccess under the real signature, and no throwaway CA is
   trusted ([windows-real-sign-2](../study/audit/windows-real-sign-2-2026-10-03/README.md)).
   It found and fixed one bug: the .NET check read the 32-bit Program Files under makensis, so
   the NSIS uninstaller stayed unsigned while the build still said "signed and verified".
   On 2026-10-04 on a fresh host `build` verified the NSIS payload by name and every PE in it (10,
   `UserInfo.dll` included, which had shipped unsigned in a signed installer and is fixed) and in the MSI
   (3) was Valid under the real certificate; the MSI was installed per machine and run
   ([gaps-windows-2](../study/audit/gaps-windows-2-2026-10-04/README.md#1-real-azure-artifact-signing-build-payload-proof-msi-installed),
   [gaps-linux-windows](../study/audit/gaps-linux-windows-2026-10-04/windows-signing.md)). Separately, the daemon now starts the
   UIAccess worker on demand and has it deliver foreground keys and text to windows its integrity cannot
   reach, and since 2026-10-05 clicks, scroll and element-addressed typing into High windows and the foreground ones into SYSTEM windows (Verified, Windows; on 2026-10-05 also with the real certificate against real elevated Notepad, cmd, regedit, Services and Task Manager at High and SYSTEM, [gaps-windows-4](../study/audit/gaps-windows-4-2026-10-05/README.md#1-real-certificate-real-elevated-applications)). Still open:
   SmartScreen on Windows 10 and 11 is unobserved (Server shows no dialog, item 7); certificate rotation
   and validity after expiry, and the client secret expiring 2027-04-01 (item 8).
3. **Publishing updates.** Two decisions and a build rule; nothing was published or signed with the real key
   ([gaps-app](../study/audit/gaps-app-2026-10-04/README.md#updates-n16)).
   - *Where `latest.json` and the archives live.* The configured endpoint
     (`https://github.com/nathanjzhao/invisible-computer-use/releases/latest/download/latest.json`)
     returns 404: there is no repository of that name under nathanjzhao, and a private one would not
     serve an unauthenticated updater. Either create a public releases repository, or name an S3, CloudFront
     or R2 location; either way `endpoints` in `tauri.conf.json` changes. The Linux `latest.json` entries
     are `linux-x86_64-appimage` (the raw AppImage and its `.sig`), `linux-x86_64-deb` (the `.deb` and its `.deb.sig`) and `linux-x86_64-rpm` (the `.rpm` and its `.rpm.sig`), as `tauri build` writes them
     ([gaps-linux-3 updater.md](../study/audit/gaps-linux-3-2026-10-05/updater.md), [gaps-linux-4 updater](../study/audit/gaps-linux-4-2026-10-05/updater/README-updater.md)); none is published.
   - *The update key.* `~/.tauri/copilot-updater.key` matches the public key in `tauri.conf.json` (key
     id 89DD69820866D4E0), has no password, and is now mode 0600 (it was 0644; gaps-mac-live ran
     `chmod 600`). Keep it, with its only copy somewhere that is not a laptop home folder or a CI
     secret, or rotate it before the first release, while no client holds the old public key.
   - Every release is cut with `@tauri-apps/cli` 2.12.1 or later (`npm ci`; the main checkout's
     `node_modules` had 2.11.4), or the client refuses the signature (Known limits).
4. **The logged-in golden Codex trace.** `golden.sh login` on a throwaway Mac prints a
   device code; you open the URL, sign in and enter it
   ([runbook](../study/audit/golden-trace-prep-2026-09-25/RUNBOOK.md)).
5. **The physical keyboard check, with a Japanese input source** (needs you at a Mac;
   [gaps-mac-live §5](../study/audit/gaps-mac-live-2026-10-04/README.md#5-what-remains)). Every key in the runs so far was a
   CGEvent posted by a probe. On a Mac with the signed app and a Japanese source added in System Settings,
   add a window (TextEdit) to Copilot and click into its tile, then on the keyboard: type ASCII, press
   Return, switch to the Japanese source, type `a` and see the marked text stay inside Copilot's editor,
   press Return to commit, then Cmd-V. The target should get the ASCII, the newline, the committed
   hiragana and the pasted text, nothing from the marked state before the commit, with Copilot
   frontmost and the target in the background. Only hardware shows key repeat, dead keys, modifier
   release and a keyboard's own IME handling.
6. **A person at a real GNOME or KDE desktop, with a real GPU.** The 2026-10-05 sessions were auto-login on vkms and an
   Xorg dummy with consent answered by a permission-store pre-grant or AT-SPI (Verified, Linux package row). Not covered
   without a person and hardware: the first Screenshot consent and the RemoteDesktop consent answered by a human (within 15 s, or
   raise `CUA_DRIVER_RS_PORTAL_TIMEOUT_MS`; the Plasma 6.6 run skipped them with a screenshot pre-grant and KDE's `kde-authorized` entry), and the
   polkit prompt of a `.deb` or `.rpm` update read by a person
   ([shell.md](../study/audit/gaps-linux-3-2026-10-05/shell.md), [updater.md](../study/audit/gaps-linux-3-2026-10-05/updater.md)).
7. **A Windows 10 or 11 client image** (none is obtainable in AWS `default`; every Windows image there is
   Server). It is needed for SmartScreen on a download with the Mark of the Web (a new Artifact Signing
   publisher is expected to show "Windows protected your PC" until reputation builds, publisher
   "Nathan Zhao"; unobserved) and for the Store-app foreground-lock case (Calculator, Settings): run
   `store-app.ps1 -Dir 'C:\Program Files\Copilot' -App calculator -Case lock` with and without the worker
   ([gaps-windows-2 §5](../study/audit/gaps-windows-2-2026-10-04/README.md#5-uiaccess-worker)). On Server 2022 `LockSetForegroundWindow` did not stop the unpatched driver. Checked again 2026-10-04
   ([gaps-windows-3 §3](../study/audit/gaps-windows-3-2026-10-05/README.md#3-a-windows-10-or-11-client-image)), and again the same day ([gaps-windows-4 §5](../study/audit/gaps-windows-4-2026-10-05/README.md#5-windows-1011-client-image); us-west-2 and us-east-1: 928 `Windows_Server` and 90 `TPM-Windows_Server` Amazon images, "Enterprise" names are Server with SQL Server, no AppStream public image names Windows 10 or 11, and dedicated hosts and VM Import need a licence and a disk image from outside AWS): no client image in `default`; all 785
   Windows base AMI names are Server, the community and marketplace names that say Windows 10 or 11 are software versions
   (Deadline workers, Commvault images), and the WorkSpaces "Windows 10" bundles are Server based. It needs a machine from outside AWS
   (Windows 365, an Azure image or a physical PC); nothing paid outside AWS was used.
8. **Certificate rotation and the client secret.** Artifact Signing certificates last 72 hours and
   rotate; signatures should stay valid after expiry on the timestamp, which no session has tested. The check is
   ready ([gaps-windows-5 §4](../study/audit/gaps-windows-5-2026-10-05/README.md#post-expiry-check-needs-nathan-item-8)): a file signed on 2026-10-05
   by the certificate valid to 2026-10-07 08:08:32Z is saved
   ([signed-2026-10-05-uninstall.exe](../study/audit/gaps-windows-5-2026-10-05/results/item4/signed-2026-10-05-uninstall.exe), sha256 CFC35CA2...3B91) with a script. After 2026-10-07 08:08Z, run
   `app/tests/windows/gaps5/postexpiry.ps1 -File <that file>` ([script](tests/windows/gaps5/postexpiry.ps1)) once on any Windows machine; it prints the status, signer validity, timestamper and
   `signtool verify /pa /v` and exits 1 unless the signature is Valid. Running it settles this item's question about validity after
   expiry: Valid means the timestamp carries the signature past the certificate's end. Before expiry it printed Valid
   ([result](../study/audit/gaps-windows-5-2026-10-05/results/item4/postexpiry-check-2026-10-05.txt)). The earlier certificates' signed files went with their hosts. The client secret of the
   service principal `taiku-signing-ci` expires 2027-04-01 and is renewed in the Azure account; that part is a date, not a test.
9. **Product choices flagged.**
   - Decided 2026-10-05: a standard user who cannot elevate, updating an all-users install, is told to ask an administrator
     before anything is downloaded (not elevated: the updater plugin exits after launching the installer and cannot see the
     UAC answer). Closed 2026-10-05: an administrator who cancels the installer's prompt keeps the app and the install (an all-users NSIS
     update is now run by the app itself; per-user and MSI installs still use the plugin's install;
     [gaps-windows-4 §3](../study/audit/gaps-windows-4-2026-10-05/README.md#3-cancelled-uac-elevation-of-the-installer)).
      - *UIA at SYSTEM integrity.* Measured 2026-10-05 and not wired: the driver's walk run as SYSTEM names finer elements in real SYSTEM windows than the worker's child-window description does (Notepad 11, cmd 11, regedit 17, Services 32, Task Manager 8), but reading them from a SYSTEM client needs a SYSTEM service in the install that accepts requests from a medium-integrity app, a privilege boundary. Build it only if you want SYSTEM windows read as finely as High ones (the worker already reads UIA for High windows) ([gaps-windows-5 §2](../study/audit/gaps-windows-5-2026-10-05/README.md#2-uia-at-system-integrity)).
   - The default Codex and OpenAI-login model, `gpt-5`, is rejected for a ChatGPT-account login;
     which model should be the default (the provider's supported list moves).
   - A Linux `.deb` or `.rpm` update needs an administrator at a polkit prompt or a rule (measured 2026-10-05; the prompt was answered by synthetic keystrokes, not a person) while an AppImage updates in place with no
     privileges; which package is the release format.
   - Off-Space gather (Space-move SPI) stays a non-goal on the evidence; build it only if you want
     other Spaces observed.
10. **Parked requirements** (26 items; 20 were never dispositioned,
    [list](../study/audit/requirements-2026-09-25/README.md#never-dispositioned)). Each carries a `recommendation` in
    `requirements.json`; on 2026-10-04 all ten `build` items were built
    ([gaps-parked](../study/audit/gaps-parked-2026-10-04/README.md), [gaps-linux-2](../study/audit/gaps-linux-2-2026-10-04/README.md);
    Verified, parked items) and the `non-goal` and `hardware` reviews were confirmed.

    | Recommendation | Items |
    | --- | --- |
    | build (now built) | Linux full-desktop facade; per-value truncation descriptor; agent cursor in screenshots plus `post_action_sleep_ms`; file and process predicates; model, prompt and run metadata on recorded events; consult memory before broad discovery (measured: the action-based text plus the first-result `memory_prior` takes sonnet from 23 to 48 of 48 on plain prompts and haiku's lookups from 17 to 34 of 48, with haiku's artifact gain not significant, and a round 3 of five more variants shipped nothing; Verified, parked items); close-enough cursor targeting; two-phase prepare/approve/commit; unified `getState`/`getApp`; the length-prefixed Sky pipe |
    | non-goal | org-id provenance (two items; no organization identity exists to carry); virtual AX cursor; off-Space gather, Space-move SPI and the live Space tracker (see item 9); always-on-top band (conflicts with the overlay contract); OSWorld board (a harness, not a feature; its persistent-state axis now has a source in the predicates) |
    | needs you | UIAccess flash-free routing (partly built, Verified, Windows; the Store-app case is item 7); golden trace (item 4); the macOS physical-keyboard check (item 5) |
    | hardware | secondary displays: partial on virtual displays; physical monitors remain |

## Known limits

- **No sway window capture.** Sway 1.9 only offers output crops, and it draws layer
  surfaces, popups and clients it spawns itself (swaybar, swaybg) over windows without
  reporting where, so no crop can be shown to hold only the target. Every native sway
  window screenshot (the Wayland opt-in) is refused and readiness says so up front. A
  driver without the opt-in lists only XWayland windows and captures them as X11
  windows, which holds that client's own pixels and none of sway's. A per-toplevel capture
  (ext-image-copy-capture, sway 1.10 and later) would be the way back; it is not built.
- **X11 capture mark.** A reused XID is caught by a property this driver writes on the
  window. Any X client can read it, and can copy it onto a new window, after which an
  old `screenshot_id` is accepted. X11 has no client isolation to stop that.
- **Tree tag on macOS.** macOS leaves the environment out of `KERN_PROCARGS2` for
  platform binaries (`/bin/sh`, `/bin/sleep`), so an orphaned system tool that called
  `setsid` is still out of reach. So is any process that clears its environment.
- **Pixel binding.**

  | | click, double, right | drag | point scroll |
  | --- | --- | --- | --- |
  | macOS | bound | bound | bound |
  | Linux | bound | bound | bound, not re-reviewed |
  | Linux hold tools | `mouse_button_down`, `mouse_drag`, `mouse_button_up` bound | | |
  | Windows | bound | bound per endpoint (N5), not re-reviewed | bound (N1), not re-reviewed |

  Without an id, macOS, Linux and Windows read the pixel against the calling session's own
  latest capture of that exact window, and nothing else
  ([c9-unify](../study/audit/c9-unify-2026-09-27/README.md), one rule in
  `cua-driver-core`). The session is the call's `_session_id`: an explicit `session`, else
  the MCP proxy's, HTTP or stdio transport's, or a one-shot CLI call's own id. Other
  sessions' captures, the app's tile included, never rescale the pixel and never make it
  refuse. Each session's latest capture per window is kept apart from the 4096-frame log,
  bounded at 256 sessions of 256 windows, least recently used first out; a forgotten
  entry refuses. No capture of the window by this session, or no session at all (a raw
  socket call without `session_id` or `session`), refuses `ambiguous_capture`. A resize,
  scale change or a new window under the same id since the session's capture (an X11
  XID, a reused Windows handle) refuses `stale_capture`. A capture counts only once it is
  finished and its call has returned it: a Windows `get_window_state` that timed out does
  not file its late capture. A capture is filed under the session its call had when the
  capture started, so a session that ends with a capture in flight does not get it back
  after a revival; only a capture started after the revival counts
  ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md)).

  `from_zoom` pixels follow the same rule for zoom images: they are read through the
  calling session's own latest zoom of that exact window, never another session's or
  another window's. No such zoom refuses `invalid_arguments`; a resize, scale change or a
  new window under the same id since the zoom refuses `stale_capture`. On Linux `click`,
  `double_click`, `right_click`, `drag` and the hold tools take `from_zoom` and read its region in
  screenshot pixels (checked 2026-10-04); elsewhere only `click` and `drag` are documented to take it,
  not rechecked; `scroll` refuses it. A
  focus click the driver computes from an element's own bounds (an element-addressed
  keystroke on a WebView host) is not a caller's pixel and is not read against any capture.

  One `icu` process is one driver session: its captures, its input and its settle polls
  carry the same session label, which includes the process id. Two accepted consequences,
  both named in the refusal text:
  - A pixel on a window this session never captured refuses, including native sway windows,
    whose capture is itself refused. Those pixels used to go through unscaled. Act on an
    element (`element_index` or `element_token`), or capture the window first.
  - A one-shot `icu call` (or `cua-driver call`) is its own session, so its unbound pixels
    refuse. Pass the `screenshotId` of the capture (`screenshot_id` to the driver), or
    keep one `icu mcp` (or `cua-driver mcp`) session, which on macOS, Linux and Windows
    lands its own unbound pixels ([c9-final-fix](../study/audit/c9-final-fix-2026-09-29/README.md)).

  The check compares window size and scale, and on X11 and Windows the window's identity
  mark: content that reflows or scrolls keeps an id valid. Surfaces in `screenshots[]`
  cannot be bound.
- **WPF and other toolkits.** WPF pixel clicks are refused, not delivered (N3), including
  WPF hosted in a WinForms form and whatever digitizer is present; no real digitizer was
  available to test. WinForms hosted in WPF (`WindowsFormsHost`) is not WPF content and
  takes posted clicks (final review D7; not re-reviewed). UWP and WinUI were not
  available on Server 2022. Linux runs used GTK3, GTK4, Qt5, Electron and Tauri fixtures
  under Xvfb and headless compositors, and in 2026-10-05 GNOME Shell and Plasma sessions on vkms and an Xorg dummy (Linux package row), never physical input or a real GPU.
- **Tokens and toggles.** A Linux token refuses once its label changes, including a
  Play/Pause toggle, and including an unnamed entry whose label is its own text after
  this driver typed into it; the model has to observe again. A named entry keeps its
  token across writes (GTK3 `set_name`, Qt5 `setAccessibleName` or a `QLabel` buddy). A
  GTK3 mnemonic label does not name its entry, so that entry counts as unnamed.
- **Facade.** The cursor-drawn probe ignores the top-left 32x32 pixels, because the driver's own overlay window shows up there in the next capture (the probe failed 2 of 3 unmutated runs before that); a layer that draws at (0, 0) would not be seen by it. The Linux desktop facade's `desktop_*` names are inferred and its approval is once per desktop; a physical pointer on a Wayland seat was not exercised; on Wayland the desktop is the first output only;  an X11 output scale was driven only on Xorg's modesetting driver over vkms (the dummy-class servers refuse it); a Wayland drag handle needs a driver that lives between calls (a one-shot call refuses it, on GNOME and KDE over libei as on sway); and the layer's desktop tools were not run in the real GNOME and Plasma sessions (Verified, parked items, and Linux package row).
- **UIAccess forwarding.** A background `drag` into a window above the daemon's integrity refuses with the UIPI text (a foreground one is forwarded since 2026-10-05). Into a SYSTEM
  window only the foreground paths work: posted messages fail with `Access is denied` even for the worker, so background
  clicks, scroll, typing and `set_value` refuse, and the window is described by child window handle, so a control with no
  child window (WPF, Chromium, UWP) is one pane and element-addressed typing there is unavailable. Tested on WinForms fixtures (throwaway CA) and, with the real certificate, on real Notepad, cmd, regedit, the Services
  console and Task Manager at High and SYSTEM ([gaps-windows-4](../study/audit/gaps-windows-4-2026-10-05/README.md#1-real-certificate-real-elevated-applications)); not on an installer, and no real application with a control that has no child window (WPF,
  Chromium, UWP) was driven. The denial for SYSTEM windows is UIPI: the worker is a High sender and its posts to a SYSTEM window
  fail with error 5 unless the window opts in; a SYSTEM sender would need a SYSTEM service in the install, a decision not made.
  UIA read by a client at normal or High integrity gives no finer elements at SYSTEM; the driver's walk run as SYSTEM names finer ones (Notepad 11, cmd 11, regedit 17, Services 32, Task Manager 8, against child windows only) but that needs a SYSTEM service, not built (Needs Nathan, item 9; [gaps-windows-5 §2](../study/audit/gaps-windows-5-2026-10-05/README.md#2-uia-at-system-integrity)).
  Foreground input into a normal-integrity window with no worker was a product bug in three parts and is fixed (Verified, Windows, round 3); a console or other window left over the target is refused `foreground_unavailable` by design, and the High fixture harness is not fully robust against a covering console. Open, High `scroll_px_fg_decoy` (a foreground wheel with another window covering the target) is flaky on a fresh host: on the gaps-windows-5 patch it passed 9 of 16 runs in the first 70 minutes or so of the host and 25 of 25 runs 2 to 3 hours in (12 idle, 8 under 3 busy loops, 5 in the original sequence); SYSTEM decoys passed 19 of 20, the miss in the early hour. It is not a merge regression: the Windows code in that patch and in HEAD's is byte-identical. Each failure was a lost wheel: the covering window stayed over the aimed point, the driver answered `delivery foreground, effect unverifiable`, neither window got a wheel event, and a read 4 seconds later was unchanged. The cause was not found. A race in `send_wheel_synthesized` (`SetCursorPos`, a queued `SendInput` wheel, a fixed 20 ms wait, `SetCursorPos` back) is plausible and not proven; a candidate that batched the move and the wheel was built and dropped, because the unmodified build then passed 25 of 25 and the fix could not be shown to change anything. No product change was made, and waiting for the host to be up an hour was not tested as a remedy ([gaps-final-build-3 §1.2, §1.3](../study/audit/gaps-final-build-3-2026-10-05/README.md#12-high-scroll_px_fg_decoy)). `get_window_state` of a large elevated window through the worker is bounded by a 15 s
  walk. One intermittent foreground-click refusal was seen in about 20 SYSTEM runs of an earlier build (a refusal, never a wrong
  delivery; it did not recur). One worker per user (a fixed pipe name: a second daemon of the same user is rejected and gets the
  UIPI error with the reason). A per-user install gets no worker. The packaged sidecar must be rebuilt
  from the patch to carry it ([gaps-windows-3 §1](../study/audit/gaps-windows-3-2026-10-05/README.md#limits-that-remain)).
  `impl/DISTRIBUTION.md` still calls the Calculator and Settings foreground lock vendor documentation:
  it was not reproduced here.
- **Memory-first.** Measured on outcomes (Verified, parked items; [gaps-memory-4](../study/audit/gaps-memory-4-2026-10-05/README.md), [gaps-memory-5](../study/audit/gaps-memory-5-2026-10-05/README.md)): with the action-based text and the
  first-result `memory_prior`, on prompts that never mention earlier work sonnet applies the stored fact in 48 of 48 runs (23 of
  48 before) and is at the ceiling on 12 fact families (0.94 case level); haiku looks memory up in 34 of 48 (17 before) but its artifact gain in round 2 (22 against 15 of 48) is not significant, it scored 0.28 case level on 12 families in round 3, and its run-to-run spread is large (4 to 15 of 48 on one surface). Round 3 found an action-triggered prior that takes haiku to 0.71 and missed two registered guards by a small margin (stray lookups, turn-cap loops), so it is not shipped and a new registration is needed. The inline-entries variants are not shipped. One scripted app, 12 fact families, two models.
- **Sky pipe.** One protocol on a unix socket or a Windows named pipe, a client of Codex's own code
  checked against the real drivers; the 4096-character value clip, its marker wording, a 10-notch page
  scroll and the 100 ms settle default are inferred, not recovered from the Sky binary.
- **Codex default model.** The app's default `gpt-5` (and `gpt-5.2-codex`, `gpt-5.1-codex`) is rejected by a
  ChatGPT-account Codex login with HTTP 400; `ICU_OPENAI_MODEL` has to name a supported model
  (gpt-5.5 worked through ccodex). The error message is accurate; the default is stale and was not
  changed, since the provider's list moves.
- **Release builds.** `plugins.updater.requireSignedVersion` is on, so a release must be cut with
  `@tauri-apps/cli` 2.12.1 or later: an older CLI writes no version into the signature and every client
  refuses it. `npm ci` picks 2.12.1 up; a stale `node_modules` did not (2.11.4).
- **X11 off-root capture with ImageMagick** returns the visible part as a success rather
  than `window_offscreen` (linux-review-fixes Limits).
- **macOS scale and Spaces.** One virtual display at 1x and 2x. Only `CGSSpaceCreate`
  Spaces were tested, not Mission Control desktops.
- **One-off failures not explained:** an extra newline after committed IME text on build
  A, once in 12 runs; one first manual tile click on M5 that predates the prompt fix; one
  GUI smoke failure that did not recur; a UIA "Unrecognized error" on each Windows host's
  first acceptance run; `blocked_stdin_deadline_kills_child_and_connection_stays_failed`
  once in a combined run; the `cli_failures...` flood case at load 298, which fails the
  same on unchanged code; no deterministic red for `_click_tile`.

## Confirmation gate and frontend, after the rest-correct review

Fixes and decisions are in [fix-rest-correct](../study/audit/fix-rest-correct-2026-10-01/README.md). What the gate still does not cover, and where it still over-asks. The App memory row above lists the gate as it was before this change; where they differ, this section is current.

- **Gate list changes.** `forward` and a bare `merge` are no longer consequential (Forward opens a draft, Merge Cells formats a table; `merge pull request`, `squash and merge` and `rebase and merge` still ask). A `Trash` row, cell or list item (a sidebar folder) does not ask; a Trash button or menu item does. `Checkout` asks except in a version-control client or editor. `unsubscribe`, `sign out`, `log out` and `clear all` ask.
- **Custom actions.** A click whose accessibility action is `Name:...` is read by that name; an unknown `Name:` action on a row or cell asks. Any other unknown action string falls back to the control text.
- **Answer buttons need a dialog in the tree.** OK, Yes, Continue, Accept, Allow, Overwrite, Replace, Install, Reset, Proceed and Agree (as the first word of a label of three words or fewer) ask only when an element with `sheet`, `dialog` or `alert` in its role or subrole is on screen. The gate never reads the goal, so an OK that commits something in a plain window with no dialog role is not asked about, and a dialog drawn without those roles is missed. Apps whose dialogs are plain groups (some Electron apps) are the likely miss.
- **Return.** Not asked for when the focused element is a multi-line text area (`textarea`, `text area`, `multiline`, `richedit`, `document`), unless the app is a chat app or terminal. Still asked for when the focus is unknown (Windows and Linux never report it) and the window shows a consequential control, and in a single-line field. So on Windows, where a multi-line box shows as a plain `Edit` with no focus marker, Return in it still asks whenever a Submit or Send button is on screen (seen in the Windows final run); that is an over-ask, the safe direction. Shift+Return is not asked for outside a terminal. VS Code, Cursor, VSCodium and Windsurf ask only when the focused element mentions a terminal, or no focus is known and a terminal is on screen; Return in their editor does not ask, so a run typing a command into an editor terminal that the tree does not show as a terminal is missed.
- **Delete and Backspace.** Any Delete chord in a focused text field is not asked for. With a focused non-text element it asks. With no focus reported (Windows, Linux) it asks only in a file manager or mail client (by name or bundle id) or when the tree has a list or table and no text field; elsewhere it does not, so a Delete on a selected item in an app outside those lists, with a text field also on screen, is missed.
- **Still not covered.** Page shortcuts with no control to read (Gmail `#`, `e`); control text in another language or an icon with no text; `alt` with Return; anything done outside the window.
- **Allow after Stop, reload, background.** An Allow that arrives with a Stop does not run the step. A reloaded window redraws the open question from `agent_status`. When the window is in the background the dock icon bounces once or the taskbar button flashes (informational, no focus taken); there is no notification text, so a user who has hidden the dock and the taskbar can still miss it for the 120 s.
- **Windows per-user install.** The UIAccess helper cannot start from `%LOCALAPPDATA%`; the app warns by install path (it cannot read an `EnableSecureUIAPaths=0` policy, so the warning is wrong there) and `installMode` stays `both`. The 2026-10-04 update runs used per-user installs too and the worker was absent there as documented, installed outside Program Files it starts with UIAccess 0 ([uiaccess.md](../study/audit/gaps-linux-windows-2026-10-04/uiaccess.md)); the warning text itself was not looked at. An update keeps the scope of the install it replaces (Updates row).
- **Window picker on other Spaces.** `project_windows` drops an untitled window whose `is_on_screen` is false. A window on another Space reports false, so when the driver gives no titles (Screen Recording denied) the off-Space windows are missing from the picker. Not reproduced: it needs a Mac with that permission denied.
- **Window fit.** `fitted_size` has no floor, so a zero-size work area would give a 0x0 window; `work_area()` falls back to the monitor size, so it is not expected. Not changed.
- **Cross builds.** `stage-driver.sh` stages `cua-driver-<host triple>`, so `tauri build --target <other>` needs the sidecar made by hand.

## Linux text area and reload, after the final Linux run

- **GTK TextView, background typing.** The Linux driver refuses a click on a GTK TextView with `background_unavailable` (no focus-free input backend) and advises `delivery_mode:"foreground"`. The app now removes any `delivery_mode` advice from a driver refusal before the model reads it and says instead that the app never fronts a window and to try `set_value` on the control (AT-SPI EditableText, already in the action schema), or finish without it or reply `fail`. Not verified on a Linux GUI: whether `set_value` takes on that TextView (the fixture's tree row for Body is a label with no actions and no value, so the control the model must name may be a different row). If the toolkit does not expose EditableText there, the model can only report that the control cannot be typed into in the background; fixing that needs a driver change (not made).
- **Reload mid-task.** Found by reading, not reproduced on a GUI: after a reload the feed was empty and the run's window was not watched again, so a next goal with no `@window` got "Reference a window with @" and did not start; the memory chip was only refreshed by a 20 s poll. The restore now loads the window list, watches the running task's window again and refreshes the chip at restore and when a task ends. A window that cannot be found gets a tile with zero bounds, marked closed after three scans. The webview reload was not re-driven on Linux; the vitest checks the source shape only (main.ts has no DOM harness).

## macOS text area click, after the second final Mac run

- **Click on an AXTextArea.** The macOS driver used to AXPress a text control that advertises no AXPress and got -25206. A plain click on AXTextArea, AXTextField, AXSearchField or AXComboBox without AXPress now writes AXFocused on the element (no window raise, no activation) and checks it is the app's focused element. If the write does not take, the driver answers a `background_unavailable` refusal (effect refused) whose text names set_value and finish/report, with no foreground advice. Verified on a macOS 26.3.1 Namespace Mac with the NSTextView fixture (study/audit/fix-mac-textarea-2026-10-01). Also verified on the notarized app with the real model ([final-mac-3](../study/audit/final-mac-3-2026-10-04/README.md): Body `line one\nline two`, 0 ownership violations). Not verified: a text control whose AXFocused write is refused (no fixture produced one; the refusal text and its pass-through to the model are unit tested), WebKit/Electron text areas (they advertise other actions or go through other rungs).


## Linux History interaction continuation, 2026-10-12

H1 Linux now has one production X11 collector for checked actions, focused field
state and accessible window snapshots. [Signed H evidence](../study/audit/parity-linux-history-2026-10-12/evidence/signed-h-verified.json)
passes ten capture cases, block-rule precedence/recovery and actual screen-lock
exclusion. It keeps delayed semantic attribution explicitly incomplete and
starts no second recorder or observer host.

[Signed I](../study/audit/parity-linux-history-2026-10-12/evidence/signed-i-verified.json)
closes H's observed terminal timeout and idle-reconnection failures in the native
fixture. Four production terminal checks pass, including a prompted ordinary-word
secret re-echoed later. Excluded-app CPU is 5.64% of one core, with 26 stable
threads, compared with H's 26.28% and thread churn. H's failures remain retained.
Ten uninterrupted X11 navigation cycles also pass 123 native checks.

Remains open: Source-time action binding,
virtual actions, Tab/Escape/Alt-only records, Wayland interactions and browser
origin/private-mode coverage remain open. H2/H3/H4/M1/M2 still need their Linux
native and model acceptance, including Wayland and a real mid-task reload.
Reference comparison still needs the isolated Linux reference sign-in.

2026-10-15 Qt5 backend checkpoint: [actual native edit/export and backend verification](../study/audit/parity-linux-qt5-capture-2026-10-15/evidence/backend-verified.json) proves the existing XCB/XWayland toolkit path works on the owned KDE Wayland session. Native Qt5 Wayland still exposes no Draft control. This is component evidence, not signed History capture or a reference exclusion. H1-L remains open.

2026-10-15 Windows H2 checkpoint: [38 signed native lifecycle checks and three actual hosted exchanges](../study/audit/parity-core-windows-2026-10-15/README.md#signed-c-review-and-deletion-lifecycle) pass, with seven failed native attempts retained. Reference sign-in is the remaining H2 comparison gate. A new shared M1/H4 regression remains runnable: imported journals reapply shape redaction to ordinary reviewed native identifiers after the hosted classifier returns no secrets. Capture and review themselves retain those names. No full cell promotion.

2026-10-15 GNOME capture checkpoint: [signed F native and raw-receiver verification](../study/audit/parity-linux-core-capture-2026-10-15/README.md) closes the missing Return observation, with 124 batch assertions, eight actual helper migration checks and 25 independent key/capture checks. The previous shortcut failure and Qt5 native Wayland provider failure remain retained. H1-L stays open for the other named coverage; no full cell promotion.

2026-10-15 Qt5 compatibility checkpoint: [signed F on KDE Wayland with the Qt5 XCB backend](../study/audit/parity-linux-core-capture-2026-10-15/evidence/plasma-xcb-verified.json) passes all 16 native toolkit checks, including real export and privacy controls. Native Qt5 Wayland remains a separate provider limitation. H1-L stays open; no N/A or full-cell promotion.

2026-10-15 Windows replay checkpoint: [signed D full candidate contract](../study/audit/parity-core-windows-2026-10-15/README.md#signed-d-recorded-replay-and-controls) passes all 16 conditions with 23 native assertions and 14 hosted exchanges. The live Windows executable-identity defect is fixed. H4-W awaits only its signed-in reference comparison; M1/M2 and broader capture work remain runnable. No whole matrix cell promotion.

2026-10-15 imported-memory counterexample: [signed E journal and hosted summary verification](../study/audit/parity-recorded-field-memory-2026-10-15/evidence/verified-e.json) passes 24 native journal checks across three platforms, then demonstrates Mac summary validation dropping model-proposed Juniper1 facts. M1 macOS and Linux are reopened for this shared path; Windows M1 remains open. Active core count: eight proven, ten open. M2 direct History summaries keep their separate scope. Signed F generation/reuse work is runnable.

2026-10-15 imported-memory closure: [signed G verification](../study/audit/parity-recorded-field-memory-2026-10-15/evidence/verified-g.json) passes 71 Mac/Windows/Linux assertions and 25 actual hosted exchanges from reviewed native capture through durable facts, restart, exact recall and memory-off UNKNOWN. [Scoped unchanged dependencies](../study/audit/parity-recorded-field-memory-2026-10-15/evidence/inheritance-g.json) restore M1 macOS and Linux; E/F failures remain. Active count: ten proven, eight open. Continue H1, Windows full M1/M2 and G08; the user-only stop condition is not met.

2026-10-15 Windows M1 continuation: [seven signed G isolation and concurrent-deletion checks](../study/audit/parity-core-windows-2026-10-15/evidence/isolation-g-verified.json) pass with seven real hosted exchanges. Wrong-app requests and post-delete requests contain no stored identifiers; releasing a real action plan after deletion causes no native effects. Current correction, pruning, full M1/M2 and G08 work remains runnable. No cell promotion.

2026-10-15 Windows correction continuation: [signed G correction verification](../study/audit/parity-core-windows-2026-10-15/evidence/correction-g-verified.json) passes 29 native assertions and 14 hosted exchanges. A second native recording and generated summary replace the recalled product and day; restart retains the correction, while memory-off gives UNKNOWN without stored identifiers in provider requests. Concurrent pruning, remaining M1/M2 runtime controls and reference comparison stay open. No cell promotion.

2026-10-15 browser capture continuation: [signed KDE Chrome verification](../study/audit/parity-core-browser-capture-2026-10-15/evidence/verified.json) passes all 14 frozen native checks with eight independent page receipts. Consent, focused fields, domain-only metadata, protected fields, blocked domains, private mode, pause/resume, revocation and native recovery pass in one cohort. Ten failed setup attempts remain. H1 retains other sessions, timing and origin/iframe boundaries; no cell promotion.

2026-10-15 storage-lock continuation: [signed H storage verification](../study/audit/parity-memory-storage-2026-10-15/evidence/verified.json) passes 38 current native assertions and 14 actual hosted exchanges across Mac, Windows and Linux. Retained G dispatches despite lock-file denial; H withholds the real action plan, prevents unsafe recall/writes and recovers after exact permission restoration. Original memory data and reference credentials remain preserved. Retry/quota persistence and other G08 conditions remain runnable; no cell promotion.

2026-10-15 embedded-frame continuation: [signed J native verification](../study/audit/parity-browser-frames-2026-10-15/evidence/verified-j.json) passes nine checks with six independent page receipts. Same-origin field capture, changed/foreign frame exclusion, top-level origin exclusion and recovery pass together. The earlier missing-field result and package-binding refusal remain. H1-L retains its other declared sessions and timing/attribution work; no cell promotion.

2026-10-15 retry continuation: [signed I verification](../study/audit/parity-memory-retry-2026-10-15/evidence/verified-i.json) binds 92 current native assertions and 37 genuine upstream exchanges, with six separately identified transport fault responses. Cooldowns survive restart on all three platforms, then expire through ordinary elapsed time and recover exact recall/off behavior. Mac transient backoff and genuine recovery also pass. The old Windows restart failure remains. Linux runtime-state and journal observations are hash-verified from immutable copies; its superseded collector cache is explicitly unavailable. Judge/provider/same-ID controls, Windows full M1/M2 and capture work remain runnable; no cell promotion.

2026-10-15 typing-fragment continuation: [signed K verification](../study/audit/parity-typing-fragments-2026-10-15/evidence/verified-k.json) passes 24 native checks across Mac capture/import and Windows/Linux review, with one actual hosted classifier exchange. Typing bursts remain partial input; complete observations retain named identifiers. Legacy rows get the same distinction. H1 timing, target attribution and casing remain open; no cell promotion.

2026-10-15 Qt5 native Wayland continuation: [signed M verification](../study/audit/parity-qt5-launcher-2026-10-15/evidence/verified-m.json) passes all 16 frozen toolkit checks through the explicit production compatibility launcher. Two native component cases pass 18 checks; the signed L readiness/memory regression passes 10 checks and five genuine hosted exchanges. Password, embedded-web, other-process and pause/resume exclusions remain effective. The L interpreter failure and initial M same-version install refusal are retained. Other H1 sessions, browser coverage and event-time attribution remain runnable; no whole-cell promotion.

2026-10-15 Windows summaries continuation: [signed K verification](../study/audit/parity-windows-summary-controls-2026-10-15/evidence/verified-k.json) binds 29 native checks and nine genuine hosted exchanges. Unattended summaries include the later native source observations; restart retains exact earlier literal recall, while memory-off returns UNKNOWN. Two standard-user instances keep one collector. Genuine responses held through generation pause and History deletion cannot write afterward, including after restart. The original profile is sealed before its deliberate deletion. Actual clock/concurrent pruning, terminal model boundaries, incomplete observations and W-AUTH remain; no full-cell promotion.

2026-10-15 Qt5 second-desktop continuation: the same signed M package passes [all 16 native toolkit checks on GNOME Wayland](../study/audit/parity-qt5-launcher-2026-10-15/evidence/gnome-m-verified.json), in addition to the KDE cohort. The explicit launcher works with the two installed Qt5 versions. Other H1 session/toolkit, browser and event-time checks remain; no full-cell promotion.

2026-10-15 Windows actual-clock continuation: [signed K clock verification](../study/audit/parity-windows-summary-controls-2026-10-15/evidence/clock-verified-k.json) binds 12 native assertions and ten genuine hosted exchanges across the retained A/B attempts. A real 15-minute rollback yields a grounded late-event summary; a three-day forward jump prunes raw activity with two instances alive; actual restoration yields new summaries that survive restart. Time services, expiry and owned recovery tasks are restored/removed. Terminal/incomplete summary boundaries, in-flight chat-memory pruning, G08 and W-AUTH remain; no full-cell promotion.

2026-10-15 Windows M2 candidate contract: [all 19 frozen conditions have signed K evidence](../study/audit/parity-windows-summary-controls-2026-10-15/evidence/m2-candidate-coverage.json). The terminal/partial continuation adds 14 selected native assertions, with three native setup failures retained. Actual Windows Terminal secrets stay outside capture/model requests, and a complete 40,240-character export yields an explicitly partial, grounded summary. Three of eight upstream exchanges finish after native exit and do not count as published-summary proof. W-AUTH and the shared G08 register remain; M2-W stays open.

2026-10-15 Windows concurrent-pruning continuation: [signed K verification](../study/audit/parity-windows-prune-inflight-2026-10-15/evidence/verified-k.json) passes 16 native assertions with three genuine hosted exchanges. A second standard-user app prunes real imported journals while the first extraction response is held, advances the generation, and prevents late source/summary resurrection. Fresh-process recall is UNKNOWN with zero extra effects. Actual clock/time services/expiry are restored and owned helper tasks removed. W-AUTH and G08 judge/provider/same-ID work remain; no whole-cell promotion.

2026-10-15 Mac capture counterexample: [signed K/N/O and paired-reference verification](../study/audit/parity-mac-capture-timing-2026-10-15/evidence/before-verified.json) retains the cross-field typing, casing and O main-queue crash failures. The same native input is captured exactly by the signed-in reference. Native target retention improves N attribution; AppKit decoding needs the application main queue. The next signed revision is pending, so H1-M stays open and no cell is promoted.

2026-10-16 signed Q batch: [13 native settings checks on Mac/Windows/Linux](../study/audit/parity-memory-settings-2026-10-15/evidence/settings-verified-q.json) close the G08 concurrent-form-save bug. A status refresh can no longer make a retention-only edit re-enable a peer-disabled memory source. [Mac capture/reference verification](../study/audit/parity-memory-settings-2026-10-15/evidence/capture-verified-q.json) passes 18 candidate and four reference assertions: all four words have exact fields/case/source times, with native privacy and pause/resume intact. The accepted comparison is sequential; concurrent observers degraded capture and that failure remains. All signing inputs/grants are removed. G08 judge/provider/same-ID and broader H1 remain runnable; no whole-cell promotion.

2026-10-16 current-package action dependency: [the Q hosted Windows task](../study/audit/parity-driver-coherence-2026-10-16/README.md) exposes a stale retained sidecar after the controller adopts the versioned CLI result envelope. Project changes once and the app stops without replay; Note/Export do not run. Q settings/capture results keep their bounded scope, but Q cannot inherit previous packages' action/replay proofs. R rebuilt the matching tracked native drivers. The signed S continuation passes ten native action checks and 13 task-bound hosted exchanges across all three platforms; full recorded replay refresh and the separate extraction failure remain runnable.

2026-10-16 coherent action continuation: [signed S verification](../study/audit/parity-action-delivery-2026-10-16/evidence/verified-s.json) confirms exact field changes and one native export on Mac, Windows and Linux. Real driver errors remain non-retryable. The unsupported Linux fixture name and Windows extraction failure remain retained. Full replay refresh, G08 and broader H1 remain open; no whole-cell promotion.

2026-10-16 continuous Mac capture: [signed T verification](../study/audit/parity-mac-continuous-2026-10-16/evidence/verified-t.json) passes 26 candidate and four sequential signed-in reference assertions. Both record all 20 fields and ten exports; exact native files, source times, one collector and privacy controls pass. Q missing/split-event failures remain. H1-M retains lifecycle/browser/ordering breadth; no whole-cell promotion.

2026-10-16 GNOME browser continuation: [signed S verification](../study/audit/parity-core-browser-capture-2026-10-15/evidence/gnome-s-verified.json) passes 23 native privacy/origin assertions with 14 independent page receipts. The full browser and frame cohorts now pass on both GNOME and KDE Wayland. Other H1-L session/toolkit and timing checks remain open; no whole-cell promotion.

2026-10-16 extraction and judge continuation: [signed U verification](../study/audit/parity-memory-extraction-wording-2026-10-16/evidence/verified-u.json) passes 25 native assertions and 25 genuine hosted exchanges. Mac/Linux retain valid preferences when asked wording is rejected. Windows enforces the real 179.96-second judge deadline, withholds trust and recovers a quoted preference after a fresh task and restart. Earlier extraction failures remain. G08 same-ID/provider-change controls and current replay/H1 work remain open.

2026-10-16 same-ID concurrency: [signed Mac V verification](../study/audit/parity-memory-concurrent-entry-2026-10-16/evidence/verified-v.json) proves 12 fixed native checks with 16 genuine before/after exchanges. Old U discards a peer's trust and quote after its own timeout; V preserves the peer entry and restart. New/existing-ID evidence collisions and pending retry pass component regressions. Windows/Linux native concurrency, provider changes and the transient-status follow-up signed check remain runnable.

2026-10-16 provider correction: [signed Mac Y verification](../study/audit/parity-memory-provider-changes-2026-10-16/evidence/verified-y.json) binds 17 native checks and 11 genuine CLI exchanges. X learns copper but retains stale cobalt; Y retires cobalt after reviewing only its previously approved quote against the new provider's own later goals. Restart and fresh recall export copper; memory off exports UNKNOWN and excludes both stored colors from requests. Earlier failures remain retained. Windows/Linux native concurrency, resolved-status checks and the Linux readiness failure remain runnable; no whole-cell promotion.

2026-10-16 current capture/replay refresh: [signed V Windows replay](../study/audit/parity-core-refresh-2026-10-16/evidence/h4-v-verified.json) passes all 16 frozen conditions with 23 native checks and 14 genuine hosted exchanges. [Both signed V Linux Wayland capture batches](../study/audit/parity-core-refresh-2026-10-16/evidence/capture-v-verified.json) pass 280 checks, including 20 independent exports and both production Qt5 launcher cohorts. The failed old-wrapper selection remains. H4-W still awaits reference sign-in; H1-L retains the other sessions, timing and shortcut coverage. The separate Linux Y readiness failure remains runnable work; no whole-cell promotion.

2026-10-16 signed Z continuation: [independent verification](../study/audit/parity-readiness-pixels-2026-10-16/README.md) binds eight Mac recall checks, 13 Mac conflict/status checks and a separate successful restart after the main restart failed. Linux Z fixes the changing-PNG readiness failure but its frozen cohort stops at 26/52 after daemon admission contention; the 14-check keyboard diagnostic is not a full-cohort pass. [FIFO admission](../study/audit/parity-daemon-contention-2026-10-16/README.md) passes component tests and awaits signed full-cohort acceptance. Windows/Linux same-ID and broader H1 work remain runnable. No whole-cell promotion.

2026-10-16 Windows capture timing: [signed Z counterexample](../study/audit/parity-windows-capture-timing-2026-10-16/README.md) binds ten exact exports and 372 rows, but five final Note values first appear after Export. Native UIA diagnostics expose all ten final values before Export. The pending collector value-notification path still needs native privacy/timing and signed-app acceptance; component success is insufficient. H1-W remains runnable.

2026-10-16 signed native Windows value path: [VT_EMPTY probe verification](../study/audit/parity-windows-capture-timing-2026-10-16/evidence/probe-h-verified.json) retains all ten final Notes before Export through a guarded current-value read, with 21 rows, no partial result and no password text in the History store. The production History module compiles with the path and 16 component tests pass. This is a signed-component and production-module compile result, not yet a signed installed app, so H1-W remains open pending a current signed candidate and reference comparison.

2026-10-16 signed current-aa Linux continuation: [sealed evidence](../study/audit/parity-core-recovery-2026-10-16/evidence/linux-current-aa-verified.json) binds the current source, signed installed package and genuine hosted exchanges. The complete GNOME Wayland frozen run passes 51/52 checks; its only false check is cancellation timing because the provider completed before the native Cancel click. A replay-scoped rerun with a bounded provider hold passes cancellation. Missing probe helper modules and unavailable relay attempts remain retained as failed setup evidence. No core cell is promoted until the full signed cohort and required reference comparison are complete.
