param(
    [Parameter(Mandatory=$true)][string]$Driver,
    [Parameter(Mandatory=$true)][string]$Worker,
    [Parameter(Mandatory=$true)][string]$Dest,
    [string]$Subject='CN=ICU uia-forward throwaway code signing',
    # Install outside Program Files on purpose, to show the worker then gets no UIAccess.
    [switch]$Outside
)
# Run as SYSTEM or an administrator. Signs a driver and its UIAccess worker with a throwaway
# self-signed code-signing certificate that this host then trusts (LocalMachine Root and
# TrustedPublisher), and installs both under $Dest, which must be below Program Files or Windows
# will not grant the worker UIAccess. Host-only trust: use a disposable machine.
$ErrorActionPreference='Stop'
if (-not $Outside -and $Dest -notlike "$env:ProgramFiles\*") { throw "Dest must be under $env:ProgramFiles (or pass -Outside)" }
$cert=Get-ChildItem Cert:\LocalMachine\My | Where-Object {$_.Subject -eq $Subject -and $_.NotAfter -gt (Get-Date).AddHours(6)} | Select-Object -First 1
if (-not $cert) {
    $cert=New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
        -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddDays(2)
    $cer=Join-Path $env:TEMP 'icu-uia-forward.cer'
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    foreach ($store in 'Root','TrustedPublisher') { Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\$store" | Out-Null }
    Remove-Item $cer
}
New-Item -ItemType Directory -Force $Dest | Out-Null
foreach ($pair in @(@($Driver,'cua-driver.exe'),@($Worker,'cua-driver-uia.exe'))) {
    $target=Join-Path $Dest $pair[1]
    Copy-Item $pair[0] $target -Force
    $result=Set-AuthenticodeSignature -FilePath $target -Certificate $cert -HashAlgorithm SHA256
    if ($result.Status -ne 'Valid') { throw "$($pair[1]) signature is $($result.Status): $($result.StatusMessage)" }
    '{0}  {1}  {2}' -f $pair[1], (Get-AuthenticodeSignature $target).Status, (Get-FileHash $target -Algorithm SHA256).Hash
}
# A standard user must be able to run them but not replace them (Program Files already says so).
(Get-Acl $Dest).Access | Where-Object {$_.IdentityReference -match 'Users'} | ForEach-Object { 'acl  {0}  {1}' -f $_.IdentityReference,$_.FileSystemRights }
