# Distribution and code-signing

This layer drives a patched `cua-driver`. On both macOS and Windows the driver
needs a stable, trusted code identity before it can be shipped to someone else's
machine. `impl/install.sh` handles the part that a local build can do by itself;
the rest needs signing credentials that only a distributor has, so it is written
down here rather than faked.

Status: this layer is NOT distributable as-is. It becomes distributable when a
distributor supplies the trust anchors below (a macOS Developer ID plus
notarization, and a Windows Authenticode identity for the UIAccess helper).
Until then it is a local/dev build: it runs for the person who built it, on the
machine that built it.

## macOS

### What install.sh does (verified)

`impl/install.sh`, on macOS, signs the freshly built `cua-driver` through
`impl/native/macos-sign.sh` before it is used:

- If a `Developer ID Application` identity exists in the login keychain (or one
  is named in `ICU_SIGN_IDENTITY`), the driver is signed with it. The resulting
  designated requirement is certificate-backed, so it is identical across
  rebuilds (`identifier "com.trycua.driver" and anchor apple generic and
  certificate leaf...`). macOS keys Accessibility and Screen Recording grants to
  that requirement, so the grants survive a rebuild.
- With `ICU_REQUIRE_STABLE_SIGNING=1` and no usable certificate identity, the
  install refuses rather than silently install an unstable binary.
- Otherwise it falls back to an ad-hoc signature and warns loudly: an ad-hoc
  signature's designated requirement is the binary's cdhash, so every rebuild is
  a new identity and the TCC grants are lost, and macOS 26+ can SIGKILL an
  unsigned rebuild on launch.

Knobs: `ICU_SIGN_IDENTITY` (a 40-hex SHA-1 or a full identity name),
`ICU_REQUIRE_STABLE_SIGNING=1`, `ICU_MACOS_IDENTIFIER` (default
`com.trycua.driver`). Guard: `impl/native/tests/macos-sign-policy.test.sh`.

Stable local signing is enough for the grants to persist on the build machine.
It is NOT enough to hand the binary to anyone else. For that:

### What a distributor must do

1. Sign with a `Developer ID Application` certificate issued by Apple (an
   individual/organization membership in the Apple Developer Program). Use the
   Hardened Runtime and the driver's entitlements, which the vendored tree
   already ships:
   - `vendor-cua/libs/cua-driver/rust/scripts/CuaDriver.entitlements`
     (`com.apple.security.device.screen-capture`,
     `com.apple.security.automation.apple-events`);
   - `codesign --force --options runtime --entitlements <that file> --sign
     "Developer ID Application: <name> (<TEAMID>)" <binary-or-app>`.
2. Ship the driver as the app bundle, not the bare CLI. The vendored tree stages
   `com.trycua.driver` (`CuaDriverBundle/Contents/Info.plist`, `LSUIElement`
   headless). TCC prompting and attribution are reliable for a bundle with a
   fixed bundle id and Info.plist; a bare CLI tool is not a good TCC subject to
   put in front of an end user. The upstream local installer
   (`libs/cua-driver/scripts/install-local.sh`) already builds and signs that
   bundle from the source tree; point it at the patched tree to carry these
   patches into a signed bundle.
3. Notarize and staple. Zip or wrap the signed bundle and submit it:
   - `xcrun notarytool submit <zip> --apple-id <id> --team-id <TEAMID>
     --password <app-specific-password> --wait`;
   - `xcrun stapler staple <app>`.

What breaks without each step:

- Unsigned or ad-hoc only: Gatekeeper blocks a downloaded copy; macOS 26+ may
  SIGKILL it; TCC grants do not persist across rebuilds.
- Signed with Developer ID but not notarized: first launch of a downloaded copy
  is blocked by Gatekeeper until the user clears quarantine by hand.
- Not shipped as a bundle: the Accessibility and Screen Recording prompts are
  attributed to the launching terminal, not to the product, and do not persist
  cleanly for an end user.

## Windows

The daemon runs at normal integrity. Sending background keystrokes and calling
`SetForegroundWindow` against AppContainer/UWP apps (Calculator, modern Notepad,
Settings) requires a helper process running at UIAccess integrity: the
`cua-driver-uia` worker. Its manifest
(`vendor-cua/libs/cua-driver/rust/crates/cua-driver-uia/cua-driver-uia.manifest`)
already declares `uiAccess="true"`. Windows grants that integrity at launch only
when the Application Information service is satisfied on all of:

1. the executable carries a valid Authenticode signature;
2. it is launched via `ShellExecute` (the daemon already does this; plain
   `CreateProcess` of a UIAccess binary fails); and
3. it lives under a "secure" path (`\Program Files\`), OR an administrator has
   set `HKLM\...\Policies\System\EnableSecureUIAPaths=0` so it may launch from
   `%LOCALAPPDATA%\Programs\trycua\cua-driver-rs\bin\`.

### What a distributor must do

- Dev / VM: create a self-signed code-signing certificate, sign
  `cua-driver-uia.exe`, and add the certificate to `LocalMachine\Root` and
  `LocalMachine\TrustedPublisher`; set `EnableSecureUIAPaths=0` if installing
  outside `\Program Files\`. `signtool sign /fd sha256 /a cua-driver-uia.exe`.
- Production: sign with an Authenticode certificate (an EV certificate is the
  norm for a UIAccess binary) and install under `\Program Files\`, so no
  registry policy relaxation is needed on the user's machine.

### Per-user installs have no UIAccess worker

The NSIS installer is `installMode: both`, so a standard user (no administrator) gets a per-user
install under `%LOCALAPPDATA%\Programs\Copilot`. That is not a `\Program Files\` path, so
Windows never gives `cua-driver-uia.exe` UIAccess integrity there, signed or not, and the
failure above applies: keystrokes to Calculator, Settings and other Store apps are dropped. The
app says so: on Windows it checks where it is installed and, when that is outside Program Files,
shows a note in the chat and the permission banner ("Copilot is installed for this user only
..."). To get the worker, install for all users: run the installer as an administrator and pick
all users, or `/S /AllUsers` silently. `installMode` stays `both` on purpose: `perMachine` alone
would refuse to install for a user without administrator rights, which is worse for a user who
only needs the plain driver. The note is by path, so it does not see an
`EnableSecureUIAPaths=0` policy an administrator set; there the warning is a false alarm.

What breaks without it: the worker never reaches UIAccess integrity, so the daemon cannot
deliver foreground keys or text to windows of higher integrity than itself, and refuses them with a
UIPI message that says why (before 2026-10-04 it answered `ok` and typed nothing). With the worker
(all-users install, signed), the daemon starts it on demand, authenticates it by pid, and has it
deliver those inputs; clicks, scroll and element-addressed typing into such windows still refuse.
The foreground-lock case for Store apps (Calculator, Settings) is vendor documentation that this
project has not reproduced: `LockSetForegroundWindow` did not stop the driver on Server 2022, and
that image has no Calculator. See [uiaccess](../study/audit/gaps-linux-windows-2026-10-04/uiaccess.md).

## Not legal advice

This file is about code identity and OS trust requirements only. For the
licensing and redistribution posture of the bundled content, see LICENSING.md.
