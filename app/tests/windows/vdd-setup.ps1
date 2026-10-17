param([Parameter(Mandatory=$true)][string]$Zip)
# SYSTEM, once per disposable host: install the signed Virtual Display Driver
# (VirtualDrivers/Virtual-Display-Driver release 25.7.23, asset
# VirtualDisplayDriver-x86.Driver.Only.zip) with two extra monitors. Then, as
# the standard user:
#   as-user.ps1 -Script layout.ps1 -Arguments 'standard'
# which finds the two virtual monitors by name (DISPLAY2/3 on a fresh host,
# DISPLAY4/5 on the first audit host), puts one at -1920,0 at 150% and the
# other at 0,-1080 at 125%, and prints `layout.ps1 list`.
$ErrorActionPreference='Stop'
if((Get-FileHash $Zip -Algorithm SHA256).Hash -ne 'E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A'){throw 'unexpected driver archive'}
$root='C:\ICUAudit\vdd'
New-Item -ItemType Directory -Force $root,'C:\VirtualDisplayDriver' | Out-Null
Expand-Archive $Zip $root -Force
# The driver reads its monitor count from this fixed path.
$settings=[xml](Get-Content "$root\VirtualDisplayDriver\vdd_settings.xml")
$settings.vdd_settings.monitors.count='2'
$settings.Save('C:\VirtualDisplayDriver\vdd_settings.xml')
$sig=Get-AuthenticodeSignature "$root\VirtualDisplayDriver\mttvdd.cat"
if($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike 'CN=SignPath Foundation*'){throw 'unexpected driver signer'}
$store=New-Object Security.Cryptography.X509Certificates.X509Store('TrustedPublisher','LocalMachine')
$store.Open('ReadWrite');$store.Add($sig.SignerCertificate);$store.Close()
Add-Type -Path (Join-Path $PSScriptRoot 'DeviceNode.cs')
"reboot required: $([DeviceNode]::Install("$root\VirtualDisplayDriver\MttVDD.inf",'Root\MttVDD'))"
Start-Sleep -Seconds 5
Get-PnpDevice -Class Display | Select-Object FriendlyName,Status,InstanceId | Format-Table -AutoSize | Out-String -Width 200
