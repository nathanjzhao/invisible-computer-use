param([Parameter(Mandatory=$true)][string]$Vendor,[Parameter(Mandatory=$true)][string]$Delta,[string[]]$Only=@(),[string]$Out='C:\ICUAudit\drivers',[string]$Revision='')
# SYSTEM, after build.ps1, inside its built vendor tree. Builds, with the
# command stage-driver.sh uses (cargo build --locked -p cua-driver --release,
# through apply-and-build.sh's fixed junction and path remapping):
#   green              the tree as built (the candidate patch)
#   red                the tree minus $Delta (the candidate patch minus this
#                      change set, i.e. the previous committed patch)
#   no-minimized-guard green with only the minimized pixel guard disabled
#   no-frame-binding   green with only the screenshot_id lookup disabled
#   no-client-guard    green without the outside-the-client-area refusal
#   any-session        green reading an unbound pixel against any session's
#                      latest capture of the window, not the caller's own
#                      (c9-unify: the rule before it, minus its refusals)
#   no-identity        green without the check that the handle still names
#                      the window a bound screenshot_id captured
#   no-session-identity green without that check for the calling session's
#                      own capture of an unbound pixel
#   no-scroll-binding  green with scroll's pixel point used unbound and
#                      unscaled, as before
#   no-drag-endpoints  green with one id binding both drag endpoints
#   no-id-effect       green with the bare {"code":"invalid_arguments"} reply
#                      for a malformed or misdirected screenshot_id
#   no-wpf-gate        green letting synthetic pen input at a WPF window
#                      report success, as before round 2
#   digitizer-exemption green skipping the WPF pen refusal while
#                      SM_DIGITIZER reports a ready digitizer, as round 2 did
#   no-wpf-child       green posting to WPF content inside a non-WPF root
#   no-region          green ignoring window regions in the input walk
#   no-httransparent   green ignoring HTTRANSPARENT in the input walk
#   skip-ex-transparent green skipping WS_EX_TRANSPARENT children in the
#                      input walk, as the previous hit test did
#   skip-transparent-subtree green passing an HTTRANSPARENT child through
#                      without trying its own children, as round 3 did
#   wfhost-refused     green refusing a click on a WPF root whose point
#                      resolves to a hosted Win32 child (WindowsFormsHost)
#   hosted-any-target  green treating any window a WPF root's point resolves
#                      to as a hosted Win32 child, the root itself included
#   hosted-any-target-no-wpf-child  the same, and without the posted route's
#                      own WPF refusal (no-wpf-child): both guards gone
#   refuse-posted      green refusing every posted pointer message with
#                      point_unmapped (a negative control for the probe's
#                      -Mutate judgement, not a fix)
#   repro-green        green relinked after touching main.rs, to compare with green
#   repro-red          red twice, main.rs touched between, to compare with each other
# Every build that fails throws; nothing is copied from a failed build. The
# tree is restored after each variant. hashes.json maps each build to its
# driver sha256, and records the source revision (-Revision), the patch's
# sha256, the path remapping and the toolchain (rustc, MSVC toolset, Windows
# SDK, Visual Studio) next to them.
$ErrorActionPreference='Stop'
$root="$Vendor\libs\cua-driver\rust"
$src="$root\crates\platform-windows\src"
$main="$root\crates\cua-driver\src\main.rs"
$env:CARGO_HOME='C:\ICUAudit\cargo';$env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH="C:\ICUAudit\cargo\bin;C:\Program Files\Git\cmd;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$vs=& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' -latest -products '*' -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
# As apply-and-build.sh builds: through a fixed junction to the source tree
# (the parent of vendor-cua, which also holds impl\axdiff), because Cargo hashes
# axdiff's absolute location into crate metadata, and with the junction and
# CARGO_HOME remapped, so the driver matches a package built from the same
# source anywhere.
$tree=(Resolve-Path "$Vendor\..").Path.TrimEnd('\')
$junction='C:\icu-driver-src'
# rmdir removes an old junction (even one whose target is gone), not its
# target, and fails on a directory with contents, and then so does mklink.
cmd /c "rmdir $junction 2>nul"
cmd /c mklink /J $junction $tree | Out-Null; if($LASTEXITCODE){throw "could not create $junction"}
$build="$junction\$(Split-Path $Vendor -Leaf)\libs\cua-driver\rust"
$env:CARGO_TARGET_DIR="$build\target"
$triple=((& rustc -vV) | Where-Object {$_ -like 'host: *'}).Substring(6)
$remap="target.$triple.rustflags=['--remap-path-prefix=$junction=/src','--remap-path-prefix=$($env:CARGO_HOME)=/cargo']"
function Build([string]$name) {
    Set-Location $build
    $ErrorActionPreference='Continue'
    & cargo build --locked -p cua-driver --release --config $remap *> "$Out\$name.log"
    $code=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    if($code -ne 0){throw "$name build failed with exit $code; see $Out\$name.log"}
    Copy-Item "$root\target\release\cua-driver.exe" "$Out\$name.exe" -Force
    $hash=(Get-FileHash "$Out\$name.exe" -Algorithm SHA256).Hash
    # Host output only: anything written to the pipeline here would become
    # part of the return value, and hashes.json would get an array.
    Write-Host "$name $hash"
    return $hash
}
function Touch { (Get-Item $main).LastWriteTime=Get-Date }
function Edited([string]$name,[string]$file,[string]$marker,[string]$replacement) {
    $original=[IO.File]::ReadAllText($file)
    # Markers are written with LF; a tree extracted with CRLF endings gets
    # the same marker in CRLF.
    if($original.Contains("`r`n")){$marker=$marker.Replace("`r`n","`n").Replace("`n","`r`n");$replacement=$replacement.Replace("`r`n","`n").Replace("`n","`r`n")}
    if(-not $original.Contains($marker)){throw "$name marker not found in $file"}
    [IO.File]::WriteAllText($file,$original.Replace($marker,$replacement))
    try { Build $name | Out-Null } finally { [IO.File]::WriteAllText($file,$original) }
}
# One marker replaced in one file; the variant's hash goes to $hashes.
function Variant([string]$name,[string]$file,[string]$marker,[string]$replacement) {
    if(-not (Want $name)){return}
    Edited $name $file $marker $replacement
    $script:hashes.$name=(Get-FileHash "$Out\$name.exe").Hash
}
function Want([string]$name){ return (-not $Only.Count) -or ($name -in $Only) }
$hashes=[ordered]@{}
if(Want 'green'){ $hashes.green=Build 'green' }
if(Want 'repro-green'){ Touch; $hashes.'repro-green'=Build 'repro-green' }
if(Want 'no-minimized-guard'){
    $m='fn minimized_pixel_refusal(hwnd: u64, px: f64, py: f64) -> Option<ToolResult> {'
    Edited 'no-minimized-guard' "$src\tools\impl_.rs" $m ($m+"`n    if true { return None; }")
    $hashes.'no-minimized-guard'=(Get-FileHash "$Out\no-minimized-guard.exe").Hash
}
if(Want 'no-frame-binding'){
    Edited 'no-frame-binding' "$src\tools\impl_.rs" 'let Some(id) = id.filter(|v| !v.is_null()) else {' 'let Some(id) = None::<&Value> else {'
    $hashes.'no-frame-binding'=(Get-FileHash "$Out\no-frame-binding.exe").Hash
}
$mouse="$src\input\mouse.rs";$impl="$src\tools\impl_.rs";$capture="$src\capture.rs";$inject="$src\input\inject.rs"
$core="$root\crates\cua-driver-core\src\capture_log.rs"
Variant 'no-client-guard' $mouse 'if client.x < area.left || client.y < area.top || client.x >= area.right || client.y >= area.bottom {' 'if false {'
Variant 'any-session' $core 'let own = session.and_then(|s| self.session_frame(s, pid, window_id));' 'let own = session.and(self.sessions.values().filter_map(|entry| entry.windows.get(&(pid, window_id)).map(|(_, frame)| *frame)).max_by_key(|frame| frame.id));'
Variant 'no-session-identity' $impl 'binding.verify_identity(|| crate::capture::current_identity_mark(hwnd))?;' ''
Variant 'no-identity' $impl 'if !crate::capture::same_window(&frame) {' 'if false {'
$m='fn bound_pixel(args: &Value, state: &ToolState, pid: u32, hwnd: u64, x: f64, y: f64) -> Result<(f64, f64), ToolResult> {'
Variant 'no-scroll-binding' $impl $m ($m+"`n    if true { return Ok((x, y)); }")
Variant 'no-drag-endpoints' $impl 'let endpoint_id = |field: &''static str| match args.get(field).filter(|v| !v.is_null()) {' 'let endpoint_id = |field: &''static str| match None::<&Value>.filter(|_| field.is_empty()) {'
Variant 'no-id-effect' $impl "`"status`": `"refused`",`n        `"code`": `"invalid_arguments`",`n        `"effect`": `"refused`",`n        `"refusal`": { `"code`": `"invalid_arguments`", `"message`": message }" '"code": "invalid_arguments"'
$w='fn pen_cannot_reach_wpf(target: HWND) -> Option<String> {'
Variant 'no-wpf-gate' $inject $w ($w+"`n    if true { return None; }")
Variant 'digitizer-exemption' $inject $w ($w+"`n    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_DIGITIZER) } & 0x80 != 0 { return None; }")
$m='fn wpf_drops_posted_input(target: HWND) -> Option<DroppedTarget> {'
Variant 'no-wpf-child' $mouse $m ($m+"`n    if true { return None; }")
$m='fn in_window_region(window: HWND, rect: &RECT, point: POINT) -> bool {'
Variant 'no-region' $mouse $m ($m+"`n    if true { return true; }")
$m='fn hit_transparent(window: HWND, point: POINT) -> bool {'
Variant 'no-httransparent' $mouse $m ($m+"`n    if true { return false; }")
Variant 'skip-ex-transparent' $mouse 'if unsafe { IsWindowVisible(child).as_bool() && IsWindowEnabled(child).as_bool() }' 'if unsafe { IsWindowVisible(child).as_bool() && IsWindowEnabled(child).as_bool() && (GetWindowLongPtrW(child, GWL_EXSTYLE) as u32 & windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT.0) == 0 }'
Variant 'skip-transparent-subtree' $mouse '.filter(|(child, rect)| contains(rect) && hold(*child))' '.filter(|(child, rect)| contains(rect) && hold(*child) && opaque(*child))'
$m='pub fn hosted_win32_child_at(root: u64, sx: i32, sy: i32) -> Option<u64> {'
Variant 'wfhost-refused' $mouse $m ($m+"`n    if true { return None; }")
Variant 'hosted-any-target' $mouse "(target != root`n        && !is_wpf_target_window(target)`n        && !would_be_silently_dropped(target, EventKind::MouseClick))" '(target == target)'
if(Want 'hosted-any-target-no-wpf-child'){
    $original=[IO.File]::ReadAllText($mouse)
    try {
        $m='fn wpf_drops_posted_input(target: HWND) -> Option<DroppedTarget> {'
        $hosted="(target != root`n        && !is_wpf_target_window(target)`n        && !would_be_silently_dropped(target, EventKind::MouseClick))"
        if($original.Contains("`r`n")){$hosted=$hosted.Replace("`n","`r`n")}
        if(-not $original.Contains($hosted)){throw 'hosted-any-target-no-wpf-child marker not found'}
        [IO.File]::WriteAllText($mouse,$original.Replace($hosted,'(target == target)'))
        Edited 'hosted-any-target-no-wpf-child' $mouse $m ($m+"`n    if true { return None; }")
    } finally { [IO.File]::WriteAllText($mouse,$original) }
    $hashes.'hosted-any-target-no-wpf-child'=(Get-FileHash "$Out\hosted-any-target-no-wpf-child.exe").Hash
}
$m='fn resolve_post_target(root: HWND, screen_pt: POINT) -> Result<(HWND, POINT)> {'
Variant 'refuse-posted' $mouse $m ($m+"`n    if true { return Err(UnmappedPoint(`"refuse-posted control`".into()).into()); }")
if((Want 'red') -or (Want 'repro-red')){
    Set-Location $Vendor
    & git.exe apply -R --check $Delta; if($LASTEXITCODE){throw 'delta does not reverse-apply'}
    & git.exe apply -R $Delta; if($LASTEXITCODE){throw 'delta reverse-apply failed'}
    try {
        $hashes.red=Build 'red'
        if(Want 'repro-red'){ Touch; $hashes.'repro-red'=Build 'repro-red' }
    } finally {
        Set-Location $Vendor
        & git.exe apply $Delta
        & git.exe apply -R --check $Delta; if($LASTEXITCODE){throw 'tree was not restored'}
        "restored"
    }
}
# Leave target\release holding the green build again.
if(Want 'red'){ Build 'green-after' | Out-Null; $hashes.'green-after'=(Get-FileHash "$Out\green-after.exe").Hash }
$hashes.source_revision=$Revision
$patch="$tree\impl\native\cua-driver.patch"
$hashes.patch_sha256=$(if(Test-Path $patch){(Get-FileHash $patch -Algorithm SHA256).Hash}else{''})
$hashes.path_remap="$junction ($tree)=/src $($env:CARGO_HOME)=/cargo"
# The toolchain the builds used, as build.ps1 records it for a package.
Set-Location $tree
$hashes.rustc=((& rustc -vV) | Where-Object {$_ -match '^(rustc |commit-hash: |LLVM version: )'}) -join '; '
$hashes.msvc_toolset=$env:VCToolsVersion
$hashes.windows_sdk="$env:WindowsSDKVersion".TrimEnd('\')
$hashes.visual_studio=$env:VSCMD_VER
$hashes | ConvertTo-Json | Set-Content "$Out\hashes.json" -Encoding UTF8
icacls $Out /grant 'Users:(OI)(CI)RX' /T /Q | Out-Null
Set-Location $Out; cmd /c rmdir $junction
$hashes
