param(
    [Parameter(Mandatory=$true)][string]$Driver,
    [Parameter(Mandatory=$true)][ValidateSet('sched','self')][string]$Start,
    # run the user script in a hidden console (the default shows the console window a scheduled powershell.exe gets)
    [switch]$Hidden,
    # fg-probe.ps1 drives the driver; fg-raw.ps1 uses Windows' own calls only
    [ValidateSet('fg-probe.ps1','fg-raw.ps1')][string]$Probe='fg-probe.ps1',
    [string]$Label='',
    [string]$Out='C:\Users\Public\gaps5-fg.json'
)
# Run as SYSTEM (SSM). Starts Notepad at the interactive user's own (normal) integrity the way gaps4-realrun.ps1 does
# (-Start sched) or leaves that to the user script (-Start self), then runs gaps5\fg-probe.ps1 as the interactive user.
$ErrorActionPreference='Stop'
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$taskName='ICUGaps5Notepad'
if($Start -eq 'sched'){
    $action=New-ScheduledTaskAction -Execute 'C:\Windows\System32\notepad.exe'
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries) -Force | Out-Null
    Start-ScheduledTask $taskName
    Start-Sleep -Seconds 4
}
Remove-Item $Out -ErrorAction SilentlyContinue
try {
    $probeArgs=$(if($Probe -eq 'fg-raw.ps1'){"-Label '$Label' -Out '$Out'"}else{"-Driver '$Driver' -Start $Start -Label '$Label' -Out '$Out'"})
    $asUser=@{Script=(Join-Path $here $Probe);Arguments=$probeArgs;TimeoutSeconds=200}
    if($Hidden){ $asUser.Hidden=$true }
    & (Join-Path (Split-Path -Parent $here) 'as-user.ps1') @asUser
} finally {
    if($Start -eq 'sched'){
        Get-Process notepad -ErrorAction SilentlyContinue | Where-Object {$_.SessionId -ne 0} | Stop-Process -Force -ErrorAction SilentlyContinue
        Unregister-ScheduledTask $taskName -Confirm:$false -ErrorAction SilentlyContinue
    }
}
