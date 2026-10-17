param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][string]$Root,
    [Parameter(Mandatory=$true)][string]$Config,
    [Parameter(Mandatory=$true)][string]$SecretName,
    [string]$UpdaterKeyFile='',
    [switch]$VerifyInstall
)
# sign-windows.ps1 build against the REAL Azure Artifact Signing account. The service principal is read from
# the one-off Secrets Manager secret (instance role) into this process's environment only.
$ErrorActionPreference='Stop'
$base='C:\ICUAudit'
$status="$base\signbuild-$Label-status.txt"; $log="$base\signbuild-$Label.log"
function Stamp($m){ "$((Get-Date).ToUniversalTime().ToString('HH:mm:ss'))Z $m" | Add-Content $status }
Remove-Item $status,$log -ErrorAction SilentlyContinue
try {
    Stamp 'start'
    . "$base\getsecret.ps1"
    Stamp 'secret loaded (lengths only logged by getsecret)'
    $env:ICU_WIN_SIGN_PROVIDER='artifact'
    $env:ICU_ARTIFACT_ENDPOINT='https://eus.codesigning.azure.net/'
    $env:ICU_ARTIFACT_ACCOUNT='nzhaosigning'
    $env:ICU_ARTIFACT_PROFILE='taiku-release-signing'
    if($VerifyInstall){ $env:ICU_WIN_VERIFY_INSTALL='1' }
    if($UpdaterKeyFile){ $env:TAURI_SIGNING_PRIVATE_KEY=(Get-Content $UpdaterKeyFile -Raw).Trim(); $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' }
    $env:CARGO_HOME="$base\cargo"; $env:RUSTUP_HOME="$base\rustup"
    $env:PATH="$base\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;C:\Program Files\7-Zip;"+[Environment]::GetEnvironmentVariable('Path','Machine')
    $env:CARGO_BUILD_JOBS=[string][Math]::Min(8,[Environment]::ProcessorCount)
    $env:CI='true'
    $vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    $vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
    Set-Location "$Root\app"
    Stamp "sign-windows.ps1 build begins (config $Config)"
    $t0=Get-Date; $ErrorActionPreference='Continue'
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$Root\app\scripts\sign-windows.ps1" build --config $Config *> $log
    $code=$LASTEXITCODE
    Stamp "sign-windows.ps1 build exit $code after $([int]((Get-Date)-$t0).TotalSeconds) s"
    $env:AZURE_CLIENT_SECRET=$null; $env:AZURE_CLIENT_ID=$null; $env:AZURE_TENANT_ID=$null; $env:TAURI_SIGNING_PRIVATE_KEY=$null
    if($code -ne 0){ exit $code }
} catch { Stamp "failed: $($_.Exception.Message)"; exit 1 }
