param(
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Harness='C:\ICUAudit\harness',
    [string]$Result='C:\Users\Public\gaps4-accept.json'
)
# The path the cancel work must not break: an administrator who ANSWERS YES to the elevation prompt. Run through as-user.ps1 as the
# host's logged-on administrator (filtered token) with the clicker answering "Yes" (cancel-run.ps1 -Action start-clicker -Key alt+y).
# The all-users 0.1.0 build's own install step (cua_install_update) runs the installer with runas; this checks that the update
# really lands: the installed app.exe becomes 0.2.0 under Program Files, no per-user copy appears, the installer relaunched the app,
# and (what the installer's /R does) the relaunched app is not elevated.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$install='C:\Program Files\Copilot'; $exe=Join-Path $install 'app.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps4-accept-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
. (Join-Path $PSScriptRoot 'lib.ps1')
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuTok {
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, byte[] v, int l, out int r);
    public static int Integrity(int pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return -1;
        IntPtr t; if (!OpenProcessToken(p, 8, out t)) { CloseHandle(p); return -1; }
        int need; GetTokenInformation(t, 25, null, 0, out need);
        byte[] b = new byte[need]; int got;
        bool ok = GetTokenInformation(t, 25, b, need, out got); CloseHandle(t); CloseHandle(p);
        if (!ok) return -1;
        IntPtr sid = (IntPtr)(Environment.Is64BitProcess ? BitConverter.ToInt64(b, 0) : BitConverter.ToInt32(b, 0));
        return Marshal.ReadInt32(sid, 8 + 4 * (Marshal.ReadByte(sid, 1) - 1));
    }
}
'@
$r=[ordered]@{scenario='administrator-answers-yes';user=$env:USERNAME}
$server=$null;$app=$null
try {
    if(-not (Is-Admin-Member)){ throw "$env:USERNAME is not a member of Administrators" }
    $before=Install-Fingerprint
    $versionBefore=Version
    Copy-Item (Join-Path $Dir 'manifest-good.json') (Join-Path $Dir 'manifest.json') -Force
    Remove-Item (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue
    $server=Start-UpdateServer
    $app=Start-App; $oldPid=$app.Id
    Step 'app started' @{pid=$oldPid;version=$versionBefore}
    Wait-For { Find-Name 'is ready' $false } 40 'no update toast' | Out-Null
    Press (Find-Name 'Install & restart')
    Step 'install clicked'
    # the installer replaces the files and relaunches the app: wait for the version on disk and a new app process
    Wait-For { (Version) -eq '0.2.0' } 240 'the installed version did not become 0.2.0' | Out-Null
    Step 'version on disk is 0.2.0'
    $new=Wait-For { @(App-Procs | Where-Object { $_.ProcessId -ne $oldPid })[0] } 90 'no relaunched app'
    Start-Sleep -Seconds 3
    $r.version_before=$versionBefore; $r.version_after=Version
    $app.Refresh(); $r.old_app_exited=$app.HasExited
    $r.relaunched_pid=$new.ProcessId
    $r.relaunched_integrity=('0x{0:x}' -f [IcuTok]::Integrity([int]$new.ProcessId))
    $r.per_user_copies=@(Per-User-Copies | ForEach-Object { [string]$_ })
    $r.files_signed=@(Get-ChildItem $install -File | Where-Object { $_.Extension -in '.exe','.dll' } | ForEach-Object { '{0} {1}' -f $_.Name,(Get-AuthenticodeSignature $_.FullName).Status })
    $r.install_changed=(($before -join "`n") -ne ((Install-Fingerprint) -join "`n"))
    $r.requests=@(Req)
    $r.consent_answers=@(Get-Content 'C:\Users\Public\consent-answers.txt' -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
    Shot 'accepted'
    if($r.version_after -ne '0.2.0'){ throw 'not updated' }
    if($r.per_user_copies.Count){ throw 'a per-user copy appeared' }
    if(-not ($r.consent_answers | Where-Object { $_ -match 'answered the prompt' })){ throw 'no prompt was answered' }
    $r.ok=$true
} catch { $r.ok=$false; $r.error=$_.Exception.Message; Step 'FAILED' @{error=$_.Exception.Message}; Shot 'failed' }
finally {
    Step 'cleanup begins'
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath.StartsWith($install) -or $_.ExecutablePath -like "$env:LOCALAPPDATA\Programs\Copilot\*") })){ try{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }catch{} }
    if($server -and -not $server.HasExited){ $server.Kill() }
    $r.steps=@($script:steps | ForEach-Object { '{0} {1}' -f $_.t,$_.what })
    try { $json=$r | ConvertTo-Json -Depth 4; [IO.File]::WriteAllText($Result,$json,(New-Object Text.UTF8Encoding($false))); [IO.File]::WriteAllText((Join-Path $work 'result.json'),$json,(New-Object Text.UTF8Encoding($false))) } catch { Write-Output ('result not written: '+$_.Exception.Message) }
}
if(-not $r.ok){ exit 1 }
