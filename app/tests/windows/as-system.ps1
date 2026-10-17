param([Parameter(Mandatory=$true)][string]$Script,[string]$Arguments='',[int]$TimeoutSeconds=7200)
# Run one harness script as SYSTEM in a scheduled task, outside the SSM
# command's lifetime (a package build outlasts one SSM invocation). Pass URLs
# and other quoted values through a file (build.ps1 -ParamsFile), never on
# this command line.
$ErrorActionPreference='Stop'
$output='C:\ICUAudit\icu-system-task.txt'
Remove-Item $output -ErrorAction SilentlyContinue
$action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -Command `"& '$Script' $Arguments *> '$output'; exit `$LASTEXITCODE`""
$settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSeconds) -AllowStartIfOnBatteries
Register-ScheduledTask -TaskName ICUAuditSystem -Action $action -User 'SYSTEM' -RunLevel Highest -Settings $settings -Force | Out-Null
Start-ScheduledTask ICUAuditSystem
"started $Script"
