# Semantic recorder integration

Linux audio uses an additional `taiku-observer-linux-audio.patch`, applied after
the unchanged click and shared audio patches. Its source/dependency hashes are
in `linux-audio-manifest.json`. It connects through libpulse to PulseAudio or
PipeWire's Pulse server, selects the default sink's exact monitor for system
sound and a non-monitor default source for microphone sound. Streams cannot
move to replacement devices. Stop/owning-stdin EOF bounds all server waits;
device errors preserve and finalize available samples with an explicit event.
ALSA-only desktops without a Pulse server report unavailable.

The integration inputs are not exactly the release tag: its Cargo files and
macOS helper pins come from later Git objects. `export_pinned.py` reconstructs
and verifies these exact inputs without changing the taiku checkout:

```sh
python3 impl/observer/export_pinned.py --repository /path/to/taiku --out /fresh/export
python3 impl/observer/prepare.py --source /fresh/export/deploy --out /fresh/prepared
```

The Linux patch changes no Mac/Windows platform implementation or shared Opus
encoder. Linux build/staging commands and native evidence are in the
[W9 report](../../study/audit/linux-audio-2026-10-12/REPORT.md).

The audio extension adds a bounded `audio` command with explicit `--source system`,
`--source microphone`, or `--source both`. It starts only those audio devices and
no process, screen, input, device or UI sensors. `--secs` accepts
1..3600 seconds and defaults to 60. The alternative `--max-duration-ms` accepts
100..3600000 integer milliseconds. The two duration flags are mutually exclusive.
Capture time starts when the first source delivers samples, with a separate
ten-second startup bound. Writing `stop` to stdin, closing the owning stdin,
Ctrl+C or reaching the duration ends capture and finalizes the recording. The
app's Audio panel owns this lifecycle and requires fresh consent for each start.

Explicit sessions also cap persisted PCM samples before encoding. Codec padding
is trimmed by the final Ogg granule, so buffered device shutdown cannot extend
the decoded recording beyond its requested duration. The sample-limit patch
covers the shared writer and the separate macOS system-audio helper.

The original `record --audio true` retains its documented both-source behavior.
Audio files are Ogg Opus. Mac system audio needs `taiku-observer-audio-helper`
beside the recorder, signed and bundled with the app. `prepare.py` now validates
the audio patch and dependency/source hashes plus the helper source manifest.
Pass a source directory containing all three observer projects. Preparation
reproduced successfully against the pinned round-four inputs.

[Round-four evidence](../../study/audit/parity-round4-2026-10-09/README.md) includes
three Windows source combinations, Windows GUI consent/save/cleanup and Mac
system-audio capture. Generated tones were verified in the decoded files.
Mac microphone, Linux audio and reference WAV/tool compatibility remain pending.

`prepare.py` applies the patch to a fresh copy of observer 1.0.32. Every changed
base file and result is pinned in `manifest.json`. The separate taiku checkout is
left untouched. Build results and live acceptance are tracked in the round-three
audit. Historical round-two evidence applies to its earlier patch only.

macOS uses a listen-only pointer tap. Windows uses a passive low-level mouse hook.
Both cover left, right and middle button presses, drag endpoints, and wheel events.
They do not subscribe to keys or persist a raw pointer-movement stream. Focus and
optional value observations retain the existing consent and sampling policy.

Accessibility resolution runs on a worker with a 64-observation queue and a 250 ms
freshness limit, checked before and after native reads. Overflow, late resolution,
unavailable desktops and unresolved targets produce explicit gaps. A pointer event
is an observation of input, not proof that an application accepted it. The target
label is resolved shortly after capture; timestamps record both moments.

The Mac tap is re-enabled if the OS disables it. Installation retries after a
missing permission, and the AX stream waits for accessibility permission without
requesting a grant. Whether a permission change takes effect without restarting
also depends on macOS. Native window ownership replaces the unbounded System Events
subprocess used by the separate foreground-app sensor.

Windows periodically reinstalls its mouse hook every ten seconds because Windows
can remove hooks silently. This bounds a potential silent-removal interval; it is
not detection of every missed event. Hook installation failure is reported and
retried. Queue loss is counted; permission/tap outages can have an unknown loss count.

The development-only `ICU_TEST_TAP_DISABLE_AFTER_MS` fault injection requires the
`dev-commands` feature. It disables the real Mac tap once to test the same recovery
path. Default builds contain no such environment-controlled fault injection.

Imports retain app-scoped click, focus, value-change, drag and scroll labels in
ordered chunks. They exclude recorded values, URLs, coordinates, keys, images and
audio. Capture-gap metadata survives import and is shown during review. Imported
activity remains untrusted evidence and cannot confer task approval.

Missing Mac bundle IDs no longer become the literal `[ NULL ]`. The importer
also handles older recordings with that value by using their recorded app name,
so those recordings do not share a placeholder memory scope.
