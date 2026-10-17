param(
    [Parameter(Mandatory=$true)][string]$Root,
    [string]$Filter='elevated_update',
    [string]$Log='C:\ICUAudit\apptest.log',
    # none, or the name of one mutation of src-tauri\src\elevated_update.rs to apply for this run (restored afterwards)
    [ValidateSet('none','no-ui-flag','cancel-code','nonzero-is-installed')][string]$Mutant='none'
)
# Run as SYSTEM (as-system.ps1) after build.ps1 prepared $Root: the app crate's unit tests for the cancelled-elevation module on
# Windows, optionally with one guard removed, so a guard is shown to fail on purpose.
$ErrorActionPreference='Stop'
$file="$Root\app\src-tauri\src\elevated_update.rs"
$backup="$Log.orig.rs"
$edits=@{
    'no-ui-flag'=@('SEE_MASK_NOCLOSEPROCESS_BITS | SEE_MASK_FLAG_NO_UI_BITS','SEE_MASK_NOCLOSEPROCESS_BITS');
    'cancel-code'=@('Launch::NotStarted(ERROR_CANCELLED) => Outcome::Kept {','Launch::NotStarted(ERROR_CANCELLED) => Outcome::Installed, Launch::NotStarted(0xFFFF_0000) => Outcome::Kept {');
    'nonzero-is-installed'=@('Launch::Finished(code) => Outcome::Kept {','Launch::Finished(code) if code != 1 => Outcome::Installed, Launch::Finished(code) => Outcome::Kept {')
}
$env:CARGO_HOME='C:\ICUAudit\cargo'; $env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH="C:\ICUAudit\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Copy-Item $file $backup -Force
try {
    if($Mutant -ne 'none'){
        $text=[IO.File]::ReadAllText($file); $old=$edits[$Mutant][0]
        $i=$text.IndexOf($old); if($i -lt 0){ throw "anchor for $Mutant not found" }
        [IO.File]::WriteAllText($file,$text.Remove($i,$old.Length).Insert($i,$edits[$Mutant][1]),(New-Object Text.UTF8Encoding($false)))
    }
    Set-Location "$Root\app\src-tauri"
    $ErrorActionPreference='Continue'
    & cargo.exe test --locked --lib --no-run *> "$Log.build"
    if($LASTEXITCODE){ Get-Content "$Log.build" -Tail 25; throw "mutant $Mutant : the app test build failed" }
    # The library test executable has no Common Controls v6 manifest (the app's own binary does); embed the app's, as build.ps1 does.
    $exe=@(Get-ChildItem "$Root\app\src-tauri\target\debug\deps\app_lib-*.exe" | Sort-Object LastWriteTime -Descending)[0].FullName
    & mt.exe -nologo -manifest 'C:\ICUAudit\test-app.manifest' "-outputresource:$exe;#1" | Out-Null
    & $exe $Filter *> $Log
    "mutant $Mutant : test run exit $LASTEXITCODE"
    Get-Content $Log | Where-Object { $_ -match '^test |test result|panicked|error(\[|:)' } | Select-Object -First 30
} finally {
    Copy-Item $backup $file -Force
    "restored: " + ((Get-FileHash $file).Hash -eq (Get-FileHash $backup).Hash)
}
