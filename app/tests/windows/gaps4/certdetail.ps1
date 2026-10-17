param(
    [string]$Install='C:\Program Files\Copilot',
    [string]$Builds='C:\ICUAudit\upd\build-0.1.0'
)
# Certificate details of the signed build: every PE of the install and the installers, with signtool's chain output for
# the driver and the worker, and the host's trust stores (no throwaway CA may be trusted when this is read).
$ErrorActionPreference='Continue'
$st=(Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' | Sort-Object FullName | Select-Object -Last 1).FullName
"signtool: $st"
"host: " + (Get-CimInstance Win32_OperatingSystem).Caption
"time: " + (Get-Date).ToUniversalTime().ToString('u')
"## trust stores: certificates whose subject mentions ICU, throwaway or uia-forward (must be none)"
foreach($store in 'Cert:\LocalMachine\Root','Cert:\LocalMachine\TrustedPublisher','Cert:\LocalMachine\My','Cert:\CurrentUser\Root','Cert:\CurrentUser\TrustedPublisher','Cert:\CurrentUser\My'){
    $n=@(Get-ChildItem $store -ErrorAction SilentlyContinue | Where-Object { $_.Subject -match 'ICU|throwaway|uia-forward' }).Count
    "{0,-40} total {1,3}   matching {2}" -f $store,@(Get-ChildItem $store -ErrorAction SilentlyContinue).Count,$n
}
$root=Get-ChildItem Cert:\LocalMachine\Root | Where-Object { $_.Subject -match 'Microsoft Identity Verification Root' }
"Microsoft Identity Verification Root CA 2020 in LocalMachine\Root: " + $(if($root){$root.Thumbprint}else{'absent'})
"## installed files"
$files=@(Get-ChildItem $Install -File | Where-Object { $_.Extension -in '.exe','.dll' })
foreach($f in $files){
    $s=Get-AuthenticodeSignature $f.FullName; $c=$s.SignerCertificate
    '{0,-24} {1,-8} sha256 {2}' -f $f.Name,$s.Status,(Get-FileHash $f.FullName -Algorithm SHA256).Hash
    if($c){ '   subject  {0}' -f $c.Subject; '   issuer   {0}' -f $c.Issuer; '   serial   {0}   thumbprint {1}' -f $c.SerialNumber,$c.Thumbprint
            '   valid    {0} to {1}   EKU {2}' -f $c.NotBefore.ToUniversalTime().ToString('u'),$c.NotAfter.ToUniversalTime().ToString('u'),(($c.EnhancedKeyUsageList | ForEach-Object { $_.FriendlyName }) -join ', ')
            '   timestamp {0}' -f $s.TimeStamperCertificate.Subject }
}
"## installers"
foreach($f in @(Get-ChildItem $Builds -File | Where-Object { $_.Extension -in '.exe','.msi' })){
    $s=Get-AuthenticodeSignature $f.FullName; $c=$s.SignerCertificate
    '{0,-40} {1,-8} {2}  thumbprint {3}' -f $f.Name,$s.Status,(Get-FileHash $f.FullName -Algorithm SHA256).Hash,$c.Thumbprint
}
foreach($name in 'cua-driver.exe','cua-driver-uia.exe','app.exe'){
    "## signtool verify /pa /v $name"
    & $st verify /pa /v (Join-Path $Install $name) 2>&1 | Out-String
}
