param(
    [Parameter(Mandatory=$true)][string]$SecretName,
    [Parameter(Mandatory=$true)][string]$Root,
    # lib.rs, main.ts and updater.ts as the commit before this change had them (git show <rev>:<path>)
    [string]$OldDir='C:\ICUAudit\old'
)
# Run as SYSTEM (as-system.ps1). The baseline for the cancelled-elevation run: the same 0.1.0 build with the plugin's own
# install step (the three files that carry the change put back as they were), real-signed the same way, in
# C:\ICUAudit\upd\build-0.1.0old\Copilot_0.1.0old_x64-setup.exe (the name updinstall.ps1 -Version 0.1.0old looks for).
# The tree is restored afterwards.
$ErrorActionPreference='Continue'
$map=@{ 'lib.rs'="$Root\app\src-tauri\src\lib.rs"; 'main.ts'="$Root\app\src\main.ts"; 'updater.ts'="$Root\app\src\updater.ts" }
$bak='C:\ICUAudit\old-backup'; New-Item -ItemType Directory -Force $bak | Out-Null
foreach($k in $map.Keys){ Copy-Item $map[$k] "$bak\$k" -Force }
try {
    foreach($k in $map.Keys){ Copy-Item "$OldDir\$k" $map[$k] -Force }
    if(Test-Path "$Root\app\src-tauri\target\release\bundle"){ cmd /c "rd /s /q `"$Root\app\src-tauri\target\release\bundle`"" }
    & C:\ICUAudit\signreal.ps1 -Label OLD -Root $Root -Config C:\ICUAudit\cfg-B.json -SecretName $SecretName -UpdaterKeyFile C:\ICUAudit\upd\k1.key
    "signreal exit $LASTEXITCODE"
    $out='C:\ICUAudit\upd\build-0.1.0old'; New-Item -ItemType Directory -Force $out | Out-Null
    Get-ChildItem "$Root\app\src-tauri\target\release\bundle\nsis\*" -File | ForEach-Object { Copy-Item $_.FullName (Join-Path $out ($_.Name -replace '0\.1\.0','0.1.0old')) -Force }
    Get-ChildItem $out | Select-Object Name,Length | Format-Table -AutoSize | Out-String
    Get-Content C:\ICUAudit\signbuild-OLD-status.txt
} finally {
    foreach($k in $map.Keys){ Copy-Item "$bak\$k" $map[$k] -Force }
    "restored the tree"
}
"done $(Get-Date -Format u)"
