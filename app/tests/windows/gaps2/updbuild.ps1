param([Parameter(Mandatory=$true)][string]$SecretName,[string]$Only='',[string]$Root='C:\ICUAudit\source-660cc19')
# Two builds for the updater test, both signed with the REAL certificate (so the UIAccess worker starts without any
# throwaway CA on the host) and with the app's updater pointed at the local test server with a throwaway minisign key:
#   B1 = 0.1.0 (cfg-B.json), B2 = 0.2.0 (cfg-C.json). Artifacts are copied to C:\ICUAudit\upd\build-<version>.
$ErrorActionPreference='Continue'
foreach($b in @(@('B1','cfg-B.json','0.1.0'),@('B2','cfg-C.json','0.2.0'))){
    if($Only -and $b[0] -ne $Only){ continue }
    "=== $($b[0]) $($b[2])  $(Get-Date -Format u)"
    # the bundle of the previous build must not be mistaken for this one
    if(Test-Path "$Root\app\src-tauri\target\release\bundle"){ cmd /c "rd /s /q `"$Root\app\src-tauri\target\release\bundle`"" }
    & C:\ICUAudit\signreal.ps1 -Label $b[0] -Root $Root -Config "C:\ICUAudit\$($b[1])" -SecretName $SecretName -UpdaterKeyFile C:\ICUAudit\upd\k1.key
    "signreal exit $LASTEXITCODE"
    $out="C:\ICUAudit\upd\build-$($b[2])"
    New-Item -ItemType Directory -Force $out | Out-Null
    Get-ChildItem "$Root\app\src-tauri\target\release\bundle\nsis\*","$Root\app\src-tauri\target\release\bundle\msi\*" -File | Copy-Item -Destination $out -Force
    Get-ChildItem $out | Select-Object Name,Length | Format-Table -AutoSize | Out-String
    "sidecars at build time:"
    foreach($f in 'cua-driver-x86_64-pc-windows-msvc.exe','cua-driver-uia-x86_64-pc-windows-msvc.exe'){ "{0} {1}" -f $f,(Get-FileHash "$Root\app\src-tauri\binaries\$f").Hash }
    Get-Content "C:\ICUAudit\signbuild-$($b[0])-status.txt"
}
"done $(Get-Date -Format u)"
