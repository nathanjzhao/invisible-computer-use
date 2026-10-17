param(
    [Parameter(Mandatory=$true)][ValidateSet('peruser-new','peruser-old','msi-bare-new','msi-bare-old','msi-cancel-new','msi-accept-new','msi-cancel-old')][string]$Scenario,
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Harness='C:\ICUAudit\harness',
    [string]$Result='C:\Users\Public\gaps5-upd.json'
)
# Run through as-user.ps1 as the host's logged-on user, a member of Administrators (filtered token), in the interactive session,
# with the UAC prompt on the user's own desktop (cancel-run.ps1 -Action policy-nosecure) and, for the scenarios that have a prompt,
# the clicker running (cancel-run.ps1 -Action start-clicker -Key escape|alt+y), which logs every "User Account Control" window it
# answers to C:\Users\Public\consent-answers.txt. The app is the real signed 0.1.0 build with its updater pointed at the local server.
#   peruser-new  a per-user NSIS install, the build with this change: no prompt at all, the update lands, the app comes back
#   peruser-old  the same install, the build before: the baseline (a prompt for an install that needs none)
#   msi-bare-new an MSI install, a manifest that has only the bare windows-x86_64 entry (the NSIS installer): not offered
#   msi-bare-old the same, the build before: the NSIS installer is offered to an MSI install
#   msi-cancel-old the same, the build before: the plugin runs msiexec and exits the app (the baseline)
#   msi-cancel-new an MSI install, a manifest with an MSI entry, the prompt cancelled: the app and the install are kept
#   msi-accept-new the same, the prompt accepted: msiexec replaces the install and the app comes back
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$kind=$(if($Scenario -like 'peruser*'){'peruser'}else{'msi'})
$install=$(if($kind -eq 'peruser'){"$env:LOCALAPPDATA\Programs\Copilot"}else{'C:\Program Files\Copilot'})
$exe=Join-Path $install 'app.exe'
$manifest=$(if($Scenario -like 'msi-bare*' -or $kind -eq 'peruser'){'manifest-bare.json'}else{'manifest-msi.json'})
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps5-'+$Scenario+'-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
. (Join-Path $Harness 'gaps4\lib.ps1')
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class IcuTok5 {
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
function Prompts { @(Get-Content 'C:\Users\Public\consent-answers.txt' -ErrorAction SilentlyContinue | Where-Object { $_ -match 'prompt \d+ window' }) }
function Registered { @(foreach($root in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'){
    foreach($k in @(Get-ChildItem $root -ErrorAction SilentlyContinue)){ $p=Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue; if($p.DisplayName -eq 'Copilot'){ '{0} {1} {2}' -f $k.PSChildName,$p.DisplayVersion,$p.InstallLocation } } }) }
function Copies { @(foreach($d in 'C:\Program Files\Copilot',"$env:LOCALAPPDATA\Programs\Copilot"){ if(Test-Path "$d\app.exe"){ '{0} {1}' -f $d,(Get-Item "$d\app.exe").VersionInfo.ProductVersion } }) }
$r=[ordered]@{scenario=$Scenario;user=$env:USERNAME;manifest=$manifest;install=$install}
$server=$null;$app=$null
try {
    if(-not (Is-Admin-Member)){ throw "$env:USERNAME is not a member of Administrators (log on again after cancel-run.ps1 -Action policy-nosecure)" }
    if(-not (Test-Path $exe)){ throw "nothing installed at $install" }
    $before=Install-Fingerprint
    $versionBefore=Version
    Copy-Item (Join-Path $Dir $manifest) (Join-Path $Dir 'manifest.json') -Force
    Remove-Item (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue
    Remove-Item 'C:\Users\Public\consent-answers.txt' -ErrorAction SilentlyContinue
    $server=Start-UpdateServer
    $app=Start-App; $oldPid=$app.Id
    Step 'app started' @{pid=$oldPid;version=$versionBefore;registered=(Registered)}
    $r.version_before=$versionBefore; $r.registered_before=@(Registered); $r.copies_before=@(Copies)
    $toast=$null; try { $toast=Wait-For { Find-Name 'is ready' $false } $(if($Scenario -eq 'msi-bare-new'){25}else{40}) 'no update toast' } catch { $toast=$null }
    $r.toast_shown=[bool]$toast
    if($toast){ $r.toast_text=$toast.Current.Name }
    if($Scenario -eq 'msi-bare-new'){
        Start-Sleep -Seconds 10
        $r.requests=@(Req)
        $r.archive_requests=@(Req | Where-Object { $_ -match '/files/' }).Count
        $r.app_alive=(-not $app.HasExited)
        Shot 'no-toast'
        if($r.toast_shown){ throw 'an MSI install was offered the NSIS installer' }
        if($r.archive_requests){ throw 'the archive was downloaded' }
        if(-not (@(Req) -join ' ' -match 'latest.json')){ throw 'the app never asked for the manifest, so "not offered" proves nothing' }
        $r.ok=$true
    } else {
        if(-not $toast){ throw 'no update toast' }
        Press (Find-Name 'Install & restart')
        Step 'install clicked' @{pids_before=@(App-Procs | ForEach-Object { $_.ProcessId })}
        $prompts=$null
        switch -Wildcard ($Scenario){
            'msi-cancel-new' {
                $said=Wait-For { Find-Name 'Copilot was not changed' $false } 120 'no cancelled message shown'
                $r.message=$said.Current.Name
                Shot 'cancelled-message'
                Start-Sleep -Seconds 5
                $app.Refresh(); $r.app_alive_same_pid=(-not $app.HasExited)
                $r.install_button_back=[bool](Find-Name 'Install & restart')
                $r.install_unchanged=(($before -join "`n") -eq ((Install-Fingerprint) -join "`n"))
                $r.version_after=Version
                $r.msiexec_left=@(Get-Process msiexec -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -ne 0 }).Count
                if($r.message -notmatch 'Copilot was not changed'){ throw "unexpected message: $($r.message)" }
                if(-not $r.app_alive_same_pid){ throw 'the app did not keep running' }
                if(-not $r.install_unchanged){ throw 'the install changed' }
                $r.ok=$true
            }
            'msi-accept-new' {
                Wait-For { (Version) -eq '0.2.0' } 240 'the installed version did not become 0.2.0' | Out-Null
                Step 'version on disk is 0.2.0'
                $new=$null; try { $new=Wait-For { @(App-Procs | Where-Object { $_.ProcessId -ne $oldPid })[0] } 120 'no relaunched app' } catch { $new=$null }
                Start-Sleep -Seconds 3
                $app.Refresh(); $r.old_app_exited=$app.HasExited
                $r.relaunched=[bool]$new
                if($new){ $r.relaunched_pid=$new.ProcessId; $r.relaunched_integrity=('0x{0:x}' -f [IcuTok5]::Integrity([int]$new.ProcessId)) }
                $r.version_after=Version
                $r.registered_after=@(Registered); $r.copies_after=@(Copies)
                $r.files_signed=@(Get-ChildItem $install -File | Where-Object { $_.Extension -in '.exe','.dll' } | ForEach-Object { '{0} {1}' -f $_.Name,(Get-AuthenticodeSignature $_.FullName).Status })
                if($r.version_after -ne '0.2.0'){ throw 'not updated' }
                if(-not $r.old_app_exited){ throw 'the old app is still running' }
                $r.ok=$true
            }
            'peruser-new' {
                Wait-For { (Version) -eq '0.2.0' } 240 'the installed version did not become 0.2.0' | Out-Null
                Step 'version on disk is 0.2.0'
                $new=$null; try { $new=Wait-For { @(App-Procs | Where-Object { $_.ProcessId -ne $oldPid })[0] } 90 'no relaunched app' } catch { $new=$null }
                Start-Sleep -Seconds 3
                $app.Refresh(); $r.old_app_exited=$app.HasExited
                $r.relaunched=[bool]$new
                if($new){ $r.relaunched_pid=$new.ProcessId; $r.relaunched_integrity=('0x{0:x}' -f [IcuTok5]::Integrity([int]$new.ProcessId)) }
                $r.version_after=Version
                $r.copies_after=@(Copies); $r.registered_after=@(Registered)
                $r.files_signed=@(Get-ChildItem $install -File | Where-Object { $_.Extension -in '.exe','.dll' } | ForEach-Object { '{0} {1}' -f $_.Name,(Get-AuthenticodeSignature $_.FullName).Status })
                if($r.version_after -ne '0.2.0'){ throw 'not updated' }
                if(@(Prompts).Count){ throw 'a UAC prompt appeared for a per-user install' }
                if(@(Copies | Where-Object { $_ -like 'C:\Program Files*' }).Count){ throw 'an all-users copy appeared' }
                $r.ok=$true
            }
            default {
                # baselines: the plugin's own install. Give it time, then record what the machine looks like.
                Start-Sleep -Seconds 100
                $app.Refresh(); $r.app_alive_same_pid=(-not $app.HasExited)
                $r.version_after=Version
                $r.copies_after=@(Copies); $r.registered_after=@(Registered)
                $r.app_processes=@(Get-Process -Name app -ErrorAction SilentlyContinue | ForEach-Object { 'pid '+$_.Id+' '+$_.Path })
                # the plugin's install exits the app, so there is usually no window left to read
                try { $r.window_texts=@(Texts | Select-Object -First 12 | ForEach-Object { [string]$_ }) } catch { $r.window_texts=@('no window: '+$_.Exception.Message) }
                try { Shot 'baseline-after' } catch {}
                $r.ok=$true
            }
        }
    }
    $r.requests=@(Req)
    $r.prompts=@(Prompts)
    $r.prompt_count=@(Prompts).Count
    $r.consent_log=@(Get-Content 'C:\Users\Public\consent-answers.txt' -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
} catch { $r.ok=$false; $r.error=$_.Exception.Message; Step 'FAILED' @{error=$_.Exception.Message}; Shot 'failed'; $r.prompt_count=@(Prompts).Count; $r.requests=@(Req); $r.copies_after=@(Copies) }
finally {
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath.StartsWith('C:\Program Files\Copilot') -or $_.ExecutablePath -like "$env:LOCALAPPDATA\Programs\Copilot\*") })){ try{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }catch{} }
    if($server -and -not $server.HasExited){ $server.Kill() }
    $r.steps=@($script:steps | ForEach-Object { '{0} {1}' -f $_.t,$_.what }); $r.work=$work
    try { $json=$r | ConvertTo-Json -Depth 4; [IO.File]::WriteAllText($Result,$json,(New-Object Text.UTF8Encoding($false))); [IO.File]::WriteAllText((Join-Path $work 'result.json'),$json,(New-Object Text.UTF8Encoding($false))) } catch { Write-Output "json failed: $_" }
}
if(-not $r.ok){ exit 1 }
