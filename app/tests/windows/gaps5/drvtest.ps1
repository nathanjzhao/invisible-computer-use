param(
    [Parameter(Mandatory=$true)][string]$Root,
    # one name filter per cargo test run (substring of a test path)
    [string[]]$Filter=@('a_focused_child_reported_as_foreground','a_real_child_of_the_target','a_thread_in_a_menu_loop'),
    [string]$Out='C:\ICUAudit\mut5'
)
# Run as SYSTEM (as-system.ps1). `cargo test -p platform-windows` for the foreground tests of gaps-windows-5, in a desktop session
# with a window station (the real-window test creates two top-level windows and a child). Prints each test's result line.
$ErrorActionPreference='Continue'
$rust="$Root\vendor-cua\libs\cua-driver\rust"
New-Item -ItemType Directory -Force $Out | Out-Null
$env:CARGO_HOME='C:\ICUAudit\cargo'; $env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH="C:\ICUAudit\cargo\bin;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Set-Location $rust
foreach($f in $Filter){
    $log="$Out\test-$($f -replace '[^a-z_]','').log"
    & cargo.exe test --locked -p platform-windows --lib $f -- --test-threads=1 *> $log
    "cargo test $f exit $LASTEXITCODE"
    Get-Content $log | Select-String -Pattern '^test |test result|panicked|error(\[|:)' | ForEach-Object { $_.Line }
}
