# Codex-equivalence conformance suite

Answers: "when we implement, can we test we have the exact same functionality as Codex computer use?" Yes, in three layers. Run: `CONFORMANCE_TARGET=<cua|our|golden:DIR> pytest conformance/`.

## Layer 1 — Format / contract conformance (runnable now)
Pins Codex's actual output contracts as golden fixtures and asserts a target matches them:
- `golden/tool-surface-window2.md` — the window2 tool set + WindowState/AccessibilityState/Screenshot shapes.
- `golden/ax-diff-format.md`: the tree, row grammar, diff, no-change and selection/focus text, rule by rule, from the real service's no-login probe. `golden/sky-replay/*.json` replays each captured observation byte for byte (`test_sky_replay.py`, `impl/axdiff/tests/sky_replay.rs`). A new capture drops in as another JSON file.
- `golden/error-taxonomy.md`: the ServerErrorCode vocabulary, with the observed -10005/-10010 strings and our mapping.
- `golden/confirmations-policy.md` — the 4 confirmation modes.
- `golden/app-instructions/*` — per-app behavioural playbooks.
Current offline verification (`impl/verify.sh --native`) builds/pins workspace executables and records source state plus SHA256s. At `fix/audit-backlog` dc20e6cc (the lead's final run, 2026-10-05; the commits after it changed only docs, study folders and Windows test scripts): `our` 195 passed / 7 skipped / 3 xfailed, `cua` 135 passed / 7 skipped / 63 xfailed. Earlier counts, kept as dated: `our` 108 passed / 7 skipped / 3 xfailed, `cua` 81 passed / 7 skipped / 30 xfailed (2026-09-26, `feat/sky-format`); against `fix/audit-backlog` f40d24c (`our` 103/7/3, `cua` 80/7/26) the difference is `test_sky_replay.py`: one fixture-existence check, plus four replay sessions that pass on `our` and xfail on `cua`, whose raw driver renders no Sky text ([evidence](../study/audit/sky-format-2026-09-26/README.md)). The 46 audit/path guards exercise our actual binaries regardless of target selection. Many other xfails are unconditional adapter selections; they are coverage labels, not observed stock-driver failures. The historical native 15/23 result requires a daemon and has not been rerun. Missing required binaries fail, while live setup remains explicitly skipped. See [FIX-STATUS](../study/audit/FIX-STATUS.md).

## Layer 2 — Ground-truth golden capture (the definitive oracle)

The offline Sky pipe tests can run the locally installed JavaScript client against our fixture
driver without launching the native service or touching the desktop:

```sh
cargo build --locked --manifest-path impl/Cargo.toml -p icu
CONFORMANCE_TARGET=our python3 -m pytest conformance/test_sky_pipe.py -q
CONFORMANCE_TARGET=our \
  CONFORMANCE_SKY_MAC_SOURCE='/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/cua/dist/project/cua/sky_js/src/targets/mac' \
  python3 -m pytest conformance/test_sky_pipe.py -q
```

The default remains the archived client. `CONFORMANCE_SKY_MAC_SOURCE` selects a different
`targets/mac` directory. This checks client/adapter interoperability, not native-service behavior.

Capture REAL Codex CU behaviour on the same fixtures, store as a trace dir, replay via `SkyGoldenAdapter`, and byte-diff our layer's outputs against it. Two capture routes:
- Drive live Codex CU on the fixture and record the sky `get_app_state` trees, diffs, and action results. NOTE (checked 2026-09-09): the `codex` CLI (0.153.4) has NO computer-use — CU lives only in ChatGPT.app (`@oai/sky`). So this route needs the ChatGPT desktop app with an account that has computer-use enabled, driving an actual task against a fixture, recorded behaviourally (fixture event log + screen capture). That is the reliable route.
- Speak the sky IPC directly on `computeruse.sock` (framing decoded: `uint32LE(len)+utf8(json)`, JSON-RPC 2.0, `ping{clientApiVersion}` handshake then `request{clientApiVersion,request,requestType}`, socket at `~/Library/Group Containers/2DC432GLL2.com.openai.sky.CUAService/IPC/computeruse.sock`). **BLOCKED, empirically confirmed 2026-09-09.** With the service NOT running the socket refuses connections (`ECONNREFUSED`); with the service running (spawned via LaunchServices) an external node process CONNECTS but the server **closes the connection without any framed reply to `ping`** (tested clientApiVersion 1-5). This is the `ComputerUseIPCSenderContext` peer-authorization gate: the transport (`native-pipe.js`) also requires the trusted `globalThis.nodeRepl` runtime that only ChatGPT.app injects. So a raw/standalone client is rejected by design; defeating that is out of scope (it is circumventing an IPC security control). The behavioural live-drive route above is the only viable golden-capture path. `adapters.SkyGoldenAdapter(trace_dir)` remains ready to replay whatever a behavioural capture produces.

## Layer 3 — Task-outcome equivalence (OSWorld-style, runnable now)
`tasks/*.json` define tasks (focus field, type, click) with an end-state oracle. Both Codex and our layer run the same task on the same fixture; assert identical end state via the fixture's own event log (ground truth, not the driver's OK) and cua `verify_state`. Equivalence regardless of internal format.

## How this is used during implementation
1. Build a slice of the layer (e.g. AX-diff). 2. `CONFORMANCE_TARGET=our CONFORMANCE_LIVE=1 pytest -k diff` -> must match `golden/ax-diff-format.md`. 3. If a golden trace exists, add a byte-diff test vs `SkyGoldenAdapter`. 4. Run the Layer-3 task on both and compare end state. Green across all three = exact-same-as-Codex for that slice.
