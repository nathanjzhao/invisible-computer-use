# Auto-update

The app checks for updates on launch and shows an "Install and restart" toast when
one is available. Built on `tauri-plugin-updater` with signed bundles.

## What's wired

- `src-tauri/src/lib.rs` registers the updater + process plugins.
- `src-tauri/tauri.conf.json` -> `plugins.updater`:
  - `endpoints`: `https://github.com/nathanjzhao/invisible-computer-use/releases/latest/download/latest.json`
  - `pubkey`: the minisign public key that every update must be signed with.
  - `bundle.createUpdaterArtifacts: true` so `tauri build` emits the signed update
    archives and `.sig` files.
- `src/main.ts` -> `checkForUpdate()` runs on startup; failures (offline, no release
  yet) are swallowed so it never nags.

## The signing key

Generated once with `tauri signer generate`:

- Private key: `~/.tauri/copilot-updater.key` (no password, mode 0600; it was 0644 until 2026-10-04). KEEP SECRET, never commit.
  Whether to keep this key or rotate it before the first release is still open (GAPS.md, Needs Nathan).
- Public key: baked into `tauri.conf.json`. If you rotate the key, shipped clients
  can no longer verify new updates, so treat rotation as a breaking release.

For CI, export the private key instead of relying on the file:

```
TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/copilot-updater.key)
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""   # this key has no password
```

## What a client checks, and what it does not

The manifest (`latest.json`) is plain JSON anyone with write access to the endpoint can change.
The client treats it as untrusted:

- The archive must verify against the public key in `tauri.conf.json` (minisign). A wrong key, a
  changed byte and a body cut short all fail before anything is installed.
- `plugins.updater.requireSignedVersion` is on. The signature's own comment names the version it
  was made for (`tauri build` writes it with `tauri-cli` 2.12 or later, so `package.json` asks for
  `^2.12.1`), and the client refuses an archive whose signed version is not the version the manifest
  announces. Without this, an old release that the key really signed could be offered under a
  higher version number. A signature with no version in it is refused, so a release must be built
  with 2.12 or later (`tauri signer sign` takes `--app-version <v>` when signing by hand).
- A release no newer than the running app is never offered. The check failing for any reason
  (offline, 404, a manifest that does not parse) shows nothing.
- Tests: `src-tauri/src/updater_tests.rs` (a local manifest server and a throwaway key, the shipped
  config, an install into a fake `Copilot.app`) and `src/updater.test.ts`.

## Cutting a release (what makes updates actually flow)

1. Bump `version` in `tauri.conf.json`.
2. On macOS, build the signed app with the signing env vars set. The build hook
   rebuilds, signs, and stages the patched driver before packaging:
   ```
   TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/copilot-updater.key) \
   TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
   npm run tauri build -- --bundles app
   ```
   This produces the macOS `.app.tar.gz` and `.sig`. Build Windows and Linux
   installers on their native hosts with their native driver sidecars and signing
   setup. Windows updates were measured on 2026-10-04 (below); on Linux the updater logic was run
   against a fixture AppImage and `.deb`, not a bundle from `tauri build` (see "Linux updates").
   Cut every release with `@tauri-apps/cli` 2.12.1 or later (`npm ci`), or clients refuse it.
3. Write `latest.json` (the manifest the endpoint serves):
   ```json
   {
     "version": "0.2.0",
     "notes": "what changed",
     "pub_date": "2026-09-20T00:00:00Z",
     "platforms": {
       "darwin-aarch64": { "signature": "<contents of the .app.tar.gz.sig>", "url": "https://github.com/.../Copilot_0.2.0_aarch64.app.tar.gz" },
       "darwin-x86_64":  { "signature": "...", "url": "..." },
       "windows-x86_64": { "signature": "...", "url": "..." },
       "linux-x86_64":   { "signature": "...", "url": "..." }
     }
   }
   ```
4. Publish a GitHub release whose assets include each updater archive and `latest.json`.
   The `releases/latest/download/latest.json` endpoint then resolves to it and clients
   update on next launch.

The configured endpoint (a GitHub repository under nathanjzhao) returned 404 on 2026-10-04: that
repository does not exist, so nothing can be updated yet. Publishing needs a public releases repository or an
S3/R2 location, and `endpoints` changed to match.

Endpoint is swappable: point `endpoints` at S3/R2/any static host instead of GitHub if
you prefer. Tauri also supports `{{target}}`, `{{arch}}`, `{{current_version}}`
templating in the endpoint URL for a dynamic server.

## Note on code signing (separate from update signing)

The minisign key above only proves an update came from you. It does NOT satisfy OS
gatekeepers. For distribution outside your own machine you still need:
- macOS: Developer ID signing + notarization of the `.app` (or users get Gatekeeper
  warnings, and the TCC grants the driver relies on won't persist across updates).
- Windows: an Authenticode cert for the installer and the UIAccess helper.
Both are wired into the build now (below) and both were proven with the real credentials: the
Windows build was signed with the real Azure Artifact Signing certificate on 2026-10-03 and 2026-10-04.

macOS is wired: `ICU_NOTARY_ISSUER=<issuer id> app/scripts/sign-notarize-macos.sh` builds
Copilot.app signed with the Developer ID Application certificate (G7P6939437, expires
2031-09-17), notarizes it with an App Store Connect team key, staples the ticket and runs
`spctl`. The first run on 2026-09-29 (submission `e88cf267-2c8e-4a37-b907-bc40b079a9c3`) came
back Accepted, and `spctl` reported "Notarized Developer ID" for the bundle and for a copy
carrying a quarantine flag. The build with memory (291f542, submission
`ad3f166a-2e63-4122-8a9f-5d48279e00a5`) was accepted the same way. The script header lists its inputs.

Windows is wired too, with the real certificate. See the next section.

## Windows code signing

`app/scripts/sign-windows.ps1` signs every PE the Windows build ships with SHA-256 and an
RFC 3161 timestamp: `app.exe`, the `cua-driver.exe` sidecar, the UIAccess worker
`cua-driver-uia.exe`, the NSIS plugin DLLs, the NSIS uninstaller, the NSIS installer and the
MSI. Its `build` mode runs `tauri build` with itself as `bundle.windows.signCommand`, so Tauri
calls it once per file. It then checks every installer and sidecar with `signtool verify /pa
/v`, and the files inside the MSI as well. It also checks the NSIS installer by name
(`verify-installer`): 7-Zip lists what is inside it, and every `.exe` and `.dll` there, the
NSIS plugin DLLs included, must be signed; `sign` keeps a ledger, and the build fails unless the
ledger holds a verified sign of each sidecar, the installer and the NSIS uninstaller (makensis
goes on when a sign command it runs fails, which is how an unsigned `uninstall.exe` shipped
once). With `ICU_WIN_VERIFY_INSTALL=1` it also installs the installer for the current user and
checks `uninstall.exe` and the installed files, then uninstalls; use that on CI or a disposable
build host. Building needs 7-Zip (`winget install 7zip.7zip`). The header lists every input;
secrets are environment variables only.

The UIAccess worker is bundled on Windows only (`src-tauri/tauri.windows.conf.json` lists it as
a second sidecar, and `impl/native/apply-and-build.sh` builds it). Windows starts it with
UIAccess only when it is signed by a certificate that chains to a trusted root and sits under
Program Files, so the NSIS installer now offers a per-machine install (`installMode: both`;
silent: `/S /AllUsers`). A per-user install still works, but there the worker runs without UIAccess, and the app says so
(a note in the chat and banner when it is installed outside Program Files; see
[DISTRIBUTION.md](../impl/DISTRIBUTION.md#per-user-installs-have-no-uiaccess-worker)). For all users,
run the installer as an administrator, or silently `/S /AllUsers`.

Proven on EC2 Windows Server 2022 first with a throwaway CA
([windows-signing](../study/audit/windows-signing-2026-10-01/README.md)) and then with the real
certificate ([windows-real-sign-2](../study/audit/windows-real-sign-2-2026-10-03/README.md),
[gaps-windows-2](../study/audit/gaps-windows-2-2026-10-04/README.md)): every shipped PE
verifies, no throwaway CA is trusted on the host, and the installed signed worker starts with `TokenUIAccess` 1. Unsigned, or signed
under a root that is no longer trusted, Windows refuses to start it; signed but outside
Program Files, it starts with `TokenUIAccess` 0.

### Provider: Azure Artifact Signing

The chosen provider is Azure Artifact Signing (formerly Trusted Signing), Basic tier, $9.99 a
month for 5,000 signatures, open to individual developers in the US and Canada. Its
certificates last 72 hours and are reissued daily, which is fine: every signature carries a
timestamp, and the certificates have no lifetime-signing EKU, so signatures should stay valid after
the certificate expires (not yet tested: the first certificate expires 2026-10-06). They chain to Microsoft Identity Verification Root CA 2020, which
Windows trusts through its automatic root update. SmartScreen treats a new Artifact Signing
publisher like any new OV publisher: the publisher name shows, and reputation builds with
installs. No certificate type skips that any more, EV included. This is the expectation, not an
observation: Windows Server shows no SmartScreen dialog, so it needs a Windows 10 or 11 client.

The account exists (`nzhaosigning`, East US, Public Trust profile `taiku-release-signing`, a service
principal with the signer role). For a new account, hand back four things:

1. **Endpoint**: the region of the Artifact Signing account, as its URL, for example
   `https://eus.codesigning.azure.net/` (East US) or `https://wus2.codesigning.azure.net/`
   (West US 2). It must match the account's region, or signing fails with 403.
2. **Account name**: the Artifact Signing account's name.
3. **Certificate profile name**: the Public Trust profile created after identity validation.
4. **How it authenticates**: either a service principal (tenant id, client id and a client
   secret, kept out of the repo) with the "Artifact Signing Certificate Profile Signer" role
   on the account, which is what CI needs; or an `az login` on the build machine by an
   account with that role.

Then, on a Windows build machine with the MSVC tools, Node, Rust, Git Bash, the Windows SDK
signing tools and the .NET 8 runtime:

```
$env:ICU_WIN_SIGN_PROVIDER = 'artifact'
$env:ICU_ARTIFACT_ENDPOINT = 'https://eus.codesigning.azure.net/'
$env:ICU_ARTIFACT_ACCOUNT  = '<account name>'
$env:ICU_ARTIFACT_PROFILE  = '<certificate profile name>'
$env:AZURE_TENANT_ID = '...'; $env:AZURE_CLIENT_ID = '...'; $env:AZURE_CLIENT_SECRET = '...'
powershell -ExecutionPolicy Bypass -File app\scripts\sign-windows.ps1 build
```

Add `TAURI_SIGNING_PRIVATE_KEY` (and its empty password) to get the signed updater artifacts
as well; without it the script turns updater artifacts off. The script downloads the pinned
Artifact Signing client (`Microsoft.ArtifactSigning.Client` 1.0.128 from NuGet, SHA-256
checked) the first time; `ICU_ARTIFACT_DLIB` points it at an installed copy instead.

Two other providers use the same script: `pfx` (a certificate file; what the end-to-end test
used) and `store` (a certificate in the Windows store, for a hardware token or a cloud HSM
whose client exposes it there, such as SSL.com eSigner, Certum SimplySign or DigiCert
KeyLocker).

`app/tests/windows/uiaccess.ps1 -Exe 'C:\Program Files\Copilot\cua-driver-uia.exe' -Expect
uiaccess`, run as the logged-on user after a per-machine install, is the check that matters:
it starts the worker the way Windows requires and reads `TokenUIAccess` from its token.

### Windows updates (measured 2026-10-04 on Windows Server 2022)

The updater plugin runs the NSIS installer with `/P /R /UPDATE` and exits the app at once.
`installMode: both` installs per user unless told otherwise, so an update of an all-users
install used to land in `%LOCALAPPDATA%` and leave the old copy in Program Files. The app now
passes `/AllUsers` when it runs from Program Files and `/CurrentUser` otherwise (NSIS installs
only; an MSI install takes no NSIS switches), so an update keeps the scope of the install it
replaces and the UIAccess worker keeps starting. Against a throwaway minisign key and a local
manifest server: a wrong key, a tampered archive and a manifest whose version does not match
the signed one are refused with the app still running, a lower version is never offered, an
update offered during a task leaves the task running, and the install button during a task
says so and does nothing. Limits: a standard user with no way to elevate gets a per-user
install next to the old all-users one (the installer falls back), and the app has already
quit by then, so it comes back as the per-user copy without the UIAccess worker. Details and
evidence: [gaps-windows-2](../study/audit/gaps-windows-2-2026-10-04/README.md).

### Linux updates (measured 2026-10-04 and 2026-10-05 on Ubuntu 24.04)

The plugin picks its install route from a marker `tauri build` writes into the binary. An AppImage is
replaced in place with no privileges (manifest key `linux-<arch>-appimage` or `linux-<arch>`); a `.deb`
cannot be replaced in place: the plugin runs `dpkg -i` through `pkexec`, else zenity or kdialog with `sudo`,
else terminal `sudo` (key `linux-<arch>-deb`), and with no polkit agent in the user's session it fails and
leaves the old version. With `createUpdaterArtifacts: true` and tauri-cli 2.12.1, `tauri build` on Linux writes
`Copilot_<v>_amd64.AppImage` and `.AppImage.sig` (the raw AppImage is the update file, no tar.gz),
`Copilot_<v>_amd64.deb` and `.deb.sig`, and the `.rpm` with its `.sig`. A `latest.json` entry's `signature` is the
contents of the matching `.sig`, its `url` the file.

Measured on a real X session with packages built by `tauri build`, a local manifest server and a throwaway key:
a click on the update toast replaced the installed AppImage in place and relaunched it as the new version; the
`.deb` ran `pkexec dpkg -i`, showed the polkit dialog, finished once it was answered (synthetic keystrokes) and
also under a polkit rule, and a refused dialog left the old version installed and running. Building on Linux
needs Node 22.12 or later and the vendor checkout at its clean HEAD. Not done: a person answering the prompt,
`.rpm`, Wayland, and publishing (the endpoint still 404s). Evidence:
[gaps-linux-2](../study/audit/gaps-linux-2-2026-10-04/updater.md),
[gaps-linux-3](../study/audit/gaps-linux-3-2026-10-05/updater.md).

## Verified end-to-end (2026-09-20)

The full flow was driven against a real https endpoint (an S3 presigned URL serving a
signed v0.2.0 artifact): a v0.1.0 build detected the update, downloaded it, verified the
signature, unpacked, and installed; the on-disk `CFBundleShortVersionString` went
0.1.0 -> 0.2.0. Two gotchas surfaced and are baked into the guidance above:

- **Build the updater artifact with `--bundles app`.** With `bundle.targets` = `all`,
  `tauri build` tries the `dmg` target too, and `bundle_dmg.sh` can fail (hdiutil / stale
  temp dmg), which aborts the build before the updater `.tar.gz`/`.sig` are written. Use
  `tauri build --bundles app` (or fix the dmg env) to emit the updater artifact.
- **Update tarballs must not contain macOS AppleDouble (`._*`) files.** Tauri's own build
  produces a clean tar; if you ever repackage a `.app` by hand, use
  `COPYFILE_DISABLE=1 tar czf ...`; otherwise the client fails with
  `failed to unpack '._<App>.app'`.
