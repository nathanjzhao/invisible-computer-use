$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
New-Item -ItemType Directory -Force C:\ICUAudit\dl | Out-Null
# 7-Zip (the build lists the NSIS payload with it)
if(-not (Test-Path 'C:\Program Files\7-Zip\7z.exe')){
    $rel=Invoke-RestMethod 'https://api.github.com/repos/ip7z/7zip/releases/latest' -Headers @{'User-Agent'='icu-audit'}
    $asset=@($rel.assets | Where-Object { $_.name -match '^7z\d+-x64\.exe$' })[0]
    if(-not $asset){ throw '7-Zip release asset not found' }
    Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile C:\ICUAudit\dl\7z.exe
    $sig=Get-AuthenticodeSignature C:\ICUAudit\dl\7z.exe
    "7-Zip installer $($asset.name): signature $($sig.Status) $($sig.SignerCertificate.Subject)"
    if($asset.digest -match '^sha256:(.+)$' -and (Get-FileHash C:\ICUAudit\dl\7z.exe).Hash -ne $Matches[1].ToUpper()){ throw '7-Zip hash mismatch' }
    $p=Start-Process C:\ICUAudit\dl\7z.exe -ArgumentList '/S' -Wait -PassThru
    "7-Zip install exit $($p.ExitCode)"
}
& 'C:\Program Files\7-Zip\7z.exe' | Select-Object -First 2
# .NET 8 runtime (x64) for the Artifact Signing client
if(-not (Get-ChildItem 'C:\Program Files\dotnet\shared\Microsoft.NETCore.App' -Directory -Filter '8.*' -ErrorAction SilentlyContinue)){
    Invoke-WebRequest -UseBasicParsing -Uri 'https://aka.ms/dotnet/8.0/dotnet-runtime-win-x64.exe' -OutFile C:\ICUAudit\dl\dotnet8.exe
    $p=Start-Process C:\ICUAudit\dl\dotnet8.exe -ArgumentList '/install','/quiet','/norestart' -Wait -PassThru
    "dotnet runtime install exit $($p.ExitCode)"
}
Get-ChildItem 'C:\Program Files\dotnet\shared\Microsoft.NETCore.App' | Select-Object -ExpandProperty Name
Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' | Select-Object -ExpandProperty FullName
(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber | Format-List | Out-String)
