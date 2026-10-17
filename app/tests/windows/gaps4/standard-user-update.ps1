param(
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Harness='C:\ICUAudit\harness',
    [string]$Result='C:\Users\Public\gaps4-standard-user.json'
)
# Item 4: the real updater UI as a standard user.
# Run through as-user.ps1 as the host's logged-on user (ICUAudit) after `gaps2\uacset.ps1 standard-deny` and a restart,
# so the user is NOT in Administrators. That is a real standard user in a real interactive session (a second local user
# would need a second interactive session, which the host does not have). The installed app is the all-users 0.1.0 build
# under C:\Program Files\Copilot that contains the update check, and the local update server (upd-server.js, throwaway
# key) offers 0.2.0. The script drives the app's own toast through UIA and records:
#   * the message the toast shows (it must tell the person to ask an administrator),
#   * that there is no Install button left to press,
#   * that the update server saw the manifest request and NO request for the archive (nothing downloaded),
#   * that the app kept running (same pid), the installed files are byte for byte what they were, and no per-user copy appeared.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$install='C:\Program Files\Copilot'; $exe=Join-Path $install 'app.exe'
$work=Join-Path $env:LOCALAPPDATA ('ICUAudit\gaps4-standard-'+(Get-Date -Format 'HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
. (Join-Path $PSScriptRoot 'lib.ps1')
$expectedText='Ask an administrator'
$r=[ordered]@{scenario='standard-user-all-users-update';user=$env:USERNAME}
$server=$null;$app=$null
try {
    if(Is-Admin-Member){ throw "$env:USERNAME is a member of Administrators: run gaps2\uacset.ps1 standard-deny and restart first" }
    if((New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){ throw 'running elevated' }
    $before=Install-Fingerprint
    if(-not $before.Count){ throw "nothing installed at $install" }
    $versionBefore=Version
    Step 'start' @{user=$env:USERNAME;is_admin_member=(Is-Admin-Member);version=$versionBefore;files=$before.Count;per_user_copies=(Per-User-Copies)}
    Copy-Item (Join-Path $Dir 'manifest-good.json') (Join-Path $Dir 'manifest.json') -Force
    Remove-Item (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue
    $server=Start-UpdateServer
    $offered=(Get-Content (Join-Path $Dir 'manifest.json') -Raw | ConvertFrom-Json).version
    $app=Start-App
    $oldPid=$app.Id
    Step 'app started' @{pid=$oldPid;server_offers=$offered}
    # The toast first says "Update 0.2.0 is ready" and then, once the app has asked the backend, replaces that with the
    # reason. Either may be seen first; wait for the reason.
    $msg=Wait-For { Find-Name $expectedText $false } 40 'the ask-an-administrator message did not appear'
    $text=$msg.Current.Name
    Step 'message shown' @{text=$text}
    Shot 'standard-user-message'
    Start-Sleep -Seconds 5
    $button=Find-Name 'Install & restart'
    $reqs=Req
    $files=@($reqs | Where-Object { $_ -match '/files/' })
    $after=Install-Fingerprint
    $r.message=$text
    $r.install_button_present=[bool]$button
    $r.requests=$reqs
    $r.archive_requests=$files.Count
    $app.Refresh(); $r.app_alive_same_pid=(-not $app.HasExited)
    $r.install_unchanged=(($before -join "`n") -eq ($after -join "`n"))
    $r.version_before=$versionBefore; $r.version_after=Version
    $r.per_user_copies=(Per-User-Copies)
    $r.toast_texts=@(Texts | Where-Object { $_ -match 'pdate' })
    Step 'checked' @{button=$r.install_button_present;archive_requests=$r.archive_requests;alive=$r.app_alive_same_pid;unchanged=$r.install_unchanged;per_user=$r.per_user_copies}
    $manifestSeen=@($reqs | Where-Object { $_ -match '/latest.json' }).Count
    $r.manifest_requests=$manifestSeen
    if($manifestSeen -lt 1){ throw 'the app never asked the update server, so the test says nothing' }
    if($text -notmatch 'not an administrator' -or $text -notmatch 'Ask an administrator'){ throw "unexpected message: $text" }
    if($r.install_button_present){ throw 'an Install button is still offered' }
    if($r.archive_requests){ throw 'the archive was downloaded' }
    if(-not $r.app_alive_same_pid){ throw 'the app did not keep running' }
    if(-not $r.install_unchanged){ throw 'the installed files changed' }
    if($r.per_user_copies.Count){ throw 'a per-user copy appeared' }
    $r.ok=$true
} catch { $r.ok=$false; $r.error=$_.Exception.Message; Step 'FAILED' @{error=$_.Exception.Message}; Shot 'failed' }
finally {
    foreach($p in @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($install) })){ try{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }catch{} }
    if($server -and -not $server.HasExited){ $server.Kill() }
    $r.steps=$script:steps; $r.work=$work
    try { $json=$r | ConvertTo-Json -Depth 4; [IO.File]::WriteAllText($Result,$json,(New-Object Text.UTF8Encoding($false))); [IO.File]::WriteAllText((Join-Path $work 'result.json'),$json,(New-Object Text.UTF8Encoding($false))) } catch { Write-Output ('result not written: '+$_.Exception.Message) }
}
if(-not $r.ok){ exit 1 }
