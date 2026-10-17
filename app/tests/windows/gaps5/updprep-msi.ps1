param([string]$Dir='C:\ICUAudit\upd')
# Run as SYSTEM after gaps4\phase1-builds.ps1 (which makes build-0.1.0 and build-0.2.0 with both installers, signed with the real
# certificate, and the NSIS manifests). Adds the files and manifests the MSI cases need:
#   manifest-bare.json  = manifest-good.json: the 0.2.0 NSIS installer under the bare `windows-x86_64` key (what the app's docs describe)
#   manifest-msi.json   = the 0.2.0 MSI under `windows-x86_64-msi` (and the NSIS one under `windows-x86_64-nsis`)
$ErrorActionPreference='Stop'
$msi=@(Get-ChildItem "$Dir\build-0.2.0" -Filter '*.msi')[0]
if(-not $msi){ throw 'no 0.2.0 MSI in the build folder' }
$sigFile="$($msi.FullName).sig"
if(-not (Test-Path $sigFile)){ throw "no updater signature next to $($msi.Name)" }
Copy-Item $msi.FullName "$Dir\files\Copilot_0.2.0_x64.msi" -Force
$msiSig=(Get-Content $sigFile -Raw).Trim()
$nsisSetup="$Dir\build-0.2.0\Copilot_0.2.0_x64-setup.exe"
$nsisSig=(Get-Content "$nsisSetup.sig" -Raw).Trim()
function Entry([string]$sig,[string]$file){ @{signature=$sig;url="http://127.0.0.1:18080/files/$file"} }
$manifest=@{version='0.2.0';notes='updater test';pub_date='2026-10-05T00:00:00Z';platforms=@{
    'windows-x86_64-msi'=(Entry $msiSig 'Copilot_0.2.0_x64.msi');
    'windows-x86_64-nsis'=(Entry $nsisSig 'Copilot_0.2.0_x64-setup.exe')}}
($manifest | ConvertTo-Json -Depth 6) | Set-Content "$Dir\manifest-msi.json" -Encoding UTF8
Copy-Item "$Dir\manifest-good.json" "$Dir\manifest-bare.json" -Force
"msi $($msi.Name) $($msi.Length) bytes; signature length $($msiSig.Length)"
Get-Content "$Dir\manifest-msi.json" -Raw
Get-ChildItem "$Dir\files" | Select-Object Name,Length | Format-Table -AutoSize | Out-String
