param(
    [Parameter(Mandatory=$true)][string]$Root,
    [string[]]$Filter=@('elevated_update')
)
# Run as SYSTEM (SSM). The app crate's lib tests for the given name filters, built and run the way build.ps1 does: Tauri embeds
# Common Controls v6 in the app, but not in its library test executable, so that same manifest is put into the test executable
# (without it the executable does not start: STATUS_ENTRYPOINT_NOT_FOUND). Needs the app built once (release\app.exe).
$ErrorActionPreference='Continue'
$env:CARGO_HOME='C:\ICUAudit\cargo'; $env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH="C:\ICUAudit\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Set-Location "$Root\app"
$log='C:\ICUAudit\apptest.log'
& cargo.exe test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run *> $log
"cargo test --no-run exit $LASTEXITCODE"
if($LASTEXITCODE){ Get-Content $log | Select-String -Pattern '^error' -Context 0,6 | Select-Object -First 8 | ForEach-Object { $_.ToString() }; return }
$manifest='C:\ICUAudit\test-app.manifest'
& mt.exe -nologo ("-inputresource:$Root\app\src-tauri\target\release\app.exe;#1") ("-out:$manifest") | Out-Null
$exe=@(Get-ChildItem "$Root\app\src-tauri\target\debug\deps\app_lib-*.exe" | Sort-Object LastWriteTime -Descending)[0]
& mt.exe -nologo -manifest $manifest ("-outputresource:$($exe.FullName);#1") | Out-Null
foreach($f in $Filter){
    & $exe.FullName $f *> $log
    "test executable, filter $f, exit $LASTEXITCODE"
    Get-Content $log | Select-String -Pattern '^test |test result|panicked' | ForEach-Object { $_.Line }
}
