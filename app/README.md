# Copilot

A desktop app for observing and controlling individual windows through the patched
cua-driver. Chat, window tiles, and direct tile controls use the same backend.

## Local setup

Use the pinned `vendor-cua` checkout described in the repository README. Install
Rust, the native platform build prerequisites, and Node.js, then run from `app/`:

```sh
npm ci
bash scripts/stage-driver.sh
npm run tauri dev
```

The app uses the driver bundled beside its executable, then a local patched
install. Missing or failed owned startup returns an error without switching to
the vendor daemon. `ICU_CUA_BIN` selects a binary; `ICU_CUA_SOCKET` connects to an existing
endpoint without starting a new daemon. `ICU_OWN_DRIVER=0` selects the vendor
installation. On macOS, use the access controls to grant the running app's
Accessibility and Screen Recording permissions. Missing grants are checked
silently before observations and actions. With Accessibility alone, the agent
can still read the accessibility tree without taking a screenshot.

A confirmed screen-capture consent denial pauses automatic captures for that
endpoint until an explicit access retry passes a fresh permission check. The
access banner reports this pause; ordinary status polling and driver restarts
do not clear it. An external driver is checked through its own passive status
API, and its retry control never requests Copilot's host grants. macOS can still
show its initial or renewed direct-capture consent after Screen Recording
preflight succeeds; the preflight does not prove that this extra consent exists.

For a macOS release, follow [AUTO_UPDATE.md](AUTO_UPDATE.md). The build hook
rebuilds and stages the native driver. It does not publish a release.

## Visual tasks and audio

Use screenshots grants the selected model access to the chosen window's images
for that task. Coordinate actions bind to the capture that was actually shared.
Images are still captured locally for readiness checks when model sharing is off.
On macOS and Linux, task observations use the daemon connection without a CLI
process. Each capture still uses a private temporary PNG, preserving the previous
target-window-only scope instead of capturing additional inline overlay surfaces.
The image, capture identity and geometry come from the same driver response, and
the temporary file is removed after its bytes are read.
Claude Code's CLI adapter remains text-only; the direct provider adapters carry
images when the selected model supports them.

The Audio panel records microphone input, system sound, or both after explicit
source selection and consent. Recordings stay in the app's audio folder, and Stop
or app exit ends the owned recorder. Audio is not automatically added to memory or
sent to a model. Dictation remains unimplemented.

Audio needs the source-specific observer from [impl/observer](../impl/observer/README.md).
The recorder executable goes beside the app executable. macOS also needs its
system-audio helper. `prepare-e2e-macos.py` accepts `--observer` and `--audio-helper`
with pinned hashes and refuses incomplete audio bundles. This is currently an
explicit packaging step. Platform results and remaining compatibility gaps are
listed in the [round-four audit](../study/audit/parity-round4-2026-10-09/README.md).

## Verification

History segments can be opened with Review for memory. Labels and domains are
shared only after review; observed values require an explicit opt-in and renewed
consent. Growing files do not add unreviewed observations, and repeated native
event IDs are skipped. The [round-twelve audit](../study/audit/parity-round12-2026-10-09/README.md)
records real-model recall, untrusted app scoping and deletion. This does not
execute recorded actions or enable automatic sharing of ambient text.

The History panel starts with app identity capture. On macOS, you can separately
approve focused control labels or labels with text values. A separate browser
opt-in enables normal Chrome/Chromium HTTP(S) pages after macOS Automation consent;
live browser acceptance currently covers Chrome. Secure fields, private windows,
unverified pages and locked sessions are withheld. Approved domains reach the
memory stream without full URLs, while observed text still stays out of model
summaries. Other browsers and rich Windows/Linux capture remain open. The
[round-eleven audit](../study/audit/parity-round11-2026-10-09/README.md) separates native
acceptance from app-service and UI checks and lists frame/window exclusions.

These commands do not drive the desktop:

```sh
npm test
npm run build
python3 -m pytest tests/test_gui_smoke.py -q
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --skip agent::e2e
```

The GUI smoke uses headless Chromium with a simulated driver. A separate Tauri
MockRuntime test exercises the actual command registry without a native window.
Neither proves permission prompts, foreground ownership, or native input delivery.
Desktop capture tests are explicitly ignored; live agent tests require a separate
authorized fixture run.

## Model selection

Local mode uses Ollama. API modes use configured provider keys.

Claude Code mode runs the installed `claude` CLI once per decision, headless
(`claude -p --output-format json`), with every tool disabled, no user or project
settings, and this app's system prompt in place of Claude Code's. It uses
whatever login Claude Code has, and this app reads none of its credentials.
`ICU_CLAUDE_BIN` names the CLI when it is not on `PATH` or in the usual install
locations, and `ICU_BRAIN_MODEL` names the model (default `claude-sonnet-5`). An
`ANTHROPIC_API_KEY` in the app's environment is not passed to the CLI; choose
Anthropic mode to use a key. A missing CLI, a Claude Code that is not logged in,
and a rate or usage limit each report as such. Each decision starts a new
process, so it takes several seconds (about 9 to 10 seconds measured with
`claude-sonnet-5`). The earlier `claude-oauth` adapter, which sent Claude Code's
login token while identifying itself as Claude Code, has been removed. Selecting
it reports that and names Claude Code mode.

The Codex login adapter borrows the Codex login's unexpired token without
refreshing it or changing its credential store; an expired login must be renewed
in Codex. It reads `CODEX_HOME/auth.json` when explicitly configured, otherwise
`.codex/auth.json` under `HOME` or Windows `USERPROFILE`. Explicit model selections report errors without silently changing
to a local model. Auto mode uses an Anthropic key if one is set, else Claude
Code when the CLI is installed, with the local model as its fallback. Each step
it takes on the fallback is logged with the preferred provider's error.

`ICU_OPENAI_BASE_URL` points the OpenAI mode at any `/chat/completions` endpoint;
a local one such as Ollama (`http://127.0.0.1:11434/v1`) needs no key.
`ICU_CODEX_BASE_URL` points the Codex mode at a local Codex-protocol router that
owns its own accounts, such as ccodex (`http://127.0.0.1:8792/backend-api/codex`);
the app sends it no token. `ICU_OPENAI_MODEL` names the model for both.

Read [GAPS.md](GAPS.md) for verified behavior and remaining acceptance work.

## Recordings and task continuation

Open Memory, then Learn from a recording. Review the selected app's labels, choose
the activity type and summary provider, and approve the import. Every eligible
event within the import limits is retained in ordered chunks. Imported observations
remain untrusted, with links to their recording and summary sources. Deleting a
source removes the memories derived from it.

Saved tasks retain redacted progress from interrupted or budget-limited runs.
Choose the target window and a decision budget when continuing. The next segment
observes the current state and obtains fresh targets and approvals.

Allow foreground is unchecked by default and applies to one submitted task.
Loading saved progress does not restore this permission. On macOS it permits
bounded keyboard and drag delivery to the chosen window; background semantic
actions remain the default. See the [current acceptance audit](../study/audit/paired-round3-2026-10-09/README.md)
for platform evidence and remaining limits.

## Learn from a recording

Open **Memory > Learn from a recording**, enter a taiku-observer recording folder,
and choose **Review recording**. The folder must contain its original
`manifest.json` and `events.jsonl`. Select one app, a project label, the recorded
actor (person, agent, or test fixture), a task description, and the model provider.
The preview shows the exact redacted event labels that will be imported, grouped
by app, with source lines and an omitted count when the recording exceeds the
40-step limit. Confirm permission to share this reviewed slice with the selected
provider. Changing the app, actor, project, task description or provider clears
consent so the revised choice must be approved again. Choose **Import into memory**. Task memories must be enabled.
An import made while paused stays queued until generation resumes.

The preview pins both files by SHA-256. If either changes, review again. Imports
retain capture time, clock mapping, source lines, actor, project, consent and
omitted-event counts. They accept semantic focus, click and value-change labels;
field values, raw keys, pixels, URLs, audio, excluded apps and secure controls are
omitted. File size and event count are bounded. These exclusions do not guarantee
that app labels contain no private information, so review the recording's scope
and the chosen provider before importing.

The memory pass summarizes an imported demonstration as **observed**, with zero
model decisions. It cannot treat the demonstration as verified success. Derived
notes are unverified context for later tasks on the same app, never executable
replay. Source deletion removes its derived summaries and durable notes. Capture
time remains in provenance; raw retention starts at import time. Existing stores
without recording fields remain readable.

On macOS, task memory resolves the selected window's exact running process to
its bundle ID, including accessory apps omitted by the launcher's app list.
The acceptance probe checks the actual note in the model's task prompt and the
source entry cited by its journal. A memory marker in a system prompt alone is
not evidence of retrieval.

The stock observer uses a 40 ms button-state poll and can miss short clicks. The
[pinned click-capture integration](../impl/observer/README.md) uses a left-button-only
event tap, with bounded hit testing and explicit gaps. It captured four of four
12 ms clicks in repeated isolated tests. Importing a recording does not repair
missing events, and Windows observer capture has not been retested in this round. See the [paired acceptance audit](../study/audit/paired-memory-2026-10-08/README.md)
for the real-model import, retrieval, reuse, deletion and comparison evidence.

The app refuses synthetic key actions on macOS, including shortcuts and single
keys. A process-targeted key can reset shared HID idle state even when foreground
focus and the pointer stay unchanged. Use element value writes or visible controls
with accessibility actions. Linux and Windows keep their existing key routes;
an unknown target platform is refused before input. This app restriction does
not change the native driver's keyboard API.

## Packaged-app real-model acceptance

`tests/run_macos_real_model_suite.py` runs calculator, text replacement,
two-window isolation and refusal-recovery tasks through the packaged GUI and a
real-model relay. Run it only on a disposable Mac in the console user's GUI
context. It verifies the app, driver and probe hashes against a build manifest,
then requires actual task success, clean focus/pointer observations, model
responses and process cleanup. A second manually submitted goal is reported as
an eventual recovery and does not pass the first-attempt gate.

```sh
ICU_DISPOSABLE_HOST=1 ICU_TEST_MODEL_URL=http://127.0.0.1:11500/RUN_TOKEN \
  python3 app/tests/run_macos_real_model_suite.py \
  --bundle /path/to/Copilot.app --manifest /path/to/build-manifest.json \
  --fixture /path/to/minicalc_mac --ui /path/to/macos_ui \
  --model-label ACTUAL_RELAY_MODEL_VERSION --repetitions 3 --work /tmp/acceptance-fresh
```

The relay is the existing `claude_code_bridge` test harness; the account stays on
the model host. `--check-only` validates artifacts without GUI input or model
calls and reports `prepared_not_executed`, never acceptance success.

Use a new work directory for live measurements. `--repetitions` accepts 1 to 10
independent trials per task, with a default of 1. The model label records the
operator's relay configuration; it is not independent proof of model identity.
Response model identifiers, when supplied by the relay, remain in each trial.
The build manifest also pins the suite, `task_quality.py` and shared Mac probe helper, so regenerate it
after changing the benchmark. No benchmark command provisions a host or account.

`suite.json` schema 2 retains every attempted trial's task, repetition, process
exit code, setup-inclusive wall time, relative `result_path`, `result_sha256`,
existing acceptance verdict and `quality` assessment. Failed goals remain in
the results. A missing cleanup proof stops later trials because their baseline
would no longer be independent. Trial directories retain the probe's raw model
responses, fixture state/events and ownership samples. With more than one
repetition they are `trial-001/TASK`, `trial-002/TASK`, and so on.
Each trial creates an empty `app-memory` directory and explicitly passes its
absolute path as `ICU_APP_MEMORY_ROOT` through LaunchServices. This overrides
inherited settings and the app's default `~/.icu/app-memory`. User retries within
one trial keep that root; different tasks and repetitions cannot reuse it. The
probe retains `memory_isolation` policy, root and empty-baseline evidence, and
grading rejects missing isolation evidence or reused roots. Memory retrieval and
learning may run within a trial, but no prior trial's task memory is available.

The nested `benchmark` schema 1 is the task-quality report contract:

| Field | Meaning |
| --- | --- |
| `planned_trials`, `recorded_trials` | Required task/repetition slots and attempts retained, including aborted attempts |
| `metrics.first_attempt_success`, `eventual_success` | Independent fixture outcome for the first submitted goal and final state; retries never become first-attempt success |
| `metrics.wrong_target_effects` | Changes to the protected Alpha document, private control, sibling calculator buttons, or foreground guard, including changes later restored |
| `metrics.noninterference` | At least 20 ownership samples with no observed focus, pointer, HID, overlay or guard-input disturbance |
| `metrics.refusal_case_exercised` | Refusal-case trials with a retained controller refusal, rather than model prose mentioning a refusal |
| `metrics.first_goal_wall_seconds` | Measured count, unknown count, median, nearest-rank p95 and maximum from goal submission through foreground handoff and the post-task model idle wait |

Boolean metrics expose `true`, `false`, `unknown`, `planned` and
`fraction_true_of_known`. Missing trials or evidence remain unknown; they are
never counted as clean outcomes. Trial `quality.refusals.count_lower_bound`
deduplicates numbered refusals across cumulative controller histories. The last
refusal may never reach another model request, so this is not a total action
refusal rate. Calculator success requires its native button trace as well as
the answer. Summary success flags cannot override fixture observations.

`benchmark.passed` requires every planned trial to pass both the existing
model/build/cleanup checks and task-quality assessment. `--check-only` has no
measurements and cannot pass this gate. The suite covers these named AppKit
fixture tasks only; `parity_established` is always false. Sampled ownership can
miss disturbances between samples. The offline tests validate scorer behavior
and runner bookkeeping, not controller quality or native execution:

```sh
python3 -m pytest app/tests/test_task_quality.py app/tests/test_task_quality_runner.py \
  app/tests/test_real_model_verdict.py app/scripts/test_prepare_e2e_macos.py -q
```

`scripts/prepare-e2e-macos.py` prepares an offline, ad-hoc-signed candidate from
the current app source, an already-verified driver and a vendor checkout matching
the tracked patch. It leaves the installed app alone. The candidate disables
updater artifact creation and points update checks to an inert loopback endpoint;
it is for behavior tests, not notarization or distribution acceptance.

The suite needs both outbound access to provision/reach its isolated desktop and
model host, and local TCP/Unix sockets inside that environment. A sandbox that
refuses socket creation cannot run the full chain. Keep such failures in the
results rather than turning them into passing or skipped acceptance cases.


The [expanded native comparison](../study/audit/paired-round2-2026-10-08/README.md)
adds checkboxes, radio and popup selection, Unicode/multiline input and scrolling.
The macOS driver retains actionable scroll areas in its element tree and uses
native scrollbar controls when available for background page/line scrolling.

Linux Wayland capture includes an explicit Qt5 accessibility compatibility
launcher for affected Qt5 applications. See [Qt5 compatibility](src-tauri/qt5-compat/README.md)
for usage and its native build dependencies.
