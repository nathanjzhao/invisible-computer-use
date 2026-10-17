param(
    [Parameter(Mandatory=$true)][string]$SecretName,
    [Parameter(Mandatory=$true)][string]$Root,
    # elevated_update.rs, lib.rs, main.ts, updater.ts as the commit before gaps-windows-5 had them (git show 9fad1227:<path>)
    [string]$OldDir='C:\ICUAudit\old5'
)
# Run as SYSTEM (as-system.ps1). The baseline for the per-user and MSI update cases: the same 0.1.0 build with the update code
# from before gaps-windows-5 (the plugin installs per-user and MSI copies itself; only an all-users NSIS install is run by
# the app), real-signed the same way, in C:\ICUAudit\upd\build-0.1.0old (names carry 0.1.0old, as oldbuild.ps1 does). The tree is restored.
$ErrorActionPreference='Continue'
$map=@{ 'elevated_update.rs'="$Root\app\src-tauri\src\elevated_update.rs"; 'lib.rs'="$Root\app\src-tauri\src\lib.rs"; 'main.ts'="$Root\app\src\main.ts"; 'updater.ts'="$Root\app\src\updater.ts" }
$bak='C:\ICUAudit\old5-backup'; New-Item -ItemType Directory -Force $bak | Out-Null
foreach($k in $map.Keys){ Copy-Item $map[$k] "$bak\$k" -Force }
try {
    foreach($k in $map.Keys){ Copy-Item "$OldDir\$k" $map[$k] -Force }
    if(Test-Path "$Root\app\src-tauri\target\release\bundle"){ cmd /c "rd /s /q `"$Root\app\src-tauri\target\release\bundle`"" }
    & C:\ICUAudit\signreal.ps1 -Label OLD -Root $Root -Config C:\ICUAudit\cfg-B.json -SecretName $SecretName -UpdaterKeyFile C:\ICUAudit\upd\k1.key
    "signreal exit $LASTEXITCODE"
    $out='C:\ICUAudit\upd\build-0.1.0old'; New-Item -ItemType Directory -Force $out | Out-Null
    Get-ChildItem "$Root\app\src-tauri\target\release\bundle\nsis\*","$Root\app\src-tauri\target\release\bundle\msi\*" -File | ForEach-Object { Copy-Item $_.FullName (Join-Path $out ($_.Name -replace '0\.1\.0','0.1.0old')) -Force }
    Get-ChildItem $out | Select-Object Name,Length | Format-Table -AutoSize | Out-String
    Get-Content C:\ICUAudit\signbuild-OLD-status.txt
} finally {
    foreach($k in $map.Keys){ Copy-Item "$bak\$k" $map[$k] -Force }
    "restored the tree"
}
"done $(Get-Date -Format u)"
