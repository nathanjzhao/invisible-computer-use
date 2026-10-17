# Readiness and verification acceptance

This batch covers the observation and action dependencies of H1-H4 and M1-M2.
The [active queue](../study/audit/QUEUE.md#active-core-queue-2026-10-15) governs
scope: G08 and core input, interruption and lifecycle checks remain active;
historical audio, Messages, Contacts, standalone JavaScript and new release or
signing work remain deferred. Existing signed candidates can be used for package
checks. Windows reference sign-in gates the final reference comparison, not
candidate implementation or acceptance.

The [retained Mac evidence](../study/audit/rea-readiness-2026-10-15/verification.json)
proves three AppKit cases through the app loop with a scripted model and a fixed
native driver. It does not prove Windows, Linux, the changed driver, or an installed
production package. A pass below is scoped to its named lane; no core matrix cell
is promoted by these checks alone.

## Required checks

| Change | Deterministic source checks | Native and package acceptance |
| --- | --- | --- |
| Readiness | Quiet interval, late observations, cancellation, target identity, private baseline, valid canvas and cold first read | Delayed result, perpetual change, unchanged UI, and a separate constant-tree busy case. Native busy metadata must be observed, never added by the harness. Repeat on AppKit, GTK3/X11 and WinForms. GNOME/KDE Wayland remains a separate lane. |
| Screenshot retention | Real PNG changes with unchanged AX, partial/unknown AX, first/session/window grounding, explicit flags, no empty output, screenshot IDs and scale | Native image/canvas changes while AX stays constant; image returned on partial coverage; stale pixel binding refused after resize. Package app must use its bundled driver. |
| Mixed verification | Disjoint file/UI success, shared streak, one budget, late success, unknowns, order, post-action default, contract-shaped predicates | Through `icu`, create/remove an owned file while a native control changes. Prove disjoint states never satisfy, then prove coexistence for two samples. Driver-only GTK3/WinUI3 verification is a regression check, not this mixed proof. |
| Typed outcomes | Preserve structured code, effect, route and refusal through transport, dispatch and event; no retry after uncertain delivery | Native delivery failure, unknown/partial delivery and refusal must match the receiver journal. No second mutation after an ambiguous transport failure. Preserve error detail in the app event and next model turn. |
| Direct observation | Exact PID/window/session, inline bytes, typed capture refusal, no second request after remote failure, final capture IDs | Read the installed sidecar through the actual app route. Bind image dimensions and digest to the same result and daemon. Record fork/request counts and latency. Windows currently falls back to CLI when the Unix direct transport is unavailable; do not inherit its speed claim. |
| Native click and menu | Source-bound click/menu admission tests for the patched driver | AppKit target and same-process decoy: ordinary click, stale target, disabled/menu action, open/closed menu, unsupported route and refusal; exact receiver effects and foreground ownership. Rebuild and bind the changed driver before claiming these fixes. |

For every native lane retain source revision, patch hash, app/test/driver/fixture
hashes, driver PID and endpoint identity, raw calls/events, receiver state, timed
ownership samples and cleanup. Keep failed attempts. A timeout, unavailable
provider, missing busy signal, skipped test or setup failure is not a pass.

## Deterministic commands

Run from the checkout root with Rust 1.97.1. These commands do not opt into native
GUI tests. Dependencies and the patched vendor source must already be present.

```sh
export RUSTC="$(rustup which --toolchain 1.97.1 rustc)"
rustup run 1.97.1 cargo test --locked --manifest-path impl/Cargo.toml -p ui-readiness
rustup run 1.97.1 cargo test --locked --manifest-path impl/Cargo.toml -p icu --test verification
rustup run 1.97.1 cargo test --locked --manifest-path impl/Cargo.toml -p icu --lib screenshot_tests
rustup run 1.97.1 cargo test --locked --manifest-path impl/Cargo.toml -p icu --lib ui_readiness_tests
rustup run 1.97.1 cargo test --locked --manifest-path app/src-tauri/Cargo.toml --lib agent::
rustup run 1.97.1 cargo test --locked --manifest-path app/src-tauri/Cargo.toml --lib transport::
rustup run 1.97.1 cargo test --locked --manifest-path app/src-tauri/Cargo.toml --lib cua::
```

`impl/verify.sh --native` hardcodes the macOS platform unit target. Use the
appropriate platform crate directly on Windows/Linux rather than treating that
script as a portable native gate. The app tests compile the new portable module
when `agent.rs` declares it:

```rust
#[cfg(test)]
#[path = "agent/readiness_portable_tests.rs"]
mod readiness_portable_tests;
```

## Portable native app loop

These ignored tests perform native input only on an explicitly supplied disposable
Linux or Windows desktop. They refuse Nathan's home paths, require an explicit
socket/pipe, verify its kernel owner PID and pinned executable, preserve all
fixture evidence and reap only their fixture process. They never provision a
host, activate a window, start a daemon or synthesize busy metadata. Run one test
process at a time. Supply the compiled app test executable as `REA_TESTS`; obtain
it with `cargo test --locked --manifest-path app/src-tauri/Cargo.toml --lib --no-run`.

The first three cases use real changing/unchanged control values. The fourth is
separate: `portable_static_busy_requires_native_signal` fails with
`CAPABILITY_MISSING` if the real driver does not export a signal recognized by
the controller. Passing the changing-state cases does not waive that failure.
Current GTK/UIA busy projection needs native confirmation and may need code.
The adapter records the actual production observation callback; it adds no
post-action capture or settling sleep. Run a source-bound red baseline that
bypasses the readiness wait: the delayed and perpetual-change cases must fail
before the correct controller is accepted, while the unchanged control remains
green. If a slow first native read lets the delayed baseline pass, that trial is
not discriminatory; retain it and adjust the owned fixture timing before rerun.

### Linux GTK3 on X11

Use an owned X11 session with AT-SPI, `/usr/bin/python3`, PyGObject GTK3/GdkX11,
`xprop`, and an unrelated foreground guard window. Both `DISPLAY` and
`DBUS_SESSION_BUS_ADDRESS` must name that session; `XDG_SESSION_TYPE` must already
be `x11`. This adapter does not claim Wayland coverage.

```sh
: "${REA_DRIVER:?absolute candidate driver}" "${REA_TESTS:?absolute app test executable}"
export ICU_DISPOSABLE_HOST=1 ICU_E2E_PORTABLE_READINESS=1
export ICU_E2E_PORTABLE_WORKDIR="$(mktemp -d /tmp/icu-readiness.XXXXXX)"
chmod 700 "$ICU_E2E_PORTABLE_WORKDIR"
export ICU_CUA_BIN="$REA_DRIVER"
export ICU_CUA_SOCKET="$ICU_E2E_PORTABLE_WORKDIR/driver.sock"
"$REA_DRIVER" serve --no-overlay --socket "$ICU_CUA_SOCKET" >"$ICU_E2E_PORTABLE_WORKDIR/driver.log" 2>&1 &
export ICU_E2E_DRIVER_PID=$!
trap 'kill "$ICU_E2E_DRIVER_PID" 2>/dev/null || true; wait "$ICU_E2E_DRIVER_PID" 2>/dev/null || true' EXIT
"$REA_DRIVER" --socket "$ICU_CUA_SOCKET" call health_report '{}'
test "$("$REA_TESTS" --list | awk '/^agent::readiness_portable_tests::portable_.*: test$/ {n++} END {print n+0}')" -eq 4 || exit 1
"$REA_TESTS" agent::readiness_portable_tests:: --ignored --nocapture --test-threads=1
```

Wait for the explicit daemon to listen before the read-only health request. The
receiver's raw foreground sampling is X11 evidence, not a physical keyboard,
pointer or all-transient-focus proof.

### Windows WinForms

Use the standard user's owned console session, with an unrelated foreground
guard window. Service and RDP sessions are excluded. The test compiles its embedded
receiver with the installed .NET Framework C# compiler. Set `$ReaDriver` and
`$ReaTests` to absolute candidate paths before running:

```powershell
$env:ICU_DISPOSABLE_HOST='1'; $env:ICU_E2E_PORTABLE_READINESS='1'
$env:ICU_E2E_PORTABLE_WORKDIR=Join-Path $env:LOCALAPPDATA ('ICUAudit\rea-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $env:ICU_E2E_PORTABLE_WORKDIR | Out-Null
$env:ICU_CUA_BIN=$ReaDriver
$env:ICU_CUA_SOCKET='\\.\pipe\icu-readiness-'+(Split-Path $env:ICU_E2E_PORTABLE_WORKDIR -Leaf)
$ReaDaemon=Start-Process $ReaDriver -ArgumentList 'serve','--no-overlay','--socket',$env:ICU_CUA_SOCKET -PassThru
$env:ICU_E2E_DRIVER_PID=[string]$ReaDaemon.Id
try {
    & $ReaDriver --socket $env:ICU_CUA_SOCKET call health_report '{}'
    if ($LASTEXITCODE) { throw 'Owned daemon is not ready' }
    $ReaCases=@(& $ReaTests --list | Select-String '^agent::readiness_portable_tests::portable_.*: test$')
    if ($ReaCases.Count -ne 4) { throw 'Portable fixture module is missing or incomplete' }
    & $ReaTests 'agent::readiness_portable_tests::' --ignored --nocapture --test-threads=1
    if ($LASTEXITCODE) { throw 'Retain result.json and logs from the failed cases' }
} finally {
    if (-not $ReaDaemon.HasExited) { $ReaDaemon.Kill(); $ReaDaemon.WaitForExit() }
}
```

Wait for the explicit pipe to listen before the read-only health request. Preserve
the resulting private evidence directory. No installer, signature or UIAccess
claim follows from this source test executable.

## Existing native and package lanes

Commands below run only on owned disposable hosts with the listed artifacts.
`CUA_TEST_DRIVER_BIN` selects the candidate sidecar, so testkit must not silently
use an older workspace binary. Stage harnesses in a copied vendor checkout,
never Nathan's shared vendor symlink.

```sh
export CUA_TEST_DRIVER_BIN="$REA_DRIVER"
rustup run 1.97.1 cargo test --locked --manifest-path vendor-cua/libs/cua-driver/rust/Cargo.toml -p cua-driver --test harness_gtk3_test harness_gtk3_verify_state -- --exact --ignored --nocapture --test-threads=1
rustup run 1.97.1 cargo test --locked --manifest-path vendor-cua/libs/cua-driver/rust/Cargo.toml -p cua-driver --test harness_gtk3_test harness_gtk3_invoke_menu_live_path -- --exact --ignored --nocapture --test-threads=1
```

GTK3 staging: `bash vendor-cua/libs/cua-driver/tests/fixtures/build/linux.sh --only gtk3`.
For Windows, in PowerShell:

```powershell
$env:RUSTC=(& rustup which --toolchain 1.97.1 rustc)
& ./vendor-cua/libs/cua-driver/tests/fixtures/build/windows.ps1 -Targets @('wpf','winui3')
$env:CUA_TEST_DRIVER_BIN=$ReaDriver
rustup run 1.97.1 cargo test --locked --manifest-path vendor-cua/libs/cua-driver/rust/Cargo.toml -p cua-driver --test harness_winui3_test harness_winui3_verify_state -- --exact --ignored --nocapture --test-threads=1
powershell -File app/tests/windows/acceptance.ps1 -ManifestPath $ReaManifest -Mode compute
powershell -File app/tests/windows/acceptance.ps1 -ManifestPath $ReaManifest -Mode toolerror
powershell -File app/tests/windows/acceptance.ps1 -ManifestPath $ReaManifest -StopPending
```

The Windows package harness installs the pinned candidate and verifies the app and
owned sidecar hashes; its current calculator cases need an additional readiness
fixture for installed-package busy and delayed-result acceptance. Its native
`native.ps1` uses one CLI subprocess per request, so it cannot prove the new
direct observation transport. Native outcome codes must also be checked against
receiver effects, not only process exit status.

Linux extracted-package startup and screenshot binding are already executable:

```sh
dbus-run-session -- python3 app/tests/probe_linux_package.py "$REA_WORK/package" "$REA_APP"
dbus-run-session -- python3 app/tests/probe_linux_screenshot_binding.py "$REA_WORK/binding" "$REA_DRIVER" app/tests/fixtures/x11_drag_receiver.py
```

The package startup probe creates its own Xvfb display and verifies the running
sidecar path. `probe_linux_capture_refusals.py` and
`probe_linux_popup_surfaces.py` additionally require a private network namespace
and `ICU_PROBE_HOST_NETNS` naming the original namespace; run each with `none`
and `picom` as its compositor argument. The native Wayland driver probe accepts
`mutter`, `sway`, `gnome-shell` and `kwin`, but these are separate compositor lanes,
not substitutes for GTK3/X11 or for package UI acceptance. Keep unsupported
capture/input routes typed and effect-free rather than coercing them to success.

For Mac, use the existing private daemon and private canonical `$REA_WORK` on
the owned console desktop, rebound to the changed driver:

```sh
ICU_DISPOSABLE_HOST=1 ICU_E2E_NATIVE_READINESS=1 \
ICU_E2E_NATIVE_READINESS_WORKDIR="$REA_WORK" ICU_CUA_SOCKET="$REA_WORK/driver.sock" \
ICU_CUA_BIN="$REA_DRIVER" "$REA_TESTS" agent::readiness_native_tests:: --ignored --nocapture --test-threads=1
```

After the benchmark extension merges, its actual hosted-model command is:

```sh
ICU_DISPOSABLE_HOST=1 ICU_TEST_MODEL_URL="$REA_RELAY" \
python3 app/tests/run_macos_real_model_suite.py --bundle "$REA_BUNDLE" \
  --manifest "$REA_MANIFEST" --fixture "$REA_MINICALC" --ui "$REA_UI" \
  --model-label "$REA_MODEL_VERSION" --repetitions 3 --work "$REA_FRESH_LIVE_WORK"
```

Use `--check-only` with a separate fresh preparation directory first. The manifest
must bind the suite, task-quality reducer and Mac probe source hashes. That suite
retains first-attempt and eventual outcomes separately; scripted models and
native-only checks must not be counted as hosted-model trials.

## Remaining engineering work

- Run the new adapters on owned Windows and Linux hosts, then fix actual native
  failures. GTK/UIA constant-tree busy capability is not established by role names
  or the Mac spinner proof.
- Add a native mixed file/UI fixture through `icu`; the existing synthetic mixed
  tests and native driver-only predicate tests do not compose into live proof.
- Add native unchanged-AX pixel-change cases to the platform fixtures and assert
  the actual facade output, rather than only the driver capture.
- Bind the new typed-outcome and direct-observation changes to installed package
  tasks. Renew package/hash evidence after a native click/menu driver change.
- Preserve H1 privacy, ordering and one-recorder controls; H2/H3 review/deletion;
  H4 replay/Stop/decoy behavior; and M1/M2 citation, recall, pause/deletion and G08
  runtime checks when these shared dependencies change. The broader queue's
  external comparison gates remain separate from these executable obligations.
