param(
    # none: the tree as it is. child: a focused child reported as the foreground window is not taken for its top-level window.
    # cover: a foreground click is sent even when another window covers the point. menu: an open menu is not told from none,
    # so the previous foreground window is restored over it.
    [Parameter(Mandatory=$true)][ValidateSet('none','child','cover','menu')][string]$Mutant,
    [Parameter(Mandatory=$true)][string]$Root,
    [string]$Out='C:\ICUAudit\mut5',
    [switch]$Uia,
    # also run these cargo test filters against the mutated source (the unit tests that must go red)
    [string[]]$Test=@(),
    # also compile the unit tests and run these ones as the interactive user (usertest.ps1): the ones that need a visible window
    [string[]]$UserTest=@()
)
# Run as SYSTEM (as-system.ps1). Builds the driver (and with -Uia the UIAccess worker) from $Root's vendor tree with one guard
# removed, the way apply-and-build.sh does on Windows (fixed junction, path remapping, one cargo run per package), puts the
# binaries in $Out\<mutant>, and restores the source.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$rust="$Root\vendor-cua\libs\cua-driver\rust"
$src="$rust\crates\platform-windows\src"
$files=@{ child="$src\win32\windows.rs"; cover="$src\input\mouse.rs"; menu="$src\input\inject.rs" }
$edits=@{
    child=@('    root_of(window).unwrap_or(window)','    let _ = &root_of; window');
    cover=@('if let Some(covering) = wait_while_covered(target, sx, sy, 400) {','if let Some(covering) = wait_while_covered(target, sx, sy, 400).filter(|_| false) {');
    menu=@('    flags & GUI_MENU_MODE_BITS != 0','    flags & GUI_MENU_MODE_BITS == u32::MAX')
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
        if($text.IndexOf($old,$i+1) -ge 0){ throw "mutation anchor not unique for $Mutant" }
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
        $packages=@('cua-driver'); if($Uia){ $packages+='cua-driver-uia' }
        foreach($package in $packages){
            $sw=[Diagnostics.Stopwatch]::StartNew()
            Run 'cargo.exe' @('build','--locked','-p',$package,'--release','--manifest-path',$manifest,'--config',$config) "$Out\$Mutant\build-$package.log"
            "$Mutant $package built in $([int]$sw.Elapsed.TotalSeconds) s"
        }
    } finally { cmd /c "rmdir $junction" | Out-Null }
    $target="$rust\target\release"
    Copy-Item "$target\cua-driver.exe" "$Out\$Mutant" -Force
    if($Uia){ Copy-Item "$target\cua-driver-uia.exe" "$Out\$Mutant" -Force }
    $ErrorActionPreference='Continue'   # cargo test writes progress to stderr
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue   # it named the junction, which is gone by now
    foreach($f in $Test){
        Push-Location $rust
        & cargo.exe test --locked -p platform-windows --lib $f -- --test-threads=1 *> "$Out\$Mutant\test-$($f -replace '[^a-z_]','').log"
        "cargo test $f under mutant $Mutant exit $LASTEXITCODE"
        Get-Content "$Out\$Mutant\test-$($f -replace '[^a-z_]','').log" | Select-String -Pattern '^test |test result|panicked' | ForEach-Object { $_.Line }
        Pop-Location
    }
    if($UserTest.Count){
        Push-Location $rust
        & cargo.exe test --locked -p platform-windows --lib --no-run *> "$Out\$Mutant\test-build.log"
        "cargo test --no-run under mutant $Mutant exit $LASTEXITCODE"
        Pop-Location
        & (Join-Path $PSScriptRoot 'usertest.ps1') -Root $Root -Filter $UserTest
    }
    Get-FileHash "$Out\$Mutant\*.exe" -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash,(Split-Path $_.Path -Leaf) }
} finally {
    if($backup -and (Test-Path $backup)){ Copy-Item $backup $files[$Mutant] -Force; "restored $($files[$Mutant]): " + ((Get-FileHash $files[$Mutant]).Hash -eq (Get-FileHash $backup).Hash) }
}
