param([Parameter(Mandatory=$true)][string]$Script,[string]$Arguments='',[int]$TimeoutSeconds=300,[switch]$Hidden,[switch]$Detached)
# Run one harness script in the auto-logged-on standard user's interactive
# session. SSM itself runs as SYSTEM in session 0, which has no desktop.
$ErrorActionPreference='Stop'
$user="$env:COMPUTERNAME\ICUAudit"
$existing=Get-ScheduledTask -TaskName ICUAuditUser -ErrorAction SilentlyContinue
if($existing -and $existing.State -eq 'Running'){throw 'another owned interactive test is still running'}
$latest='C:\Users\ICUAudit\AppData\Local\ICUAudit\latest.json'
$output='C:\Users\Public\icu-user-task.txt'
Remove-Item $latest,$output -ErrorAction SilentlyContinue
# A parameter-binding error or an uncaught throw stops the script's statement
# but not this command line, so the old `...; exit $LASTEXITCODE` exited 0
# with an empty output file. Report it with its message and a nonzero code.
$command="try { & '$Script' $Arguments *> '$output'; exit `$LASTEXITCODE } catch { 'as-user: script did not run to completion: '+`$_+' '+`$_.InvocationInfo.PositionMessage | Out-File -Append -Encoding utf8 '$output'; exit 3 }"
# Encode the command so paths with spaces survive the task scheduler's command line.
$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
$action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile $(if($Hidden){"-WindowStyle Hidden "})-ExecutionPolicy Bypass -EncodedCommand $encoded"
$principal=New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSeconds) -AllowStartIfOnBatteries
Register-ScheduledTask -TaskName ICUAuditUser -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask ICUAuditUser
$deadline=(Get-Date).AddSeconds($TimeoutSeconds+30)
Start-Sleep -Seconds 2
if($Detached){
    @{task='ICUAuditUser';state=[string](Get-ScheduledTask ICUAuditUser).State;output=$output;timeout_seconds=$TimeoutSeconds}|ConvertTo-Json
    return
}
while((Get-ScheduledTask ICUAuditUser).State -eq 'Running' -and (Get-Date) -lt $deadline){Start-Sleep -Seconds 2}
$info=Get-ScheduledTaskInfo ICUAuditUser
Unregister-ScheduledTask ICUAuditUser -Confirm:$false
"task_result=$($info.LastTaskResult)"
if(Test-Path $output){Get-Content $output}
if(Test-Path $latest){Get-Content $latest}
if($info.LastTaskResult -ne 0){exit 1}
