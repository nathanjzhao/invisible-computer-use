param([string]$Root='C:\ICUAudit\source-660cc19')
# Mutation proof for updater_scope_arg: the old behaviour (no scope switch) must fail the new test.
$ErrorActionPreference='Continue'
$base='C:\ICUAudit'
$f="$Root\app\src-tauri\src\permissions.rs"
$good=Get-Content $f -Raw
$mut=$good.Replace('Some(if uia_path_ok(exe, program_files) { "/AllUsers" } else { "/CurrentUser" })','None')
if($mut -eq $good){ throw 'mutation did not apply' }
$env:CARGO_HOME="$base\cargo"; $env:RUSTUP_HOME="$base\rustup"
$env:PATH="$base\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'; $env:CI='true'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Set-Location "$Root\app"
function Run([string]$label){
    & cargo.exe test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run *> "$base\perm-build-$label.log"
    $exe=@(Get-ChildItem "$Root\app\src-tauri\target\debug\deps\app_lib-*.exe" | Sort-Object LastWriteTime -Descending)[0]
    & mt.exe -nologo -manifest "$base\test-app.manifest" ("-outputresource:"+$exe.FullName+';#1') | Out-Null
    & $exe.FullName permissions *> "$base\perm-run-$label.log"
    "$label exit $LASTEXITCODE"
    Get-Content "$base\perm-run-$label.log" | Where-Object { $_ -match '^test |test result|panicked|left:|right:' }
}
try {
    [IO.File]::WriteAllText($f,$mut); (Get-Item $f).LastWriteTime=Get-Date
    Run 'mutant-noarg'
} finally {
    [IO.File]::WriteAllText($f,$good); (Get-Item $f).LastWriteTime=Get-Date
}
Run 'restored'
