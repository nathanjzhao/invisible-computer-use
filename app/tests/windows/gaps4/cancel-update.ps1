param(
    [Parameter(Mandatory=$true)][ValidateSet('new','old')][string]$Build,
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Harness='C:\ICUAudit\harness',
    [string]$Result='C:\Users\Public\gaps4-cancel.json'
)
# Item 3: an administrator cancels the update's elevation prompt.
# Run through as-user.ps1 as the host's logged-on user, who is a member of Administrators (filtered token, medium
# integrity) with the default consent policy (ConsentPromptBehaviorAdmin 5: consent on the secure desktop), while
# gaps4\cancel-run.ps1 has a SYSTEM watcher running that kills consent.exe the moment it appears. Killing the consent UI
# is how this host answers "Cancel" without driving the secure desktop (which UI Automation cannot reach); Windows
# reports it to the caller of ShellExecute as a cancelled elevation.
#   -Build new : the 0.1.0 build with `cua_install_update` (the app runs the installer with `runas` and waits).
#                Expected: the toast says the update was cancelled and Copilot was not changed; same pid; the installed
#                files are unchanged; no per-user copy.
#   -Build old : the 0.1.0 build before this change (the plugin runs the installer and exits the app). The baseline:
#                the app exits and a per-user copy appears. The script records it and does NOT fail on it.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$install='C:\Program Files\Copilot'; $exe=Join-Path $install 'app.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps4-cancel-'+$Build+'-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
. (Join-Path $PSScriptRoot 'lib.ps1')
$r=[ordered]@{scenario="cancelled-elevation-$Build";user=$env:USERNAME}
$server=$null;$app=$null
try {
    if(-not (Is-Admin-Member)){ throw "$env:USERNAME is not a member of Administrators (add it and log on again first)" }
    $before=Install-Fingerprint
    if(-not $before.Count){ throw "nothing installed at $install" }
    $versionBefore=Version
    Copy-Item (Join-Path $Dir 'manifest-good.json') (Join-Path $Dir 'manifest.json') -Force
    Remove-Item (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue
    $server=Start-UpdateServer
    $app=Start-App; $oldPid=$app.Id
    Step 'app started' @{pid=$oldPid;version=$versionBefore;per_user_copies=(Per-User-Copies)}
    $toast=Wait-For { Find-Name 'is ready' $false } 40 'no update toast'
    Step 'toast' @{text=$toast.Current.Name}
    Press (Find-Name 'Install & restart')
    Step 'install clicked' @{pids_before=@(App-Procs | ForEach-Object { $_.ProcessId })}
    if($Build -eq 'new'){
        $said=Wait-For { Find-Name 'Copilot was not changed' $false } 120 'no cancelled message shown'
        $r.message=$said.Current.Name
        Shot 'cancelled-message'
        Start-Sleep -Seconds 5
        $app.Refresh(); $r.app_alive_same_pid=(-not $app.HasExited)
        $r.install_button_back=[bool](Find-Name 'Install & restart')
    } else {
        # baseline: the plugin's own install step. The prompt is answered at once; look at what the app does next.
        Start-Sleep -Seconds 12
        Shot 'baseline-after-cancel'
        $r.toast_texts_after_cancel=@(Texts | Where-Object { $_ -match 'pdate|nstall|ailed|estart|ancel' } | ForEach-Object { [string]$_ })
        $r.app_processes=@(Get-Process -Name app -ErrorAction SilentlyContinue | ForEach-Object { 'pid '+$_.Id+' '+$_.Path })
        $r.window_texts=@(Texts | Select-Object -First 40 | ForEach-Object { [string]$_ })
        $app.Refresh(); $r.app_alive_same_pid=(-not $app.HasExited)
        $r.old_app_exited=-not $r.app_alive_same_pid
        $r.installer_processes_left=@(Get-Process | Where-Object { $_.Path -like '*Copilot*setup*' -or $_.Name -like 'Copilot*' } | ForEach-Object { [string]$_.Name })
        $r.install_button_back=[bool](Find-Name 'Install & restart')
        # give a per-user fallback time to appear if there were one
        $d=(Get-Date).AddSeconds(60); while((Get-Date) -lt $d -and -not (Per-User-Copies).Count){ Start-Sleep -Seconds 3 }
    }
    $after=Install-Fingerprint
    $r.install_unchanged=(($before -join "`n") -eq ($after -join "`n"))
    $r.version_before=$versionBefore; $r.version_after=Version
    $r.per_user_copies=(Per-User-Copies)
    $r.requests=Req
    $r.consent_kills=@(Get-Content 'C:\Users\Public\consent-kills.txt' -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
    # the other way to cancel: the prompt on the user's desktop answered with a key press through the signed worker
    $r.consent_answers=@(Get-Content 'C:\Users\Public\consent-answers.txt' -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
    Step 'checked' @{alive=$r.app_alive_same_pid;unchanged=$r.install_unchanged;per_user=$r.per_user_copies;kills=$r.consent_kills.Count;answers=$r.consent_answers.Count}
    if(-not $r.consent_kills.Count -and -not ($r.consent_answers | Where-Object { $_ -match 'answered the prompt' })){ throw 'nothing saw a consent prompt, so nothing was cancelled (run cancel-run.ps1 start or start-clicker first)' }
    if($Build -eq 'new'){
        if($r.message -notmatch 'Copilot was not changed'){ throw "unexpected message: $($r.message)" }
        if(-not $r.app_alive_same_pid){ throw 'the app did not keep running' }
        if(-not $r.install_unchanged){ throw 'the installed files changed' }
        if($r.per_user_copies.Count){ throw 'a per-user copy appeared' }
    }
    $r.ok=$true
} catch { $r.ok=$false; $r.error=$_.Exception.Message; Step 'FAILED' @{error=$_.Exception.Message}; Shot 'failed' }
finally {
    Step 'cleanup begins'
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath.StartsWith($install) -or $_.ExecutablePath -like "$env:LOCALAPPDATA\Programs\Copilot\*") })){ try{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }catch{} }
    Step 'app processes stopped'
    if($server -and -not $server.HasExited){ $server.Kill() }
    Step 'update server stopped'
    $r.steps=$script:steps; $r.work=$work
    try { $json=$r | ConvertTo-Json -Depth 4; Step 'json built'; [IO.File]::WriteAllText($Result,$json,(New-Object Text.UTF8Encoding($false))); [IO.File]::WriteAllText((Join-Path $work 'result.json'),$json,(New-Object Text.UTF8Encoding($false))) } catch { Write-Output ('result not written: '+$_.Exception.Message) }
}
if(-not $r.ok){ exit 1 }
