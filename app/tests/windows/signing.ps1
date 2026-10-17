<#
Test for `sign-windows.ps1 verify-installer` (the by-name check of what ships inside the NSIS
installer). Run as admin on a Windows host with the Windows SDK signtool, NSIS's makensis (Tauri's
copy under %LOCALAPPDATA%\tauri\NSIS, or -Makensis) and 7-Zip (7z.exe on PATH or -SevenZip). It makes
a throwaway CA, trusts it in LocalMachine\Root for the run and removes it afterwards, builds small NSIS
installers with the same !finalize / !uninstfinalize sign hooks Tauri uses, and checks:

  green     everything signed, including the uninstaller            -> verify-installer exits 0
  red       a plugin DLL in the payload unsigned (UserInfo.dll, which Tauri leaves unsigned)
  red       app.exe in the payload unsigned
  red       a required file missing from the payload
  red       the uninstaller left unsigned and reported as signed (what the 32-bit .NET check did):
            caught by the ledger (no recorded sign) and, with --install, by installing it
  red       the ledger lacking the uninstaller's entry although it is signed (the ledger guard alone)

-Mutate then removes each guard once, from a copy of the script, and requires the cases it protects to
stop failing (the test going red when the guard is gone). Exit 0 only if every case behaves.
  powershell -File signing.ps1 [-Script path\sign-windows.ps1] [-Makensis path] [-SevenZip path] [-Mutate] [-Out dir]
#>
param([string]$Script, [string]$Makensis, [string]$SevenZip, [switch]$Mutate, [string]$Out = "$env:TEMP\icu-signing-test")
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $PSCommandPath
if (-not $Script) { $Script = Join-Path $here '..\..\scripts\sign-windows.ps1' }
$Script = (Resolve-Path $Script).Path
if (-not $Makensis) {
    $found = @(Get-ChildItem "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe", 'C:\Program Files (x86)\NSIS\makensis.exe' -ErrorAction SilentlyContinue)
    if (-not $found.Count) { $found = @(Get-ChildItem 'C:\ICUAudit' -Recurse -Filter makensis.exe -ErrorAction SilentlyContinue) }
    if ($found.Count) { $Makensis = $found[0].FullName }
}
if (-not $Makensis -or -not (Test-Path $Makensis)) { throw 'makensis.exe not found; pass -Makensis' }
if ($SevenZip) { $env:ICU_WIN_7Z = $SevenZip }
Remove-Item $Out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Out | Out-Null
$results = New-Object Collections.ArrayList
function Check([string]$name, [bool]$ok, [string]$detail = '') {
    [void]$results.Add([pscustomobject]@{ name = $name; ok = $ok; detail = $detail })
    Write-Host ("{0}  {1}  {2}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $detail)
}
function Run-Script([string]$script, [string[]]$arguments) {
    $previous = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $text = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $script @arguments 2>&1 | Out-String; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $previous }
    return @{ code = $code; text = $text }
}
function Slug([string]$text) { return ($text -replace '[^A-Za-z0-9]+', '_') }
$productKey = 'IcuSigningTestApp'
if (Test-Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productKey") { throw "$productKey is already installed for this account" }

# Throwaway CA and leaf, as in the 2026-10-01 signing study.
$stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
$ca = New-SelfSignedCertificate -Type Custom -Subject "CN=ICU signing-test throwaway CA $stamp" -KeyUsage CertSign,CRLSign,DigitalSignature `
    -KeyExportPolicy NonExportable -KeyLength 3072 -HashAlgorithm sha256 -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddDays(1) `
    -TextExtension @('2.5.29.19={critical}{text}ca=1&pathlength=0')
$leaf = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=ICU signing-test signer $stamp" -Signer $ca -KeyExportPolicy Exportable `
    -KeyLength 3072 -HashAlgorithm sha256 -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddHours(20)
$caCer = Join-Path $Out 'ca.cer'
Export-Certificate -Cert $ca -FilePath $caCer | Out-Null
$trusted = Import-Certificate -FilePath $caCer -CertStoreLocation Cert:\LocalMachine\Root
$password = [Convert]::ToBase64String((1..18 | ForEach-Object { Get-Random -Maximum 256 }))
$pfx = Join-Path $Out 'leaf.pfx'
Export-PfxCertificate -Cert $leaf -FilePath $pfx -Password (ConvertTo-SecureString $password -AsPlainText -Force) -ChainOption BuildChain | Out-Null
Remove-Item "Cert:\LocalMachine\My\$($ca.Thumbprint)" -DeleteKey
$env:ICU_WIN_SIGN_PROVIDER = 'pfx'; $env:ICU_WIN_PFX = $pfx; $env:ICU_WIN_PFX_PASSWORD = $password
try {
    # Tiny PEs. Names match what the app ships; UserInfo.dll stands for the NSIS plugin Tauri leaves unsigned.
    $csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
    $src = Join-Path $Out 'p.cs'
    'class P { static int Main() { return 0; } }' | Set-Content $src
    'public class L { public static int V() { return 1; } }' | Set-Content "$Out\l.cs"
    $files = Join-Path $Out 'files'; New-Item -ItemType Directory $files | Out-Null
    foreach ($n in 'app.exe', 'cua-driver.exe', 'cua-driver-uia.exe') { & $csc /nologo "/out:$files\$n" $src | Out-Null; if ($LASTEXITCODE) { throw "csc $n" } }
    & $csc /nologo /target:library "/out:$files\UserInfo.dll" "$Out\l.cs" | Out-Null; if ($LASTEXITCODE) { throw 'csc dll' }

    # The sign hook makensis runs. A file whose name matches ICU_TEST_SKIP_SIGN is left unsigned and
    # reported as success: what makensis did with the failed uninstaller sign, which it carried on past.
    $hookText = @'
param([string]$File)
$skip = $env:ICU_TEST_SKIP_SIGN
if ($skip -and (Split-Path -Leaf $File) -like $skip) { exit 0 }
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $env:ICU_TEST_SCRIPT sign $File
exit $LASTEXITCODE
'@
    Set-Content "$Out\hook.ps1" $hookText
    $env:ICU_TEST_SCRIPT = $Script
    function Build-Installer([string]$name, [string]$skipPattern, [string[]]$exes, [string]$ledger) {
        $env:ICU_TEST_SKIP_SIGN = $skipPattern
        $env:ICU_WIN_SIGN_LEDGER = $ledger
        $work = Join-Path $Out $name; New-Item -ItemType Directory $work | Out-Null
        foreach ($f in $exes + @('UserInfo.dll')) { Copy-Item "$files\$f" $work }
        # signed the way Tauri signs the app's files and the plugins before makensis packs them
        foreach ($f in $exes + @('UserInfo.dll')) { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$Out\hook.ps1" "$work\$f" | Out-Null }
        $hook = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$Out\hook.ps1`" `"%1`""
        $uninstallKey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$productKey"
        $lines = @('Unicode true', 'Name "Mini"', "OutFile `"$work\setup.exe`"", 'InstallDir "$LOCALAPPDATA\Programs\IcuSigningTest"', 'RequestExecutionLevel user', 'SilentInstall silent',
            "!uninstfinalize '$hook'", "!finalize '$hook'", 'Section')
        $lines += 'SetOutPath $INSTDIR'
        foreach ($f in $exes) { $lines += "File `"$work\$f`"" }
        # a plugin DLL travels in $PLUGINSDIR, as NSIS plugins do
        $lines += @('InitPluginsDir', 'SetOutPath $PLUGINSDIR', "File `"$work\UserInfo.dll`"", 'SetOutPath $INSTDIR')
        $lines += @('WriteUninstaller "$INSTDIR\uninstall.exe"', "WriteRegStr HKCU `"$uninstallKey`" UninstallString '`"`$INSTDIR\uninstall.exe`"'", 'SectionEnd',
            'Section Uninstall', 'Delete "$INSTDIR\*.exe"', 'RMDir "$INSTDIR"', "DeleteRegKey HKCU `"$uninstallKey`"", 'SectionEnd')
        Set-Content "$work\mini.nsi" $lines -Encoding Unicode
        $previous = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        $log = & $Makensis /V2 "$work\mini.nsi" 2>&1 | Out-String
        $ErrorActionPreference = $previous
        Set-Content "$work\makensis.log" $log
        Remove-Item Env:\ICU_TEST_SKIP_SIGN -ErrorAction SilentlyContinue; Remove-Item Env:\ICU_WIN_SIGN_LEDGER -ErrorAction SilentlyContinue
        if (-not (Test-Path "$work\setup.exe")) { throw "makensis failed for ${name}:`n$log" }
        return "$work\setup.exe"
    }
    $std = @('app.exe', 'cua-driver.exe', 'cua-driver-uia.exe')
    $require = 'app.exe,cua-driver.exe,cua-driver-uia.exe,UserInfo.dll'
    $green = Build-Installer 'green' '' $std "$Out\ledger-green.txt"
    Set-Content "$Out\ledger-nonst.txt" @(Get-Content "$Out\ledger-green.txt" | Where-Object { $_ -notmatch '\\nst[0-9A-Za-z]+\.tmp\|' })
    if (@(Get-Content "$Out\ledger-nonst.txt").Count -ne @(Get-Content "$Out\ledger-green.txt").Count - 1) { throw 'expected exactly one nst*.tmp line in the green ledger' }
    $noUninst = Build-Installer 'unsigned-uninstaller' 'nst*.tmp' $std "$Out\ledger-nouninst.txt"
    $noApp = Build-Installer 'unsigned-app' 'app.exe' $std "$Out\ledger-noapp.txt"
    $noDriver = Build-Installer 'missing-driver' '' @('app.exe', 'cua-driver.exe') "$Out\ledger-nodriver.txt"
    $noPlugin = Build-Installer 'unsigned-plugin' 'UserInfo.dll' $std "$Out\ledger-noplugin.txt"
    $cases = @(
        @{ name = 'green: all signed'; installer = $green; ledger = "$Out\ledger-green.txt"; install = $true; expect = 0 },
        @{ name = 'red: unsigned plugin DLL in the payload'; installer = $noPlugin; ledger = $null; expect = 1; mention = 'UserInfo.dll' },
        @{ name = 'red: unsigned app.exe in the payload'; installer = $noApp; ledger = $null; expect = 1; mention = 'app.exe' },
        @{ name = 'red: required cua-driver-uia.exe missing'; installer = $noDriver; ledger = $null; expect = 1; mention = 'cua-driver-uia.exe' },
        @{ name = 'red: unsigned uninstaller, ledger has no sign for it'; installer = $noUninst; ledger = "$Out\ledger-nouninst.txt"; expect = 1; mention = 'nst' },
        @{ name = 'red: unsigned uninstaller, found by installing'; installer = $noUninst; ledger = $null; install = $true; expect = 1; mention = 'uninstall.exe' },
        # the uninstaller itself is signed here; only its ledger line is gone, so only the ledger guard can catch it
        @{ name = 'red: ledger lacks the uninstaller (signed) entry'; installer = $green; ledger = "$Out\ledger-nonst.txt"; expect = 1; mention = 'nst' }
    )
    function Run-Cases([string]$script, [string]$label, [string[]]$only) {
        $outcome = @{}
        foreach ($case in $cases) {
            if ($only -and ($only -notcontains $case.name)) { continue }
            $arguments = @('verify-installer', $case.installer, '--require', $require, '--product', $productKey)
            if ($case.ledger) { $arguments += @('--ledger', $case.ledger) }
            if ($case.install) { $arguments += '--install' }
            $r = Run-Script $script $arguments
            $named = (-not $case.mention) -or ($r.text -match [regex]::Escape($case.mention))
            $ok = ($r.code -eq $case.expect) -and $named
            $outcome[$case.name] = $ok
            Set-Content (Join-Path $Out ("out-" + $label + (Slug $case.name) + '.txt')) $r.text
            if (-not $label) {
                $note = if ($named) { '' } else { "; output does not name $($case.mention)" }
                Check $case.name $ok "exit $($r.code) (expected $($case.expect))$note"
            }
        }
        return $outcome
    }
    # the install check must leave nothing behind
    function Assert-Clean { Check 'no install left behind' (-not (Test-Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productKey")) '' }


    [void](Run-Cases $Script '' $null)
    Assert-Clean

    if ($Mutate) {
        # each guard removed once from a copy; the cases it protects must stop failing
        $text = Get-Content $Script -Raw
        $mutants = @(
            @{ name = 'unsigned PE in the payload no longer a problem'; from = '$problems += ("{0} in the installer: {1}" -f $relative, ($check.problems -join ''; ''));'; to = '';
               cases = @('red: unsigned plugin DLL in the payload', 'red: unsigned app.exe in the payload') },
            @{ name = 'required payload names no longer checked'; from = 'if ($names -notcontains $name) { $problems += "$name is not in the installer" }'; to = '';
               cases = @('red: required cua-driver-uia.exe missing') },
            @{ name = 'NSIS uninstaller ledger entry no longer required'; from = '$problems += ''no verified sign recorded for the NSIS uninstaller (nst*.tmp)'''; to = '';
               cases = @('red: ledger lacks the uninstaller (signed) entry') },
            # 7-Zip also lists the mini installer's uninstall.exe, so the payload and install guards each cover the
            # other here; only removing both lets the unsigned uninstaller through. (In Tauri's installer 7-Zip does
            # not list uninstall.exe, so the install check is the one that sees it: see the README's real build.)
            @{ name = 'unsigned PE in the payload and in the install no longer a problem';
               from = @('$problems += ("{0} in the installer: {1}" -f $relative, ($check.problems -join ''; ''));', '$problems += ("{0} installed: {1}" -f $relative, ($check.problems -join ''; ''));'); to = '';
               cases = @('red: unsigned uninstaller, found by installing') }
        )
        foreach ($mutant in $mutants) {
            $mutated = $text; $stale = $false
            foreach ($from in @($mutant.from)) {
                if (-not $mutated.Contains($from)) { $stale = $true; break }
                $mutated = $mutated.Replace($from, $mutant.to)
            }
            if ($stale) { Check "mutant '$($mutant.name)'" $false 'guard text not found in the script (stale mutation)'; continue }
            $copy = Join-Path $Out ("mutant-" + (Slug $mutant.name) + ".ps1")
            Set-Content $copy $mutated -Encoding UTF8
            $outcome = Run-Cases $copy ('mutant-' + (Slug $mutant.name) + '-') $mutant.cases
            foreach ($c in $mutant.cases) { Check "mutation '$($mutant.name)': case '$c' goes wrong, so the test would go red" (-not $outcome[$c]) "case still behaved as required under the mutant: $($outcome[$c])" }
        }
        Assert-Clean
    }
} finally {
    Remove-Item "Cert:\LocalMachine\Root\$($trusted.Thumbprint)" -ErrorAction SilentlyContinue
    Remove-Item "Cert:\LocalMachine\My\$($leaf.Thumbprint)" -DeleteKey -ErrorAction SilentlyContinue
    Remove-Item Env:\ICU_WIN_PFX_PASSWORD, Env:\ICU_WIN_PFX, Env:\ICU_WIN_SIGN_PROVIDER -ErrorAction SilentlyContinue
}
$bad = @($results | Where-Object { -not $_.ok })
$results | ConvertTo-Json | Set-Content (Join-Path $Out 'results.json')
Write-Host ("{0} checks, {1} failed" -f $results.Count, $bad.Count)
exit $(if ($bad.Count) { 1 } else { 0 })
