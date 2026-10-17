param(
    [Parameter(Mandatory=$true)][string]$RunId,
    [Parameter(Mandatory=$true)][string]$AuditPassword,
    [Parameter(Mandatory=$true)][datetime]$ExpiresAt
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = 'C:\ICUAudit'
New-Item -ItemType Directory -Force $root | Out-Null
$artifacts = @()
function Status([string]$stage, [string]$detail = '') {
    @{run=$RunId; stage=$stage; detail=$detail; time=(Get-Date).ToUniversalTime().ToString('o')} |
        ConvertTo-Json | Set-Content "$root\bootstrap-status.json" -Encoding UTF8
}
function Download([string]$url, [string]$name, [string]$signer = '') {
    $path = Join-Path $root $name
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $path
    if ($signer) {
        $signature = Get-AuthenticodeSignature $path
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike "*$signer*") {
            throw "Unexpected installer signature for $name"
        }
    }
    $script:artifacts += @{name=$name; sha256=(Get-FileHash $path -Algorithm SHA256).Hash}
    return $path
}
function Install([string]$path, [string[]]$arguments) {
    $process = Start-Process -FilePath $path -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -notin @(0,3010)) { throw "Installer failed: $([IO.Path]::GetFileName($path)); code $($process.ExitCode)" }
}
try {
    # Install expiry before downloading tooling. EC2 must be configured to
    # terminate, rather than stop, when Windows initiates shutdown.
    $expiry = New-ScheduledTaskAction -Execute 'shutdown.exe' -Argument '/s /f /t 0'
    $trigger = New-ScheduledTaskTrigger -Once -At $ExpiresAt.ToLocalTime()
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -WakeToRun
    Register-ScheduledTask -TaskName 'ICUAuditExpiry' -Action $expiry -Trigger $trigger -Settings $settings -User 'SYSTEM' -RunLevel Highest -Force | Out-Null
    Status 'expiry-installed'

    # The interactive acceptance user is deliberately not an administrator.
    $password = ConvertTo-SecureString $AuditPassword -AsPlainText -Force
    if (Get-LocalUser -Name 'ICUAudit' -ErrorAction SilentlyContinue) {
        Set-LocalUser -Name 'ICUAudit' -Password $password
    } else {
        New-LocalUser -Name 'ICUAudit' -Password $password -PasswordNeverExpires -AccountNeverExpires | Out-Null
    }
    Add-LocalGroupMember -Group 'Users' -Member 'ICUAudit' -ErrorAction SilentlyContinue
    if (@(Get-LocalGroupMember 'Administrators' | Where-Object { $_.Name -like '*\ICUAudit' }).Count) {
        throw 'Acceptance account must not be an administrator'
    }
    $logon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
    Set-ItemProperty $logon AutoAdminLogon '1'
    Set-ItemProperty $logon DefaultUserName 'ICUAudit'
    Set-ItemProperty $logon DefaultDomainName $env:COMPUTERNAME
    Set-ItemProperty $logon DefaultPassword $AuditPassword
    # The key exists on current images, and New-Item -Force on it is refused
    # there ("Attempted to perform an unauthorized operation").
    $policies='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
    if (-not (Test-Path $policies)) { New-Item $policies | Out-Null }
    Set-ItemProperty $policies EnableFirstLogonAnimation 0
    $AuditPassword = $null; $password = $null

    # Client images show a privacy OOBE after auto-logon unless provisioned here.
    # That screen is not a usable desktop for the standard-user acceptance session.
    $oobe='HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE'
    New-Item $oobe -Force | Out-Null
    New-ItemProperty $oobe -Name DisablePrivacyExperience -Value 1 -PropertyType DWord -Force | Out-Null
    $edge='HKLM:\SOFTWARE\Policies\Microsoft\Edge'
    New-Item $edge -Force | Out-Null
    New-ItemProperty $edge -Name HideFirstRunExperience -Value 1 -PropertyType DWord -Force | Out-Null

    Status 'installing-tooling'
    $releases = Invoke-RestMethod 'https://nodejs.org/dist/index.json'
    $node = @($releases | Where-Object { $_.version -like 'v24.*' -and $_.lts })[0]
    if (-not $node) { throw 'Node LTS release metadata unavailable' }
    $nodeInstaller = Download "https://nodejs.org/dist/$($node.version)/node-$($node.version)-x64.msi" 'node.msi' 'OpenJS Foundation'
    Install 'msiexec.exe' @('/i',"`"$nodeInstaller`"",'/qn','/norestart')

    $git = Invoke-RestMethod 'https://api.github.com/repos/git-for-windows/git/releases/latest'
    $gitAsset = @($git.assets | Where-Object { $_.name -match '^Git-.*-64-bit\.exe$' })[0]
    if (-not $gitAsset) { throw 'Git installer metadata unavailable' }
    if ($gitAsset.digest -notmatch '^sha256:[a-f0-9]{64}$') { throw 'Git release checksum unavailable' }
    $gitInstaller = Download $gitAsset.browser_download_url 'git.exe' 'Johannes Schindelin'
    if ((Get-FileHash $gitInstaller -Algorithm SHA256).Hash.ToLowerInvariant() -ne $gitAsset.digest.Substring(7)) {
        throw 'Git release checksum mismatch'
    }
    Install $gitInstaller @('/VERYSILENT','/NORESTART','/NOCANCEL','/SP-','/DIR="C:\Program Files\Git"')
    # Git for Windows ships core.autocrlf=true; `git apply` of the driver patch then writes CRLF into the vendor tree
    # and the build is not byte-identical to one made from LF files. build.ps1 and apply-and-build.sh also pass
    # -c core.autocrlf=false, so a host bootstrapped before this line is covered too.
    & 'C:\Program Files\Git\cmd\git.exe' config --system core.autocrlf false

    $vs = Download 'https://aka.ms/vs/17/release/vs_BuildTools.exe' 'vs-buildtools.exe' 'Microsoft Corporation'
    Install $vs @('--quiet','--wait','--norestart','--nocache','--add','Microsoft.VisualStudio.Workload.VCTools','--add','Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre','--includeRecommended')

    $webview = Download 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' 'webview2.exe' 'Microsoft Corporation'
    Install $webview @('/silent','/install')

    $env:CARGO_HOME = "$root\cargo"
    $env:RUSTUP_HOME = "$root\rustup"
    [Environment]::SetEnvironmentVariable('CARGO_HOME',$env:CARGO_HOME,'Machine')
    [Environment]::SetEnvironmentVariable('RUSTUP_HOME',$env:RUSTUP_HOME,'Machine')
    $rustUrl = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
    $rust = Download $rustUrl 'rustup-init.exe'
    $expected = (Invoke-RestMethod "$rustUrl.sha256").ToString().Split(' ')[0].Trim().ToLowerInvariant()
    if ((Get-FileHash $rust -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'Rustup checksum mismatch' }
    Install $rust @('-y','--profile','minimal','--default-toolchain','1.97.1','--no-modify-path')

    $artifacts | ConvertTo-Json | Set-Content "$root\tooling-hashes.json" -Encoding UTF8
    Status 'tooling-ready'
    Restart-Computer -Force
} catch {
    # Never copy the bootstrap's credential-bearing command line into logs.
    Status 'failed' ($_.Exception.Message -replace 'https?://\S+','<url>')
    exit 1
}
