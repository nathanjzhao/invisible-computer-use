param(
    # none: the tree as built. raise: the target is not made the window at the press point first. fwd: the daemon's drag is not forwarded to the worker. points: the worker checks only the start
    # point of a drag. validate: the worker does not validate a drag's shape.
    [Parameter(Mandatory=$true)][ValidateSet('none','fwd','points','validate','raise')][string]$Mutant,
    [Parameter(Mandatory=$true)][string]$Root,
    [string]$Out='C:\ICUAudit\mut'
)
# Run as SYSTEM (as-system.ps1). Builds the driver and the UIAccess worker from $Root's vendor tree with one guard of the
# drag forwarding removed, the way apply-and-build.sh does on Windows (fixed junction, path remapping, one cargo run per
# package), puts them in $Out\<mutant>, and restores the source. Signing and installing is gaps4\mutinstall.ps1's job.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$rust="$Root\vendor-cua\libs\cua-driver\rust"
$files=@{
    fwd="$rust\crates\platform-windows\src\input\mouse.rs";
    points="$rust\crates\platform-windows\src\uia_protocol.rs";
    validate="$rust\crates\platform-windows\src\uia_protocol.rs";
    raise="$rust\crates\platform-windows\src\input\mouse.rs"
}
# the drag's UIPI check never fires: the call goes on to SendInput, which UIPI drops without an error
$fwdOld="post_message_blocked_by_uipi(target.0 as u64) {`n        // Like the click: SendInput from below"
$fwdNew="post_message_blocked_by_uipi(target.0 as u64).filter(|_| false) {`n        // Like the click: SendInput from below"
$edits=@{
    fwd=@($fwdOld,$fwdNew);
    points=@('InputOp::Drag { from_x, from_y, to_x, to_y, .. } => vec![(*from_x, *from_y), (*to_x, *to_y)],','InputOp::Drag { from_x, from_y, .. } => vec![(*from_x, *from_y)],')
    validate=@('if *duration_ms > DRAG_MAX_DURATION_MS {','if false {');
    # the drag (and the wheel) no longer make the target the window at the press point first
    raise=@("    if !target_is_under_point(target, sx, sy) {`n        // 1. Raise it without taking focus.","    if false {`n        // 1. Raise it without taking focus.")
}
function Run([string]$exe,[string[]]$arguments,[string]$log) {
    $p=Get-Command $exe -CommandType Application | Select-Object -First 1
    $prev=$ErrorActionPreference;$ErrorActionPreference='Continue'
    & $p.Source @arguments *> $log
    $code=$LASTEXITCODE;$ErrorActionPreference=$prev
    if($code -ne 0){throw "$exe failed ($code); see $log"}
}
New-Item -ItemType Directory -Force "$Out\$Mutant" | Out-Null
$env:CARGO_HOME='C:\ICUAudit\cargo'; $env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH="C:\ICUAudit\cargo\bin;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
$backup=$null
try {
    if($Mutant -ne 'none'){
        $file=$files[$Mutant]; $backup="$Out\$Mutant\original.rs"
        Copy-Item $file $backup -Force
        $text=[IO.File]::ReadAllText($file)
        $old=$edits[$Mutant][0]; $new=$edits[$Mutant][1]
        $i=$text.IndexOf($old); if($i -lt 0){ throw "mutation anchor not found for $Mutant" }
        if($text.IndexOf($old,$i+1) -ge 0 -and $Mutant -ne 'validate'){ throw "mutation anchor not unique for $Mutant" }
        $text=$text.Remove($i,$old.Length).Insert($i,$new)
        [IO.File]::WriteAllText($file,$text,(New-Object Text.UTF8Encoding($false)))
        "mutant $Mutant : $file differs from the original: " + ((Get-FileHash $file).Hash -ne (Get-FileHash $backup).Hash)
    }
    $junction='C:\icu-driver-src'
    if(Test-Path $junction){cmd /c "rmdir $junction"}
    cmd /c "mklink /J $junction $Root" | Out-Null
    try {
        $hostTriple=(& rustc -vV | Select-String 'host: ').ToString().Replace('host: ','').Trim()
        $config="target.$hostTriple.rustflags=['--remap-path-prefix=$junction=/src','--remap-path-prefix=$($env:CARGO_HOME)=/cargo']"
        $manifest="$junction\vendor-cua\libs\cua-driver\rust\Cargo.toml"
        $env:CARGO_TARGET_DIR="$junction\vendor-cua\libs\cua-driver\rust\target"
        foreach($package in 'cua-driver','cua-driver-uia'){
            $sw=[Diagnostics.Stopwatch]::StartNew()
            Run 'cargo.exe' @('build','--locked','-p',$package,'--release','--manifest-path',$manifest,'--config',$config) "$Out\$Mutant\build-$package.log"
            "$Mutant $package built in $([int]$sw.Elapsed.TotalSeconds) s"
        }
    } finally { cmd /c "rmdir $junction" | Out-Null }
    $target="$rust\target\release"
    Copy-Item "$target\cua-driver.exe","$target\cua-driver-uia.exe" "$Out\$Mutant" -Force
    Get-FileHash "$Out\$Mutant\*.exe" -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash,(Split-Path $_.Path -Leaf) }
} finally {
    if($backup -and (Test-Path $backup)){ Copy-Item $backup $files[$Mutant] -Force; "restored $($files[$Mutant]): " + ((Get-FileHash $files[$Mutant]).Hash -eq (Get-FileHash $backup).Hash) }
}
