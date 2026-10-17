param(
    [string]$Revision,
    [string]$SourceUrl,
    [string]$SourceSha256,
    [string]$VendorUrl,
    [string]$VendorSha256,
    [string]$ObserverUrl,
    [string]$ObserverSha256,
    [switch]$StorageManagedIdentity,
    # JSON with the five values above. A presigned URL passed on a scheduled
    # task's `powershell -File` command line keeps its quotes and fails to
    # parse ("Invalid URI: The hostname could not be parsed"); a file does not.
    [string]$ParamsFile
)
$ErrorActionPreference='Stop'
if($ParamsFile){
    $values=Get-Content $ParamsFile -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach($name in @('Revision','SourceUrl','SourceSha256','VendorUrl','VendorSha256')){
        if(-not $values.$name){throw "$name missing from $ParamsFile"}
        Set-Variable -Name $name -Value ([string]$values.$name)
    }
    $ObserverUrl=[string]$values.ObserverUrl
    $ObserverSha256=[string]$values.ObserverSha256
    $StorageManagedIdentity=[bool]$values.StorageManagedIdentity
}
if([bool]$ObserverUrl -ne [bool]$ObserverSha256){throw 'ObserverUrl and ObserverSha256 are required together'}
foreach($name in @('Revision','SourceUrl','SourceSha256','VendorUrl','VendorSha256')){
    if(-not (Get-Variable -Name $name -ValueOnly)){throw "$name is required"}
}
$ProgressPreference='SilentlyContinue'
$base='C:\ICUAudit'
$root=Join-Path $base "source-$Revision"
function Status([string]$stage,[string]$detail='') {
    @{revision=$Revision;stage=$stage;detail=$detail;time=(Get-Date).ToUniversalTime().ToString('o')} |
        ConvertTo-Json | Set-Content "$base\build-status.json" -Encoding UTF8
}
function Run([string]$program,[string[]]$arguments,[string]$log) {
    $executable=(Get-Command $program -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $previousPreference=$ErrorActionPreference
    try {
        # Windows PowerShell 5 wraps native stderr as ErrorRecords. Warnings must
        # reach the log without bypassing the authoritative native exit status.
        $ErrorActionPreference='Continue'
        $global:LASTEXITCODE=$null
        & $executable @arguments *> $log
        $nativeExitCode=$LASTEXITCODE
    } finally {
        $ErrorActionPreference=$previousPreference
    }
    if ($null -eq $nativeExitCode -or $nativeExitCode -ne 0) { throw "Build command failed: $([IO.Path]::GetFileName($program)); code $nativeExitCode; see $([IO.Path]::GetFileName($log))" }
}
function Fetch([string]$url,[string]$path,[string]$expected) {
    $headers=@{}
    if($StorageManagedIdentity){
        if($url -notmatch '^https://[a-z0-9]+\.blob\.core\.windows\.net/'){throw 'Managed identity download requires an Azure blob URL'}
        $token=Invoke-RestMethod -Headers @{Metadata='true'} -Uri 'http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https%3A%2F%2Fstorage.azure.com%2F'
        $headers=@{Authorization="Bearer $($token.access_token)";'x-ms-version'='2023-11-03'}
    }
    Invoke-WebRequest -UseBasicParsing -Uri $url -Headers $headers -OutFile $path
    if ((Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected.ToLowerInvariant()) {
        throw "Source hash mismatch: $([IO.Path]::GetFileName($path))"
    }
}
try {
    if (Test-Path $root) { throw 'Use a fresh source directory for each build candidate' }
    New-Item -ItemType Directory $root | Out-Null
    Status 'downloading-source'
    Fetch $SourceUrl "$base\source.tar.gz" $SourceSha256
    Fetch $VendorUrl "$base\vendor.tar.gz" $VendorSha256
    & tar.exe -xzf "$base\source.tar.gz" -C $root
    if ($LASTEXITCODE) { throw 'Source extraction failed' }
    New-Item -ItemType Directory "$root\vendor-cua" | Out-Null
    & tar.exe -xzf "$base\vendor.tar.gz" -C "$root\vendor-cua"
    if ($LASTEXITCODE) { throw 'Vendor extraction failed' }
    if($ObserverUrl){
        Fetch $ObserverUrl "$base\observer.tar.gz" $ObserverSha256
        New-Item -ItemType Directory "$root\observer" | Out-Null
        & tar.exe -xzf "$base\observer.tar.gz" -C "$root\observer"
        if($LASTEXITCODE){throw 'Observer extraction failed'}
    }

    $env:CARGO_HOME="$base\cargo"
    $env:RUSTUP_HOME="$base\rustup"
    $env:PATH="$base\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
    $env:CARGO_BUILD_JOBS=[string][Math]::Min(4,[Environment]::ProcessorCount)
    $env:CI='true'
    $env:RUSTC_WRAPPER=''
    $vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    $vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw 'MSVC toolchain not installed' }
    Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
    Set-Location $root
    Run 'git.exe' @('-c','core.autocrlf=false','-C',"$root\vendor-cua",'apply',"$root\impl\native\cua-driver.patch") "$base\patch.log"
    Run 'git.exe' @('-c','core.autocrlf=false','-C',"$root\vendor-cua",'apply','--check','--reverse',"$root\impl\native\cua-driver.patch") "$base\patch-reverse.log"
    Run 'rustc.exe' @('--version') "$base\rust-version.log"
    if($ObserverUrl){
        Status 'building-observer'
        Run 'cargo.exe' @('build','--release','--locked','--manifest-path',"$root\observer\taiku-observer\Cargo.toml") "$base\observer-build.log"
        New-Item -ItemType Directory -Force "$root\app\src-tauri\binaries" | Out-Null
        Copy-Item "$root\observer\taiku-observer\target\release\taiku-observer.exe" "$root\app\src-tauri\binaries\taiku-observer-x86_64-pc-windows-msvc.exe"
    }
    Set-Location "$root\app"
    Status 'installing-node-dependencies'
    Run 'npm.cmd' @('ci','--no-audit','--no-fund') "$base\npm.log"
    Status 'building-package'
    # SYSTEM's cache is under System32. NSIS's 32-bit launcher cannot reliably
    # launch its child there because of filesystem redirection.
    $bundleConfig=Join-Path $base 'bundle-tools.json'
    $overlay=@{bundle=@{useLocalToolsDir=$true;createUpdaterArtifacts=$false}}
    if($ObserverUrl){$overlay.bundle.externalBin=@('binaries/cua-driver','binaries/cua-driver-uia','binaries/taiku-observer')}
    [IO.File]::WriteAllText($bundleConfig,($overlay|ConvertTo-Json -Depth 5))
    Run 'npm.cmd' @('run','tauri','build','--','--bundles','nsis','--no-sign','--config',$bundleConfig) "$base\package.log"
    Status 'backend-tests'
    Run 'cargo.exe' @('test','--locked','--manifest-path','src-tauri/Cargo.toml','--lib','--no-run') "$base\backend-build.log"
    # Tauri embeds Common Controls v6 in the app, but not its library test
    # executable. Use that same manifest so TaskDialogIndirect can load.
    $appManifest=Join-Path $base 'test-app.manifest'
    Run 'mt.exe' @('-nologo',("-inputresource:"+"$root\app\src-tauri\target\release\app.exe;#1"),("-out:"+$appManifest)) "$base\test-manifest.log"
    $testExecutables=@(Get-ChildItem "$root\app\src-tauri\target\debug\deps\app_lib-*.exe")
    if($testExecutables.Count -ne 1){throw 'Backend test executable missing or ambiguous'}
    foreach($test in $testExecutables){
        Run 'mt.exe' @('-nologo','-manifest',$appManifest,("-outputresource:"+$test.FullName+';#1')) "$base\test-manifest.log"
    }
    # Running Cargo again can regenerate the executable and discard its
    # manifest. Execute exactly the artifact that was just built and prepared.
    Run $testExecutables[0].FullName @('--skip','agent::e2e') "$base\backend-tests.log"

    $package=@(Get-ChildItem "$root\app\src-tauri\target\release\bundle\nsis\*.exe")
    if ($package.Count -ne 1) { throw 'Installer is missing or ambiguous' }
    # Tauri restores the build binary after packaging. The NSIS payload has
    # its one bundle marker changed from UNK to NSS; hash that exact transform.
    Add-Type @'
using System;
using System.IO;
using System.Text;
using System.Security.Cryptography;
public static class NsisPayloadHash {
    public static string Compute(string path) {
        byte[] data=File.ReadAllBytes(path);
        byte[] old=Encoding.ASCII.GetBytes("__TAURI_BUNDLE_TYPE_VAR_UNK");
        byte[] next=Encoding.ASCII.GetBytes("__TAURI_BUNDLE_TYPE_VAR_NSS");
        int count=0;
        for(int i=0;i<=data.Length-old.Length;i++) {
            bool match=true;
            for(int j=0;j<old.Length;j++) if(data[i+j]!=old[j]) {match=false;break;}
            if(match) {count++;Buffer.BlockCopy(next,0,data,i,next.Length);}
        }
        if(count!=1) throw new Exception("Expected exactly one Tauri bundle marker");
        using(var sha=SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(data)).Replace("-","");
    }
}
'@
    $manifest=@{revision=$Revision;root=$root;installer=$package[0].FullName;
        source_archive_sha256=$SourceSha256;vendor_archive_sha256=$VendorSha256;
        installer_sha256=(Get-FileHash $package[0].FullName -Algorithm SHA256).Hash;
        app_build_sha256=(Get-FileHash "$root\app\src-tauri\target\release\app.exe" -Algorithm SHA256).Hash;
        app_sha256=[NsisPayloadHash]::Compute("$root\app\src-tauri\target\release\app.exe");
        app_hash_transform="__TAURI_BUNDLE_TYPE_VAR_UNK -> __TAURI_BUNDLE_TYPE_VAR_NSS, exactly one occurrence, no other bytes";
        driver_sha256=(Get-FileHash "$root\app\src-tauri\binaries\cua-driver-x86_64-pc-windows-msvc.exe" -Algorithm SHA256).Hash;
        # The driver depends on the whole source revision (impl\axdiff too), not
        # only on the pin and patch. apply-and-build.sh builds through a fixed
        # junction and maps it and CARGO_HOME away, so the same revision gives
        # the same driver anywhere.
        driver_source_revision=$Revision;
        driver_patch_sha256=(Get-FileHash "$root\impl\native\cua-driver.patch" -Algorithm SHA256).Hash;
        driver_path_remap="C:\icu-driver-src ($root)=/src $($env:CARGO_HOME)=/cargo";
        # The toolchain is an input too: rustc 1.97.0 built another driver from
        # the same tree, and the MSVC toolset links its static CRT in. rustc is
        # the one rust-toolchain.toml selects under $root, where the driver builds.
        driver_rustc=((& rustc.exe -vV) | Where-Object {$_ -match '^(rustc |commit-hash: |LLVM version: )'}) -join '; ';
        driver_msvc_toolset=$env:VCToolsVersion;
        driver_windows_sdk="$env:WindowsSDKVersion".TrimEnd('\');
        driver_visual_studio=$env:VSCMD_VER;
        signed=$false;installed=$false}
    if($ObserverUrl){
        $manifest.observer_archive_sha256=$ObserverSha256
        $manifest.observer_sha256=(Get-FileHash "$root\app\src-tauri\binaries\taiku-observer-x86_64-pc-windows-msvc.exe" -Algorithm SHA256).Hash
    }
    $manifest | ConvertTo-Json | Set-Content "$base\package-manifest.json" -Encoding UTF8
    Status 'package-ready'
} catch {
    Status 'failed' ($_.Exception.Message -replace 'https?://\S+','<url>')
    exit 1
}
