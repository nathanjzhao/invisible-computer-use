## Long-horizon memory: Skysight vs cua computer-history

### What it is

Both systems give a future agent context about earlier work, but they are almost opposite designs. Codex Skysight is a memory extension: a background process records a rolling local event stream of everything the user does, an LLM condenses it into chronological 10-minute and 6-hour summaries, and a phase2 step consolidates those into durable memory. cua "computer history" is a narrow, opt-in, encrypted ledger that records only the metadata of actions the driver itself performed, never ambient user activity, and never summarizes anything in the shipped preview.

### Codex/Sky

Skysight is not in the shipped `sky_js` CUA service at all (grep for `skysight` across `.../\@oai/sky/dist` returns nothing). It lives in the Codex memory extension and is defined entirely by two prompt files in the corpus. CONFIRMED from the prompts:

- `codex-corpus/mac/ComputerUse/SkysightSummarizer.md` is the summarizer system prompt. Key mechanisms: it "records the user's activity through an event stream" (the companion `SkysightMemoryInstructions.md` line 3 describes the source as a "rolling local event stream process that runs in the background") feeding "event records, app/window/AX text, browser content, terminal output, documents, chat messages, local artifact paths, and child summaries." It emits one markdown summary per window with YAML frontmatter (`title`, `description`, `applications` bundle-id array, optional `suggestion` for a skill or automation) and a body of exactly three headings: `## Memory summary`, `## Recording summary`, `## Citations`, plus `### Relevant prior context` and `### Important non-obvious context about the user`. It runs at two cadences: `10min` captures immediate task state; `6h` compresses child 10min summaries into larger arcs. The whole prompt is a security boundary: observed content is "highly untrusted," taint is "sticky," and output "must be descriptive, not directive" (no instructions, policies, or tool requests addressed to future agents; observed commands become "the user ran x" not "run x").
- `codex-corpus/mac/ComputerUse/SkysightMemoryInstructions.md` defines the phase2 consolidation seam. Mechanism: "When generating phase2 memories, use relevant summaries from the resources folder... Grep over the folder." It reads the summaries as evidence (ignoring their frontmatter), promotes only sufficiently-supported durable patterns into the `User Profile`, writes task arcs into `MEMORY.md`, and tags anything derived with `[skysight memory]`. Storage layout: `resources/*.md` named `YYYY-MM-DDTHH-MM-SS-{4_alpha}-10min-{slug}.md` or `-6h-{slug}.md`, under `$HOME/.codex/memories/extensions/skysight/resources/`.

So Skysight is a three-tier pipeline: event stream to 10min to 6h to phase2 memory. It is content-rich (files, symbols, commands, collaborators, decisions), human-readable, and defended against prompt injection at the prompt layer.

### cua-driver

cua ships a real, code-backed subsystem, but it is a metadata ledger, not a memory summarizer. CONFIRMED:

- `rust/crates/cua-driver-core/src/history.rs` is the whole engine. `HistoryManager::begin_action` / `finish_action` hook the canonical tool-dispatch chokepoint beside recording's begin/finish; they enqueue `action_started` / `action_completed` events via non-blocking `try_send` onto a bounded `sync_channel` (`WRITER_QUEUE_CAPACITY = 512`) drained by one `cua-history-writer` thread (`writer_loop`). Events are the `HistoryEvent` CloudEvents envelope with a strict `HistoryPayload` enum (`Control`, `ActionStarted`, `ActionCompleted { effect, route, delivery, evidence_kinds, escalation_kind }`, `Session`, `Access`, `Health`) and `#[serde(deny_unknown_fields)]`. `finish_action` derives its fixed fields only from `ActionExecutionRecord::stable_projection()`; free-form detail is discarded. Read side: `query()` bounds to `MAX_QUERY_LIMIT = 200`, filters by sequence/session, and appends an encrypted `access` audit. `HistoryStatusTool` / `HistoryQueryTool` expose `history_status` / `history_query`; their tool descriptions already carry the consultation policy ("For prompts to continue or revisit recent work, call this first, before broad desktop discovery"). Encryption is inline: `chacha20poly1305`, `coset` COSE_Encrypt0, `hkdf` per-chunk keys, `Zeroizing` key material; session ids are HMAC-SHA256 keyed, not stored raw. `DEFAULT_RETENTION_DAYS = 7`, `DEFAULT_QUOTA_BYTES = 100 MiB`, `CHUNK_ROTATION_INTERVAL = 55min`.
- `rust/crates/platform-macos/src/history.rs` is the thin native adapter. `MacosKeychainKeyProvider` stores the 256-bit namespace root key in the macOS Data Protection Keychain (`AccessibleWhenUnlockedThisDeviceOnly`, non-synchronizable, access group under `com.trycua`), with a login-Keychain fallback only for the `cua-driver-local` namespace. `MacosApplicationIdentityProvider::resolve(pid, window_id)` maps a PID to a `bundle_id` + `display_name` via `apps::list_running_apps()` and nothing else.
- `docs/computer-history-architecture.md`, `docs/computer-history-agent-integration-rfc.md`, `docs/computer-history-preview.md` are the spec. They fix the privacy allowlist (no screenshots, keystrokes, text, clipboard, AX trees, paths, titles, URLs, or free-form diagnostics), the two-gate consent model (`serve --experimental-history` admission plus `history enable` opt-in), and mark the interesting parts as future stages: a rule-based / model summarizer is the "Model summarization stage" (deferred), ambient focus history and window-title/URL capture are "later" in the platform-adapter table.

There is no summarization, no time-windowed rollup, no ambient capture, and no consolidation in cua today. `writer_loop`'s `checkpoint_retention` and `prune_expired_chunks` are retention housekeeping (seal at ~55min, prune expired sealed chunks), not semantic consolidation.

### Delta

| Dimension | Skysight (Codex) | cua computer-history |
|---|---|---|
| What is recorded | Ambient: everything the user does (apps, AX text, terminal, browser, docs, chat) | Only driver-mediated actions; zero ambient activity |
| Content richness | Full semantic content: files, symbols, commands, people, decisions, blockers | Fixed enums + bundle id/name only; no free text ever |
| Summarization | LLM 10min then 6h then phase2 consolidation | None in preview; rule-based/model summary deferred |
| Storage | Plaintext markdown under `~/.codex/memories/...` | Encrypted CBOR/COSE chunks, ChaCha20-Poly1305, native-keychain key |
| Read access | Injected into agent context via memory files | Permission-gated `history_query`, capped at 200, audited |
| Injection defense | Prompt-layer: untrusted, sticky taint, descriptive-not-directive | Structural: no free text exists to inject; encryption at rest |
| Consultation policy | Memory is consulted/consolidated by the memory pipeline | Tool descriptions instruct "call this first"; RFC defines it |

- Matches: both are local and private, both hand a fresh agent prior-run context, and both encode a "check history before broad discovery" policy.
- cua is ahead on: encryption at rest with native key custody, a hard structural privacy allowlist, permission-gated and audited reads, action-isolation (`try_send`, fail-open), and a formal cross-platform schema/CDDL.
- Codex is ahead on: actually capturing what the user was doing, temporal consolidation into durable memory, skill/automation suggestions, ambient (not just driver-mediated) coverage, human-readable summaries, and a mature prompt-injection framework for untrusted observed content.

### Recreate/port plan

Getting Skysight-like long-horizon memory onto cua is a large build, because cua deliberately omits the two things Skysight is made of: ambient content capture and LLM summarization. Concrete steps:

1. Copy-verbatim: bring `SkysightSummarizer.md` and `SkysightMemoryInstructions.md` into the cua agent as the summarizer and phase2 consolidation prompts, plus the descriptive-not-directive security framing. These are portable prompt contracts and are the cheapest, highest-value lift.
2. Large: build an ambient event-stream capture pipeline. cua has no ambient observer; `platform-macos/src/history.rs` only resolves a PID on demand. This is a new capture module (AX/window/screen observers) that cua's own docs list as deferred "ambient focus history" and "window-title/URL opt-in." It must not reuse `history.rs`, whose allowlist forbids exactly this content, so it is a sibling subsystem, not a patch.
3. Large / native-patch: add a summarizer + model broker. cua's "Model summarization stage" is explicitly deferred and the vault design forbids the vault process from holding model connections, so this needs the new model-broker component the RFC sketches, plus a scheduler that fires the 10min and 6h cadences and a phase2 consolidation job writing markdown memory files.
4. Thin-port: reuse cua's existing consultation policy. The `history_status` / `history_query` tool descriptions in `history.rs` already say "call this first, before broad desktop discovery," so aligning the agent to consult the new memory before broad discovery is a small change, not new plumbing.
5. Native-patch (optional bridge): if the goal is only "prior-run context" rather than full ambient memory, keep cua's encrypted `history.rs` ledger and add a rule-based summarizer over its metadata events. That is a modest extension of `HistoryManager::query`, but it yields action-metadata timelines, not Skysight's semantic user-activity memory.
