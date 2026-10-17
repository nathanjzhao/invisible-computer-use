param(
    [Parameter(Mandatory=$true)][string]$File,
    [string]$SignTool='C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe'
)
# Needs Nathan item 8: a file signed on 2026-10-05 with the three day Azure Artifact Signing certificate (valid 2026-10-04 08:08:32Z to
# 2026-10-07 08:08:32Z) must still verify as Valid after 2026-10-07 08:08Z, because the signature carries a Microsoft RFC 3161
# timestamp made while the certificate was valid. Run on any Windows machine, before and after the expiry, on the saved signed file
# (study/audit/gaps-windows-5-2026-10-05/results/item4/signed-2026-10-05-uninstall.exe). Prints what a reader needs and exits 1 unless Valid.
$ErrorActionPreference='Stop'
$s=Get-AuthenticodeSignature $File
$fmt='yyyy-MM-dd HH:mm:ss'
'file             {0}' -f $File
'sha256           {0}' -f (Get-FileHash $File -Algorithm SHA256).Hash
'status           {0}' -f $s.Status
'status message   {0}' -f $s.StatusMessage
'signer           {0}' -f $s.SignerCertificate.Subject
'signer valid     {0}Z to {1}Z' -f $s.SignerCertificate.NotBefore.ToUniversalTime().ToString($fmt),$s.SignerCertificate.NotAfter.ToUniversalTime().ToString($fmt)
'timestamper      {0}' -f $s.TimeStamperCertificate.Subject
'now              {0}Z' -f (Get-Date).ToUniversalTime().ToString($fmt)
if(Test-Path $SignTool){ & $SignTool verify /pa /v $File 2>&1 | Select-String -Pattern 'Successfully verified|Signing Certificate Chain|Timestamp|Issued to|Expires|Number of errors' | ForEach-Object { $_.Line } }
if($s.Status -ne 'Valid'){ exit 1 }
