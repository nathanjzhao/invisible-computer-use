# Licensing and redistribution posture

This is a description of where the code and content in this repository come
from, so the redistribution stance is explicit. It is documentation, not legal
advice. This repository is a private parity/research tree; several parts are not
ours to redistribute and must be removed or redrawn before any public
distribution, as noted below.

There are three buckets: OpenAI/Codex proprietary content, the vendored
cua-driver (MIT), and our own original work.

## 1. OpenAI / Codex proprietary content (not ours to redistribute)

This material is OpenAI's. It is present in this private tree for parity study
and verification. It carries no license to us and must not be shipped.

Tracked in git (verbatim OpenAI bytes, here for study, remove or redraw before
distribution):

- `reference/` (the reverse-engineering corpus): decompiled Sky JavaScript
  (`reference/openai-sky/sky_js_src/`), decompilation logs, plists, API docs, and
  the pack checksum list `reference/openai-sky/app-bundle-packs/SHA256SUMS.txt`.
  This is OpenAI's Codex/Sky, obtained by decompilation.
- The seven per-app instruction packs, `impl/app-instructions/packs/*.md`
  (Slack, Spotify, iPhone Mirroring, Numbers, AppleMusic, Clock, Notion). These
  are byte-identical to OpenAI's shipped Sky bundle packs; each file's sha256
  matches the corpus list above (guarded by
  `impl/app-instructions/tests/verbatim.rs`). The delivery mechanism around them
  is ours (see bucket 3); the pack text is OpenAI's.
- The confirmations policy text, `impl/confirmations/policy/*.md`
  (`confirmations-policy.md`, `confirmations-policy-macos.md`,
  `windows-hard-deny.md`). These are the verbatim Codex/Sky confirmation and
  hard-deny strings. The enforcement mechanism is ours; the policy text is
  OpenAI's.

Gitignored (never committed; loaded or extracted locally at build time):

- OpenAI compiled binaries: `reference/openai-sky/bin/SkyComputerUseService`,
  `reference/openai-windows/codex-computer-use*.exe`,
  `win/helpers/codex-computer-use-swift.exe`.
- The SkyLens cursor art: the real Codex LensSequence frames and any extracted
  cursor renditions, under `assets/codex/` in the cursor-overlay crate. They are
  loaded at build time from the installed Codex app on the build machine; their
  provenance and sha256 are recorded in the committed
  `assets/codex-assets-manifest.json`, but the art itself is never committed.
- The A/B cursor renders that contain that art, `study/cursor-graphics/*.png`.

Before distribution: the tracked verbatim OpenAI text (the seven packs and the
confirmations policy text) and the `reference/` corpus must be removed or
rewritten to non-verbatim equivalents. The gitignored binaries and art are
already kept out of the tree and out of any distributable build; a build that
embeds the Codex cursor art must instead ship original art.

## 2. cua-driver (vendored, MIT)

The driver we patch is trycua's `cua-driver`. Its license is MIT
(`vendor-cua/LICENSE.md`, Copyright (c) 2025 Cua AI, Inc.). MIT permits use,
modification, and redistribution provided the copyright notice and permission
notice are retained.

- The full upstream checkout, `vendor-cua/`, is gitignored in this repository
  (it is its own git repo, ~533 MB). This repository therefore does not itself
  redistribute cua-driver source.
- Our changes to it live in `impl/native/cua-driver.patch`, a tracked `git diff`
  of the vendored tree. The patched driver is a derivative work of an MIT-licensed
  project.
- A distributor who ships the patched driver (source or binary) must include
  cua-driver's MIT copyright and permission notice, per the MIT terms.

## 3. Our original work (owned)

Everything under `impl/` that is our own code, plus `specs/`, `study/` (our
analysis and notes), and the documentation, is our original work:

- the `icu` layer and its crates: `impl/icu`, `impl/axdiff`, `impl/skysight`,
  and the mechanisms in `impl/app-instructions` (the first-touch delivery and
  session tracking, distinct from the pack text) and `impl/confirmations` (the
  policy enforcement and hard-deny dispatch, distinct from the policy text);
- `impl/native/cua-driver.patch` (our modifications to cua-driver, a derivative
  work under MIT, bucket 2), including the cursor-overlay integration code
  (SkyLens rendering and theme selection) minus the raw Codex art;
- the specs, build plan, audit notes, drive harnesses, and install/sign scripts.

We can license our own work as we choose. We cannot relicense buckets 1 or 2:
bucket 1 is OpenAI's and unlicensed to us, and bucket 2 stays under cua-driver's
MIT terms.

## Summary

| Content | Where | License to us | Ship as-is? |
| --- | --- | --- | --- |
| Codex reference corpus | `reference/` (tracked) | none (OpenAI's) | no; remove |
| 7 instruction packs (text) | `impl/app-instructions/packs/` (tracked) | none (OpenAI's) | no; redraw/reword |
| Confirmations policy (text) | `impl/confirmations/policy/` (tracked) | none (OpenAI's) | no; redraw/reword |
| Codex binaries, SkyLens art | gitignored | none (OpenAI's) | no; already excluded |
| cua-driver + our patch | `vendor-cua/` (ignored) + `impl/native/cua-driver.patch` | MIT | yes, with MIT notice |
| Our layer, specs, harnesses | `impl/`, `specs/`, `study/` | ours | yes |

For the code-signing and OS-trust requirements of a distributable build, see
impl/DISTRIBUTION.md.
