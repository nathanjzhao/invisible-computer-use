param([string]$Root='C:\ICUAudit\source-660cc19')
# Rebuild the app crate's test executable from the CURRENT source (memory.rs restored) and run the whole suite, as build.ps1 does.
$ErrorActionPreference='Continue'
$base='C:\ICUAudit'
Copy-Item "$base\memmut\memory.rs.orig" "$Root\app\src-tauri\src\memory.rs" -Force; (Get-Item "$Root\app\src-tauri\src\memory.rs").LastWriteTime=Get-Date  # cargo compares mtimes: a copy keeps the old one
$env:CARGO_HOME="$base\cargo"; $env:RUSTUP_HOME="$base\rustup"
$env:PATH="$base\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'; $env:CI='true'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Set-Location "$Root\app"
& cargo.exe test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run *> "$base\full-build.log"
"build exit $LASTEXITCODE"
$exe=@(Get-ChildItem "$Root\app\src-tauri\target\debug\deps\app_lib-*.exe" | Sort-Object LastWriteTime -Descending)[0]
& mt.exe -nologo -manifest "$base\test-app.manifest" ("-outputresource:"+$exe.FullName+';#1') | Out-Null
$t0=Get-Date
& $exe.FullName --skip agent::e2e *> "$base\full-run.log"
"suite exit $LASTEXITCODE after $([int]((Get-Date)-$t0).TotalSeconds) s"
Get-Content "$base\full-run.log" | Where-Object { $_ -match 'test result|FAILED|panicked' } | Select-Object -First 30
