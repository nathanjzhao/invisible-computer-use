# Computer History controls

This component owns observation rules, capture lifecycle state and local activity
storage. The desktop app uses it for approved native capture and feeds the
same approved observations into the memory summarizer's event stream.

App identity remains the default. macOS and Windows support separately approved
focused control labels and text values. Linux now implements these scopes through
AT-SPI; focused capture and privacy controls pass on signed candidate G. Secure fields are withheld.
macOS browser capture has a separate opt-in for normal Chrome/Chromium HTTP(S)
pages. Native interactions have their own scope on macOS, Windows and Linux X11.
The Linux collector combines source coordinates and shortcuts with checked
AT-SPI field values, window snapshots and redacted terminal text. Delayed
semantic attribution is marked incomplete. Linux Wayland interactions and
browser capture remain unavailable. Full parity stays governed by
the platform acceptance matrix.

## Capture scope

The host panel offers app identity, focused control labels, and labels with text
values. Expanding scope needs fresh approval based on the current settings
revision. Existing consent migrates to app identity only, with browser capture off.
Model controls cannot expand either scope. Accepted app/domain context reaches the
automatic memory summarizer. Sharing observed text with a model needs separate
content approval. Use Review for memory to approve a recording and provider, or
the production app's separately approved automatic content-summary controls.

The macOS adapter reads the live foreground process and native CGWindowID. It
requires Accessibility for richer capture, binds the focused AX control to that
same window, checks secure-field and web-area ancestry before reading content,
and rechecks focus and the unlocked console session afterward. Unknown structure
is withheld. Normal AppKit fields explicitly lacking an optional subrole remain
eligible, as do Chromium fields explicitly reporting no optional subrole value;
timeouts and invalid text-field subroles do not. This depends on the
application reporting its accessibility roles correctly.

Control labels are limited to 512 characters, identifiers to 256 and values to
2048. Per-field truncation flags identify partial captures, including a one-character
look-ahead in the native reader. Only changes produce records. Individual keystrokes, window titles, images
and audio are outside these capture modes. Known browsers without a verified
page and private-mode source are withheld from rich capture.

## Browser context

Enable browser capture separately, then use Allow browser access for the selected
running Chrome/Chromium instance. The app asks macOS for Automation consent on a
worker thread. Passive capture never triggers that permission prompt. The signed
app includes the AppleEvents usage description and entitlement needed for this
path; current packaged-app acceptance is still separate from the signed probe.

Read-only AppleEvents resolve a scripting window by its stable ID and check its
normal/incognito mode before reading any URL. Private windows are withheld even
in metadata mode when browser context is enabled. The active tab ID, page URL,
native PID, focused native window and unique window geometry bind the result.
The source rechecks that binding before commit. Only the normalized domain enters
the history and memory envelopes; URL paths, credentials, queries and fragments
from the page context stay out of them. Captured text values can contain personal
content and require their own review before being shared with a model.

## Reviewed memory reuse

The recording review UI accepts either a taiku-observer folder or one native
history segment. Native review pins a complete byte prefix and its hashes, so
new appends do not expand consent. Changed prefixes, deleted files and incomplete
records are refused at import. A separate values option changes the review hash
and requires fresh consent. Protected observations are omitted and values are
locally redacted.

Native event IDs prevent repeated imports of growing files from duplicating old
observations. Conflicting IDs and concurrent overlaps are refused. UTC history
timestamps remain identified as UTC. Old records at an unmarked storage cap are
shown as potentially partial.

Reviewed values can inform untrusted app facts through the existing classifier,
summarizer and memory selector. Canonical domains have their own grounding type.
An explicit recall request can preserve useful observed content without turning
it into a preference or proof that an action succeeded. Deleting the resulting
memory removes it from later retrieval. This is a reviewed memory path, not
executable replay or automatic sharing of all captured text.

Focused controls must belong to the matching native window and web document.
The reader uses web-area document URLs, never a link's destination URL. Different
documents in frames, browser toolbars and ambiguous same-bounds windows are
currently withheld. Supporting those cases remains a functional gap. The native
reader activates cold Chromium accessibility through its application AXRole read,
then makes a bounded structural query when needed. It does not enable VoiceOver
or set AXEnhancedUserInterface.

## MCP surface

```sh
cargo build --manifest-path impl/computer-history/Cargo.toml --release --locked
computer-history mcp --root "$HOME/.icu/app-memory/computer-history"
```

The app's default history root is the path above. If `ICU_APP_MEMORY_ROOT` is set,
append `computer-history` to that absolute memory root instead. The MCP control
server shares the store with the desktop collector; it does not start a collector.

The five tool names, input schemas and annotations match the captured reference:

- `computer_history_pause`
- `computer_history_resume`
- `computer_history_status`
- `computer_history_get_settings`
- `computer_history_update_settings`

Enable capture explicitly in the desktop app. Resuming does not enable a disabled
store. Pause changes are durable and apply across processes. Settings replacement
requires a preceding settings read in that MCP connection; a concurrent change
causes a stale-settings error rather than overwriting unseen rules.

## Observation rules

App and website scopes must both allow capture. Explicit block rules win within
each scope. Domains are canonicalized with IDNA and match the named domain and its
subdomains, not arbitrary suffixes. URL rules reject schemes, credentials, paths,
queries and ports. A known browser without a verified URL is allowed only when
the default website behavior permits observation and no website block rule needs
to be checked. Unknown or invalid HTTP(S) URLs do not satisfy domain allow rules.

These precedence and matching choices are implementation behavior awaiting a
successful native reference comparison. Matching the tool schema does not prove
that every policy edge case matches the reference.

Collectors call `Store::observation` before any native read, including identity
discovery, then `bind` before optional content. The resulting ticket is bound to
policy generation, revision, start time and source context. `commit` rechecks it
under the same interprocess
lock used by pause and settings updates. An observation begun before a pause,
clear or rule change cannot append afterward using the old ticket. A changed
window or website context also invalidates it. `commit_with` runs a host-owned
memory sink under that same barrier, after the activity file write succeeds.
Legacy memory sampling uses `with_ticket` and the same history-to-memory lock
order. A pause/resume during identity discovery cannot admit the older sample.

The collector lease distinguishes enabled configuration from a live recorder.
Status reports the collector mode, heartbeat and errors. The app reuses native
z-order and includes its own front window, rather than using the target picker's
list, which excludes the app itself. Existing Skysight opt-in/pause migrates once;
reopening the app does not re-enable a store that was disabled afterward.

## Storage and boundaries

Files are local JSONL segments, rotated at 16 MiB and pruned by whole UTC days.
Retention may keep the boundary day's records until the next day. An event has a
random id, capture timestamp, source identity, optional domain and collector
payload. URL paths, credentials, queries and fragments are not stored by this
metadata envelope. Collector payloads must honor their own approved content
scope. Metadata mode supplies an empty payload; approved richer modes supply the
bounded focused-control fields above. There is no unfiltered recording file that
is scrubbed afterward.

On Unix, files are owner-only and directories are `0700`. File reads refuse
symlinks. Windows uses the profile's inherited ACL and refuses reparse points;
that path still needs fresh native Windows acceptance. Settings replacement is
atomic, including `MoveFileExW` on Windows. Capture stops near disk exhaustion
while settings and pause remain available. Deletion removes only this store's
owned activity filenames and invalidates outstanding tickets.

The app and MCP client are trusted control-plane callers. The five model tools do
not expose enable, append, capture-ticket or delete operations. Use the host's MCP
approval policy to control changes to observation rules. This is not an OS
security boundary against arbitrary processes with the same user identity.

## Validation

```sh
cargo test --manifest-path impl/computer-history/Cargo.toml --locked
cargo clippy --manifest-path impl/computer-history/Cargo.toml --all-targets --locked -- -D warnings
```

The [round-twelve audit](../../study/audit/parity-round12-2026-10-09/README.md)
records reviewed history imports and real-model recall from the prior native
Chrome capture. Round eleven retains the native browser acceptance.
Other browser/platform adapters, complete frame/window coverage, current signed
desktop distribution, executable replay, automatic content summaries and authenticated reference
comparison remain open.

## Linux focused capture

The production host uses the same collector lease and store for Linux AT-SPI
observations. Application rules run before optional control labels or text are
read. The adapter binds D-Bus ownership, process lifetime, active window, focused
control and its ancestors, then verifies the observation again before publication.
It excludes protected controls, credential labels, web content and terminal text.

A known desktop lock service must be available. Unknown lock state refuses
capture. Lock changes invalidate pending observations, and pause or shutdown
drops the native connection. Supported lock providers currently cover XFCE,
GNOME and Plasma. Other desktops need an authenticated lock-state integration.
The adapter does not start Taiku Observer or a second History host.

Labels-only capture reads explicit label relations for editable controls, never
the editable value as a substitute label. Control and window identities are
opaque and cannot be reused as computer-use action targets. Interaction events,
terminal commands and browser-origin capture remain separate unfinished work.
