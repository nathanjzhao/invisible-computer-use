<#
Authenticode signing for the Windows build of Copilot (the Windows half of sign-notarize-macos.sh).

  powershell -File app\scripts\sign-windows.ps1 build [tauri build args...]
      Builds the NSIS and MSI installers (`npm run tauri build -- --bundles nsis,msi`) with this
      script as Tauri's bundle.windows.signCommand, then verifies what was signed (installers,
      sidecars, the MSI's files, and by name every file inside the NSIS payload through
      verify-installer; a missing or unsigned one fails the build). Run it where
      a normal Windows `tauri build` works (MSVC developer shell, Git Bash's bash first on PATH).
  powershell -File app\scripts\sign-windows.ps1 sign <file>
      Signs one file. Tauri calls this for app.exe, both sidecars (cua-driver.exe and the
      UIAccess worker cua-driver-uia.exe), the NSIS plugin DLLs, the NSIS uninstaller, the NSIS
      installer and the MSI.
  powershell -File app\scripts\sign-windows.ps1 verify-installer <nsis-installer.exe> [--ledger <file>] [--install] [--require a.exe,b.exe] [--product name]
      By-name check of what ships inside an NSIS installer, which `verify` on the installer
      alone does not give (makensis carries on when a sign command it ran fails: an unsigned
      uninstall.exe shipped that way). It
        - extracts the installer with 7-Zip (7z.exe on PATH, C:\Program Files\7-Zip, or
          ICU_WIN_7Z) and requires every .exe and .dll in it, the NSIS plugin DLLs included, to
          be signed (SHA-256, RFC 3161 timestamp, trusted chain), and the app, the sidecars to be
          there by name;
        - with --ledger (what `build` passes) requires that `sign` recorded a successful,
          verified sign of every sidecar, of the installer, and of the NSIS uninstaller (the
          %TEMP%\nst*.tmp that makensis embeds), and that the sidecars still hold those bytes;
        - with --install (build runs it when ICU_WIN_VERIFY_INSTALL=1) installs the installer
          for the current user and requires uninstall.exe, the app and the sidecars to be
          installed and signed, then uninstalls. That changes the machine, so it is for CI and
          disposable build hosts, and it refuses to run over an existing install.
      The 7-Zip listing is the shipped payload; the uninstaller is not in it (NSIS builds it
      at install time from the installer), hence the ledger and --install.
  powershell -File app\scripts\sign-windows.ps1 verify <file-or-directory>...
      signtool verify /pa /v on each file and on every .exe, .dll and .msi under a directory.
      Fails unless each one chains to a trusted root, has a SHA-256 file digest and carries an
      RFC 3161 timestamp.

Every signature is SHA-256 (/fd sha256) with an RFC 3161 timestamp (/tr, /td sha256).
Secrets are read from environment variables only. The pfx provider is the one place a secret
reaches another process's command line: signtool has no environment or stdin route for the PFX
password (/p), so it is visible to other processes on the machine for the seconds each signtool
runs. Use the store or artifact provider on a shared machine; pfx is for test certificates on a
build machine you own. Nothing here is written outside %TEMP%, and the temporary files are
deleted, including when the script fails.

Provider: ICU_WIN_SIGN_PROVIDER = artifact | pfx | store

  artifact  Azure Artifact Signing (formerly Trusted Signing), through signtool's /dlib.
            ICU_ARTIFACT_ENDPOINT  regional endpoint, e.g. https://eus.codesigning.azure.net/
            ICU_ARTIFACT_ACCOUNT   the Artifact Signing account name
            ICU_ARTIFACT_PROFILE   the certificate profile name
            Authentication is Azure's DefaultAzureCredential, so either a service principal
            (AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET) or a prior `az login` on
            this machine. On an Azure VM, set ICU_ARTIFACT_MANAGED_IDENTITY=1 to
            explicitly use its assigned managed identity. The identity needs the "Artifact Signing Certificate Profile
            Signer" role on the account.
            ICU_ARTIFACT_DLIB      optional path to Azure.CodeSigning.Dlib.dll. Without it the
                                   pinned NuGet client below is downloaded once, hash-checked,
                                   and cached under %LOCALAPPDATA%\icu-signing.
            The client needs the .NET 8 runtime (x64).
  pfx       A certificate file, for test certificates.
            ICU_WIN_PFX            path to the .pfx, or ICU_WIN_PFX_BASE64 with its contents
            ICU_WIN_PFX_PASSWORD   its password (may be empty)
  store     A certificate already in the Windows certificate store: a hardware token or a
            cloud HSM whose client exposes the key through a CSP/KSP (SSL.com eSigner,
            Certum SimplySign, DigiCert KeyLocker).
            ICU_WIN_CERT_SHA1      the certificate's SHA-1 thumbprint
            ICU_WIN_CERT_STORE     user (default) or machine

Optional for every provider:
  ICU_WIN_TIMESTAMP_URL   RFC 3161 server (default: Microsoft's for artifact, DigiCert's otherwise)
  ICU_WIN_SIGNTOOL        path to signtool.exe (default: newest Windows 10/11 SDK x64 signtool)
  ICU_WIN_SIGN_DESCRIPTION  the signature's description (default Copilot)
  ICU_WIN_SIGN_SKIP_VERIFY=1  skip the chain check right after signing (a test root that is
                          not trusted on the build machine); `verify` still checks.
#>
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version 2

# Pinned Artifact Signing client (signtool dlib). Bump both together.
$ArtifactClientPackage = 'Microsoft.ArtifactSigning.Client'
$ArtifactClientVersion = '1.0.128'
$ArtifactClientSha256 = '74bd7d27e6ce1051409c38d9b46bc8df0400ecd643d51ffbf2ac00869061e40b'
$ArtifactDlibName = 'Azure.CodeSigning.Dlib.dll'

function Fail([string]$message) { [Console]::Error.WriteLine("sign-windows: $message"); exit 1 }

function Find-SignTool {
    if ($env:ICU_WIN_SIGNTOOL) {
        if (-not (Test-Path $env:ICU_WIN_SIGNTOOL)) { Fail "ICU_WIN_SIGNTOOL does not exist: $env:ICU_WIN_SIGNTOOL" }
        return $env:ICU_WIN_SIGNTOOL
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = @(Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
        Where-Object { Test-Path $_ })
    if (-not $found.Count) { Fail "signtool.exe not found under $kits; install the Windows SDK signing tools or set ICU_WIN_SIGNTOOL" }
    return $found[0]
}

# One argument as the Windows C runtime (and so signtool) parses it back: backslashes that come
# before a quote are doubled and the quote escaped, and so are backslashes at the end of a
# quoted argument (`two words\` becomes "two words\\", not "two words\").
function Quote-Argument([string]$argument) {
    if ($argument -ne '' -and $argument -notmatch '[\s"]') { return $argument }
    $escaped = $argument -replace '(\\*)"', '$1$1\"'
    $escaped = $escaped -replace '(\\+)$', '$1$1'
    return '"' + $escaped + '"'
}

function Invoke-Tool([string]$exe, [string[]]$arguments) {
    # A Process with both streams read, not `& exe 2>&1`: Windows PowerShell 5 turns native
    # stderr into error records and loses signtool's message (measured: "RemoteException").
    $quoted = $arguments | ForEach-Object { Quote-Argument $_ }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.Arguments = $quoted -join ' '
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stderr = $process.StandardError.ReadToEndAsync()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    if (-not $process.WaitForExit(300000)) {
        & taskkill.exe /PID $process.Id /T /F *> $null
        throw "Signing tool exceeded its five-minute deadline: $([IO.Path]::GetFileName($exe))"
    }
    if (-not $stdout.Wait(5000) -or -not $stderr.Wait(5000)) {
        throw "Signing tool exited without closing its output streams"
    }
    return @{ code = $process.ExitCode; output = ($stdout.Result + $stderr.Result).Trim() }
}

function Get-ArtifactDlib {
    # The client is a mixed-mode DLL hosted on .NET 8. Without the runtime signtool exits 3 and
    # prints nothing (measured), so say what is missing instead.
    # makensis is 32-bit, so the uninstaller's sign command runs under 32-bit PowerShell, where
    # $env:ProgramFiles is "Program Files (x86)". ProgramW6432 is the 64-bit folder either way.
    $programFiles64 = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
    $runtimes = Join-Path $programFiles64 'dotnet\shared\Microsoft.NETCore.App'
    if (-not @(Get-ChildItem $runtimes -Directory -Filter '8.*' -ErrorAction SilentlyContinue).Count) {
        Fail "Artifact Signing needs the .NET 8 runtime (x64): none under $runtimes. Install it, e.g. winget install Microsoft.DotNet.Runtime.8"
    }
    if ($env:ICU_ARTIFACT_DLIB) {
        if (-not (Test-Path $env:ICU_ARTIFACT_DLIB)) { Fail "ICU_ARTIFACT_DLIB does not exist: $env:ICU_ARTIFACT_DLIB" }
        return $env:ICU_ARTIFACT_DLIB
    }
    $cache = Join-Path $env:LOCALAPPDATA "icu-signing\$ArtifactClientPackage.$ArtifactClientVersion"
    $dlib = Join-Path $cache "bin\x64\$ArtifactDlibName"
    if (Test-Path $dlib) { return $dlib }
    New-Item -ItemType Directory -Force $cache | Out-Null
    $nupkg = Join-Path $cache 'client.nupkg'
    $id = $ArtifactClientPackage.ToLowerInvariant()
    Invoke-WebRequest -UseBasicParsing -Uri "https://api.nuget.org/v3-flatcontainer/$id/$ArtifactClientVersion/$id.$ArtifactClientVersion.nupkg" -OutFile $nupkg
    $hash = (Get-FileHash $nupkg -Algorithm SHA256).Hash
    if ($hash -ne $ArtifactClientSha256.ToUpperInvariant()) {
        Remove-Item $nupkg -Force
        Fail "$ArtifactClientPackage $ArtifactClientVersion hash $hash does not match the pinned $ArtifactClientSha256"
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($nupkg, $cache)
    if (-not (Test-Path $dlib)) { Fail "$ArtifactDlibName missing from $ArtifactClientPackage $ArtifactClientVersion" }
    return $dlib
}

function Require([string]$name) {
    $value = [Environment]::GetEnvironmentVariable($name)
    if (-not $value) { Fail "$name is required for ICU_WIN_SIGN_PROVIDER=$env:ICU_WIN_SIGN_PROVIDER" }
    return $value
}

# Returns the provider's signtool arguments and the temporary files to delete afterwards.
function Get-ProviderArguments {
    $temp = @()
    switch ($env:ICU_WIN_SIGN_PROVIDER) {
        'artifact' {
            # The native client creates a temporary user key. A Windows OpenSSH
            # public-key logon can read files and fetch a VM identity token while
            # still lacking the credentials DPAPI needs for that key. Detect this
            # before three opaque SignTool failures or any cloud signing request.
            try {
                Add-Type -AssemblyName System.Security
                $probe = [Text.Encoding]::UTF8.GetBytes('icu-signing-preflight')
                $null = [Security.Cryptography.ProtectedData]::Protect($probe, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
            } catch {
                Fail 'Artifact Signing needs this Windows user''s data-protection context. Authenticate the same authorized signing user with its password (or run in its normal signed-in session), then retry. File access and a managed-identity token alone are insufficient.'
            }
            $metadata = [ordered]@{
                Endpoint = (Require 'ICU_ARTIFACT_ENDPOINT')
                CodeSigningAccountName = (Require 'ICU_ARTIFACT_ACCOUNT')
                CertificateProfileName = (Require 'ICU_ARTIFACT_PROFILE')
            }
            # With neither a service principal nor the Azure CLI, DefaultAzureCredential falls through
            # to the managed-identity probe, which hung signtool for minutes on EC2 (measured).
            $servicePrincipal = $env:AZURE_TENANT_ID -and $env:AZURE_CLIENT_ID -and $env:AZURE_CLIENT_SECRET
            $managedIdentity = $env:ICU_ARTIFACT_MANAGED_IDENTITY -eq '1'
            if ($managedIdentity) {
                $metadata.ExcludeCredentials = @('EnvironmentCredential','WorkloadIdentityCredential','SharedTokenCacheCredential','VisualStudioCredential','VisualStudioCodeCredential','AzureCliCredential','AzurePowerShellCredential','AzureDeveloperCliCredential','InteractiveBrowserCredential')
                try {
                    $instance = Invoke-RestMethod -Headers @{Metadata='true'} -Uri 'http://169.254.169.254/metadata/instance?api-version=2021-02-01' -TimeoutSec 3
                    if (-not $instance.compute.vmId) { throw 'Azure VM identity metadata missing' }
                } catch { Fail 'Managed identity signing requires an Azure VM with its own assigned signing role' }
            }
            if (-not $servicePrincipal -and -not $managedIdentity -and -not (Get-Command az -ErrorAction SilentlyContinue)) {
                Fail 'Artifact Signing needs credentials: set AZURE_TENANT_ID, AZURE_CLIENT_ID and AZURE_CLIENT_SECRET, or install the Azure CLI and run az login'
            }
            # Resolve the client first: its failures exit the script, and the metadata file must not exist yet.
            $dlib = Get-ArtifactDlib
            $file = Join-Path $env:TEMP ("icu-artifact-" + [guid]::NewGuid().ToString('n') + '.json')
            [IO.File]::WriteAllText($file, ($metadata | ConvertTo-Json))
            $temp += $file
            return @{ args = @('/dlib', $dlib, '/dmdf', $file); temp = $temp; timestamp = 'http://timestamp.acs.microsoft.com' }
        }
        'pfx' {
            $pfx = $env:ICU_WIN_PFX
            if (-not $pfx) {
                $encoded = Require 'ICU_WIN_PFX_BASE64'
                $pfx = Join-Path $env:TEMP ("icu-" + [guid]::NewGuid().ToString('n') + '.pfx')
                [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($encoded))
                $temp += $pfx
            } elseif (-not (Test-Path $pfx)) { Fail "ICU_WIN_PFX does not exist: $pfx" }
            $arguments = @('/f', $pfx)
            if ($null -ne $env:ICU_WIN_PFX_PASSWORD -and $env:ICU_WIN_PFX_PASSWORD -ne '') { $arguments += @('/p', $env:ICU_WIN_PFX_PASSWORD) }
            return @{ args = $arguments; temp = $temp; timestamp = 'http://timestamp.digicert.com' }
        }
        'store' {
            $arguments = @('/sha1', (Require 'ICU_WIN_CERT_SHA1'))
            if ($env:ICU_WIN_CERT_STORE -eq 'machine') { $arguments += '/sm' }
            return @{ args = $arguments; temp = $temp; timestamp = 'http://timestamp.digicert.com' }
        }
        default { Fail "set ICU_WIN_SIGN_PROVIDER to artifact, pfx or store (got '$env:ICU_WIN_SIGN_PROVIDER')" }
    }
}

# A signature alone cannot give UIAccess to a worker whose build omitted its manifest.
function Test-WorkerManifest([string]$path) {
    if ((Split-Path -Leaf $path) -notmatch '^cua-driver-uia(?:-.*)?\.exe$') { return @() }
    $mt=Join-Path (Split-Path -Parent $script:SignTool) 'mt.exe'
    if (-not (Test-Path $mt)) { return @('Windows SDK mt.exe is required to verify the UIAccess manifest') }
    $manifest=Join-Path $env:TEMP ('icu-uia-manifest-'+[guid]::NewGuid().ToString('n')+'.xml')
    try {
        $result=Invoke-Tool $mt @('-nologo',"-inputresource:$path;#1","-out:$manifest")
        if ($result.code -ne 0 -or -not (Test-Path $manifest)) { return @('UIAccess worker has no readable embedded manifest') }
        [xml]$xml=Get-Content $manifest -Raw
        $levels=@($xml.GetElementsByTagName('requestedExecutionLevel'))
        if ($levels.Count -ne 1 -or $levels[0].GetAttribute('uiAccess') -ne 'true' -or $levels[0].GetAttribute('level') -ne 'asInvoker') {
            return @('UIAccess worker manifest must request asInvoker and uiAccess=true')
        }
        return @()
    } catch { return @('UIAccess worker manifest could not be verified') }
    finally { Remove-Item $manifest -Force -ErrorAction SilentlyContinue }
}

# signtool verify /pa /v, plus the two properties a passing chain check does not imply.
function Test-Signed([string]$path) {
    $manifestProblems = @(Test-WorkerManifest $path)
    $result = Invoke-Tool $script:SignTool @('verify', '/pa', '/v', $path)
    $problems = @($manifestProblems)
    if ($result.code -ne 0) { $problems += "signtool verify exit $($result.code)" }
    if ($result.output -notmatch 'Hash of file \(sha256\)') { $problems += 'file digest is not SHA-256' }
    if ($result.output -notmatch 'The signature is timestamped') { $problems += 'no timestamp' }
    return @{ ok = ($problems.Count -eq 0); problems = $problems; output = $result.output }
}

function Invoke-Sign([string]$path) {
    $manifestProblems = @(Test-WorkerManifest $path)
    if ($manifestProblems.Count) { Fail ($manifestProblems -join "; ") }
    if (-not (Test-Path $path -PathType Leaf)) { Fail "no such file: $path" }
    $provider = Get-ProviderArguments
    $timestamp = if ($env:ICU_WIN_TIMESTAMP_URL) { $env:ICU_WIN_TIMESTAMP_URL } else { $provider.timestamp }
    $description = if ($env:ICU_WIN_SIGN_DESCRIPTION) { $env:ICU_WIN_SIGN_DESCRIPTION } else { 'Copilot' }
    $arguments = @('sign', '/v', '/fd', 'sha256', '/tr', $timestamp, '/td', 'sha256', '/d', $description) + $provider.args + @($path)
    try {
        # Timestamp servers drop requests now and then; a failed attempt leaves the file unsigned.
        for ($attempt = 1; ; $attempt++) {
            $result = Invoke-Tool $script:SignTool $arguments
            if ($result.code -eq 0) { break }
            if ($attempt -ge 3) {
                # The output names the file and the error; the arguments (which can hold a password) are not printed.
                Fail ("signtool sign failed for ${path}:`n" + $result.output)
            }
            Start-Sleep -Seconds (5 * $attempt)
        }
    } finally {
        foreach ($file in $provider.temp) { Remove-Item $file -Force -ErrorAction SilentlyContinue }
    }
    if ($env:ICU_WIN_SIGN_SKIP_VERIFY -ne '1') {
        $check = Test-Signed $path
        if (-not $check.ok) { Fail ("signed but verification failed for ${path}: " + ($check.problems -join '; ') + "`n" + $check.output) }
    }
    # What this call signed, for `build`'s by-name check (see Test-SignLedger). Written only after
    # the sign and its verification succeeded, so a sign that failed leaves no entry.
    if ($env:ICU_WIN_SIGN_LEDGER) {
        $verified = if ($env:ICU_WIN_SIGN_SKIP_VERIFY -ne '1') { 'verified' } else { 'unverified' }
        $line = '{0}|{1}|{2}' -f $path, (Get-FileHash $path -Algorithm SHA256).Hash, $verified
        [IO.File]::AppendAllText($env:ICU_WIN_SIGN_LEDGER, $line + [Environment]::NewLine)
    }
    Write-Output "signed $path"
    # Tauri signs the NSIS plugin DLLs it lists, not every plugin the installer embeds: UserInfo.dll
    # shipped unsigned in a signed installer (found by verify-installer, 2026-10-04). makensis packs
    # whatever sits in that folder when it runs, so sign the unsigned DLLs beside a signed plugin too.
    if (-not $script:SigningSiblings -and $path -match '\\nsis\\[^\\]+\\Plugins\\.+\.dll$') {
        $script:SigningSiblings = $true
        try {
            foreach ($sibling in @(Get-ChildItem (Split-Path -Parent $path) -Filter *.dll -File)) {
                if ($sibling.FullName -ne $path -and (Get-AuthenticodeSignature $sibling.FullName).Status -eq 'NotSigned') { Invoke-Sign $sibling.FullName }
            }
        } finally { $script:SigningSiblings = $false }
    }
}

function Get-SignableFiles([string[]]$targets) {
    foreach ($target in $targets) {
        if (Test-Path $target -PathType Container) {
            Get-ChildItem $target -Recurse -File -Include *.exe, *.dll, *.msi | ForEach-Object { $_.FullName }
        } elseif (Test-Path $target -PathType Leaf) {
            (Resolve-Path $target).Path
        } else { Fail "no such file or directory: $target" }
    }
}

function Invoke-Verify([string[]]$targets) {
    $files = @(Get-SignableFiles $targets)
    if (-not $files.Count) { Fail "nothing to verify in $($targets -join ', ')" }
    $failed = 0
    foreach ($file in $files) {
        $check = Test-Signed $file
        if ($check.ok) {
            $signature = Get-AuthenticodeSignature $file
            Write-Output ("ok    {0}  signer={1}  timestamp={2}" -f $file, $signature.SignerCertificate.Subject, $signature.TimeStamperCertificate.Subject)
        } else {
            $failed++
            Write-Output ("FAIL  {0}  {1}" -f $file, ($check.problems -join '; '))
        }
    }
    if ($failed) { Fail "$failed of $($files.Count) files failed verification" }
    Write-Output "verified $($files.Count) files"
}

# The app's main executable name: Tauri's mainBinaryName, else the Cargo package name.
function Get-MainExeName([string]$app) {
    $tauri = Join-Path $app 'src-tauri'
    $conf = Get-Content (Join-Path $tauri 'tauri.conf.json') -Raw | ConvertFrom-Json
    if ($conf.PSObject.Properties['mainBinaryName'] -and $conf.mainBinaryName) { return "$($conf.mainBinaryName).exe" }
    $match = [regex]::Match((Get-Content (Join-Path $tauri 'Cargo.toml') -Raw), '(?ms)^\[package\].*?^name\s*=\s*"([^"]+)"')
    if (-not $match.Success) { Fail "no package name in $tauri\Cargo.toml" }
    return "$($match.Groups[1].Value).exe"
}

# Sidecars are bundled as <name>-<target triple>.exe and installed as <name>.exe.
function Get-InstalledSidecarNames([string]$app) {
    foreach ($file in @(Get-ChildItem (Join-Path $app 'src-tauri\binaries\*.exe'))) {
        ($file.Name -replace '-(x86_64|aarch64|i686)-pc-windows-msvc\.exe$', '.exe')
    }
}

function Get-SevenZip {
    $candidates = @($env:ICU_WIN_7Z, (Join-Path $env:ProgramW6432 '7-Zip\7z.exe'), (Join-Path ${env:ProgramFiles(x86)} '7-Zip\7z.exe'))
    $onPath = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += $onPath.Source }
    foreach ($candidate in $candidates) { if ($candidate -and (Test-Path $candidate -PathType Leaf)) { return $candidate } }
    Fail 'verify-installer needs 7-Zip to read inside the NSIS installer: install it (winget install 7zip.7zip) or set ICU_WIN_7Z to 7z.exe'
}

# Every .exe and .dll inside the NSIS installer, by name: the NSIS plugin DLLs ($PLUGINSDIR) and the
# app files. Returns the problems found.
function Test-NsisPayload([string]$installer, [string[]]$requiredNames) {
    if (-not (Test-Path $installer -PathType Leaf)) { Fail "no such installer: $installer" }
    $sevenZip = Get-SevenZip
    $dir = Join-Path $env:TEMP ('icu-nsis-payload-' + [guid]::NewGuid().ToString('n'))
    $problems = @()
    try {
        $result = Invoke-Tool $sevenZip @('x', '-y', "-o$dir", $installer)
        if ($result.code -ne 0) { return @("7-Zip could not extract the installer (exit $($result.code)): " + $result.output) }
        $files = @(Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in '.exe', '.dll' })
        $names = @($files | ForEach-Object { $_.Name })
        foreach ($name in $requiredNames) {
            if ($names -notcontains $name) { $problems += "$name is not in the installer" }
        }
        foreach ($file in $files) {
            $relative = $file.FullName.Substring($dir.Length + 1)
            $check = Test-Signed $file.FullName
            if ($check.ok) { Write-Host ("ok    payload {0}" -f $relative) }
            else { $problems += ("{0} in the installer: {1}" -f $relative, ($check.problems -join '; ')); Write-Host ("FAIL  payload {0}  {1}" -f $relative, ($check.problems -join '; ')) }
        }
    } finally { Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue }
    return $problems
}

# Install the NSIS installer for the current user and check what landed, uninstall.exe included.
# /D is ignored by Tauri's installer, so the install directory is read from the uninstall key.
function Test-NsisInstall([string]$installer, [string[]]$requiredNames, [string]$productName) {
    $key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productName"
    if (Test-Path $key) { return @("$productName is already installed for this user ($key); remove it first, this check installs and uninstalls") }
    $problems = @(); $dir = $null
    try {
        $process = Start-Process -FilePath $installer -ArgumentList @('/S', '/CurrentUser') -Wait -PassThru
        if ($process.ExitCode -ne 0) { return @("installer exit $($process.ExitCode)") }
        if (-not (Test-Path $key)) { return @("the install left no uninstall key at $key") }
        $uninstallString = (Get-ItemProperty $key).UninstallString
        $dir = Split-Path -Parent ($uninstallString.Trim('"'))
        # A 32-bit installer run as SYSTEM records a path under System32 that only a 32-bit process
        # resolves (WOW64 redirection); this process may be 64-bit.
        if (-not (Test-Path $dir)) { $dir = $dir -replace '(?i)\\system32\\', '\SysWOW64\' }
        $files = @(Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in '.exe', '.dll' })
        $names = @($files | ForEach-Object { $_.Name })
        foreach ($name in $requiredNames) {
            if ($names -notcontains $name) { $problems += "$name is not in the install" }
        }
        foreach ($file in $files) {
            $relative = $file.FullName.Substring($dir.Length + 1)
            $check = Test-Signed $file.FullName
            if ($check.ok) { Write-Host ("ok    installed {0}" -f $relative) }
            else { $problems += ("{0} installed: {1}" -f $relative, ($check.problems -join '; ')); Write-Host ("FAIL  installed {0}  {1}" -f $relative, ($check.problems -join '; ')) }
        }
    } finally {
        if ($dir -and (Test-Path (Join-Path $dir 'uninstall.exe'))) {
            # _?= keeps the uninstaller from copying itself away, so -Wait waits for the real work.
            Start-Process -FilePath (Join-Path $dir 'uninstall.exe') -ArgumentList @('/S', "_?=$dir") -Wait | Out-Null
            Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    return $problems
}

# By-name check of the ledger `sign` wrote during the build: every sidecar, the installer and the
# NSIS uninstaller (makensis signs it from %TEMP%\nst*.tmp, and goes on when that fails) has a
# verified entry, and the sidecars and the installer still hold the bytes that were signed.
function Test-SignLedger([string]$ledger, [string[]]$mustExist, [string[]]$mustMatchFile) {
    $entries = @()
    if (Test-Path $ledger) {
        $entries = @(Get-Content $ledger | Where-Object { $_ } | ForEach-Object {
            $parts = $_ -split '\|'
            [pscustomobject]@{ path = $parts[0]; leaf = (Split-Path -Leaf $parts[0]); sha = $parts[1]; state = $parts[2] } })
    }
    $problems = @()
    # With ICU_WIN_SIGN_SKIP_VERIFY=1 the ledger records the sign, not its verification; the payload
    # check is the verification then.
    $pool = if ($env:ICU_WIN_SIGN_SKIP_VERIFY -eq '1') { $entries } else { @($entries | Where-Object { $_.state -eq 'verified' }) }
    foreach ($name in $mustExist) {
        if (-not @($pool | Where-Object { $_.leaf -eq $name }).Count) { $problems += "no verified sign recorded for $name" }
    }
    if (-not @($pool | Where-Object { $_.leaf -match '^nst[0-9A-Za-z]+\.tmp$' }).Count) {
        $problems += 'no verified sign recorded for the NSIS uninstaller (nst*.tmp)'
    }
    foreach ($file in $mustMatchFile) {
        $leaf = Split-Path -Leaf $file
        $hash = (Get-FileHash $file -Algorithm SHA256).Hash
        $last = @($entries | Where-Object { $_.leaf -eq $leaf } | Select-Object -Last 1)
        if ($last.Count -and $last[0].sha -ne $hash) { $problems += "$leaf changed after it was signed" }
    }
    return $problems
}

# verify-installer <installer> [--ledger <file>] [--install] [--require name,name]
# --require replaces the names derived from this repo (for a test installer).
function Invoke-VerifyInstaller([string[]]$arguments) {
    $usage = 'usage: sign-windows.ps1 verify-installer <installer> [--ledger <file>] [--install] [--require a.exe,b.exe] [--product name]'
    if (-not $arguments.Count) { Fail $usage }
    $installer = $arguments[0]; $ledger = $null; $require = $null; $install = $false; $product = $null
    for ($i = 1; $i -lt $arguments.Count; $i++) {
        switch ($arguments[$i]) {
            '--install' { $install = $true }
            '--ledger' { $i++; if ($i -ge $arguments.Count) { Fail $usage }; $ledger = $arguments[$i] }
            '--require' { $i++; if ($i -ge $arguments.Count) { Fail $usage }; $require = @($arguments[$i] -split ',') }
            '--product' { $i++; if ($i -ge $arguments.Count) { Fail $usage }; $product = $arguments[$i] }
            default { Fail "unknown option $($arguments[$i])`n$usage" }
        }
    }
    $app = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
    $payloadNames = $require
    if ($null -eq $payloadNames) { $payloadNames = @((Get-MainExeName $app)) + @(Get-InstalledSidecarNames $app) }
    $problems = @(Test-NsisPayload $installer $payloadNames)
    if ($ledger) {
        $sidecars = @(Get-ChildItem (Join-Path $app 'src-tauri\binaries\*.exe') -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
        $problems += Test-SignLedger $ledger (@($sidecars | ForEach-Object { Split-Path -Leaf $_ }) + @((Split-Path -Leaf $installer))) ($sidecars + @($installer))
    }
    if ($install) {
        if (-not $product) { $product = (Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).productName }
        $problems += Test-NsisInstall $installer (@('uninstall.exe') + @($payloadNames | Where-Object { $_ -like '*.exe' })) $product
    }
    if ($problems.Count) { Fail ("NSIS payload check failed for ${installer}:`n  " + ($problems -join "`n  ")) }
    Write-Output "verified NSIS payload of $installer by name"
}

function Invoke-Build([string[]]$tauriArgs) {
    if (-not $env:ICU_WIN_SIGN_PROVIDER) { Fail 'set ICU_WIN_SIGN_PROVIDER (artifact, pfx or store) before building' }
    $self = $PSCommandPath
    $app = Split-Path -Parent (Split-Path -Parent $self)
    $config = @{ bundle = @{ windows = @{ signCommand = @{
        cmd = 'powershell.exe'
        args = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $self, 'sign', '%1') } } } }
    # Updater artifacts need the minisign key (AUTO_UPDATE.md); skip them when it is not set.
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) { $config.bundle.createUpdaterArtifacts = $false }
    $configFile = Join-Path $env:TEMP ("icu-sign-config-" + [guid]::NewGuid().ToString('n') + '.json')
    [IO.File]::WriteAllText($configFile, ($config | ConvertTo-Json -Depth 8))
    $bundles = if ($tauriArgs -contains '--bundles' -or $tauriArgs -contains '-b') { @() } else { @('--bundles', 'nsis,msi') }
    # Fail before a long build if the provider is not usable.
    Get-ProviderArguments | ForEach-Object { foreach ($file in $_.temp) { Remove-Item $file -Force -ErrorAction SilentlyContinue } }
    # Every `sign` call (makensis and Tauri run them as children) appends here; see Test-SignLedger.
    $ledger = Join-Path $env:TEMP ("icu-sign-ledger-" + [guid]::NewGuid().ToString('n') + '.txt')
    $env:ICU_WIN_SIGN_LEDGER = $ledger
    Push-Location $app
    try {
        $previous = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        # An array, so PowerShell passes the bare `--` through to npm instead of eating it.
        $npmArgs = @('run', 'tauri', 'build', '--') + $bundles + @('--config', $configFile) + $tauriArgs
        & npm.cmd @npmArgs
        $code = $LASTEXITCODE
        $ErrorActionPreference = $previous
        if ($code -ne 0) { Fail "tauri build failed (exit $code)" }
    } finally {
        Pop-Location
        Remove-Item $configFile -Force -ErrorAction SilentlyContinue
    }

    $bundle = Join-Path $app 'src-tauri\target\release\bundle'
    $installers = @(Get-ChildItem (Join-Path $bundle 'nsis\*.exe'), (Join-Path $bundle 'msi\*.msi') -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    $sidecars = @(Get-ChildItem (Join-Path $app 'src-tauri\binaries\*.exe') | ForEach-Object { $_.FullName })
    Invoke-Verify ($installers + $sidecars)
    # By name, inside the NSIS payload: a failed sign that makensis ignored shows up here.
    try {
        foreach ($nsis in @($installers | Where-Object { $_ -like '*.exe' })) {
            $verifyArgs = @($nsis, '--ledger', $ledger)
            if ($env:ICU_WIN_VERIFY_INSTALL -eq '1') { $verifyArgs += '--install' }
            Invoke-VerifyInstaller $verifyArgs
        }
    } finally { Remove-Item $ledger -Force -ErrorAction SilentlyContinue; Remove-Item Env:\ICU_WIN_SIGN_LEDGER -ErrorAction SilentlyContinue }
    # app.exe is restored unsigned in target\release after packaging; what ships is inside the
    # installers. An MSI admin image exposes its files, so check those too.
    foreach ($msi in @($installers | Where-Object { $_ -like '*.msi' })) {
        $image = Join-Path $env:TEMP ("icu-msi-" + [guid]::NewGuid().ToString('n'))
        $process = Start-Process msiexec.exe -ArgumentList @('/a', "`"$msi`"", '/qn', "TARGETDIR=`"$image`"") -Wait -PassThru
        if ($process.ExitCode -ne 0) { Fail "msiexec /a failed for $msi (exit $($process.ExitCode))" }
        # The admin image also holds msiexec's own rewritten (unsigned) copy of the MSI; check only its PEs.
        try { Invoke-Verify @(Get-ChildItem $image -Recurse -File -Include *.exe, *.dll | ForEach-Object { $_.FullName }) } finally { Remove-Item $image -Recurse -Force -ErrorAction SilentlyContinue }
    }
    Write-Output 'Windows build signed and verified:'
    $installers | ForEach-Object { Write-Output "  $_" }
}

if ($args.Count -lt 1) { Fail 'usage: sign-windows.ps1 build [tauri args...] | sign <file> | verify <file-or-dir>... | verify-installer <installer>' }
$script:SignTool = Find-SignTool
$script:SigningSiblings = $false
$rest = @($args | Select-Object -Skip 1)
switch ($args[0]) {
    'build' { Invoke-Build $rest }
    'sign' { if ($rest.Count -ne 1) { Fail 'usage: sign-windows.ps1 sign <file>' }; Invoke-Sign $rest[0] }
    'verify-installer' { Invoke-VerifyInstaller $rest }
    'verify' { if (-not $rest.Count) { Fail 'usage: sign-windows.ps1 verify <file-or-dir>...' }; Invoke-Verify $rest }
    default { Fail "unknown mode '$($args[0])'" }
}
