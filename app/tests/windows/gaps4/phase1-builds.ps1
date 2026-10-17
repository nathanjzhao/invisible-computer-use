param(
    [Parameter(Mandatory=$true)][string]$SecretName,
    [Parameter(Mandatory=$true)][string]$Root,
    [string]$Harness='C:\ICUAudit\harness'
)
# Run as SYSTEM (as-system.ps1) after build.ps1 prepared $Root. Makes the two updater-test builds with the REAL Azure
# Artifact Signing certificate (the service principal is read from the one-off Secrets Manager secret by getsecret.ps1,
# into the signing process's environment only):
#   C:\ICUAudit\upd\build-0.1.0  the build under test (this tree), updater pointed at the local server, key k1
#   C:\ICUAudit\upd\build-0.2.0  the update the server offers
# then the manifests (good, wrong key, tampered, downgrade, version mismatch).
$ErrorActionPreference='Continue'
foreach($f in 'signreal.ps1','updbuild.ps1','updinstall.ps1','updprep.ps1','uacset.ps1','updater.ps1'){ Copy-Item (Join-Path $Harness "gaps2\$f") 'C:\ICUAudit\' -Force }
New-Item -ItemType Directory -Force 'C:\ICUAudit\upd' | Out-Null
Copy-Item (Join-Path $Harness 'gaps2\upd-server.js') 'C:\ICUAudit\upd\' -Force
"=== keys";      & C:\ICUAudit\updprep.ps1 -Step keys -Root $Root -Dir C:\ICUAudit\upd
"=== configs";   & (Join-Path $Harness 'gaps4\mkcfg.ps1') -Dir C:\ICUAudit\upd -Out C:\ICUAudit
"=== builds";    & C:\ICUAudit\updbuild.ps1 -SecretName $SecretName -Root $Root
"=== manifests"; & C:\ICUAudit\updprep.ps1 -Step manifests -Root $Root -Dir C:\ICUAudit\upd
"done $(Get-Date -Format u)"
