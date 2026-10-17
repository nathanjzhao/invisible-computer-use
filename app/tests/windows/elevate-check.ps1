param(
    # The app's library test executable, built by build.ps1 with the app's manifest embedded
    # (C:\ICUAudit\source-<rev>\app\src-tauri\target\debug\deps\app_lib-*.exe).
    [Parameter(Mandatory=$true)][string]$TestExe,
    [string]$Out='C:\Users\Public\elevate-check.json'
)
# Run as SYSTEM. Runs the app's permissions tests as two kinds of signed-in user and checks that
# the update guard reads the user's ability to elevate from the token:
#   ICUAudit  a member of Administrators, normal (filtered) token  -> can elevate  (ICU_EXPECT_CAN_ELEVATE=1)
#   ICUStd    a local standard user created here                    -> cannot       (ICU_EXPECT_CAN_ELEVATE=0)
# The test only compares when ICU_EXPECT_CAN_ELEVATE is set, so each run also has to pass without
# the variable (it checks nothing) and must FAIL when the variable names the wrong answer: that
# crossed run is what shows the check can fail.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Web
$exe=(Get-Item $TestExe).FullName
$report=[ordered]@{exe=$exe}
$std='ICUStd'
$password=[System.Web.Security.Membership]::GeneratePassword(20,4)+'aA1!'
if(-not (Get-LocalUser $std -ErrorAction SilentlyContinue)){
    New-LocalUser -Name $std -Password (ConvertTo-SecureString $password -AsPlainText -Force) -AccountNeverExpires -PasswordNeverExpires | Out-Null
    Add-LocalGroupMember -Group 'Users' -Member $std -ErrorAction SilentlyContinue
    # Only for the batch-logon right a scheduled task needs; it grants no administrative rights and no elevation.
    Add-LocalGroupMember -Group 'Performance Log Users' -Member $std -ErrorAction SilentlyContinue
} else {
    Set-LocalUser -Name $std -Password (ConvertTo-SecureString $password -AsPlainText -Force)
}
$report.std_in_administrators=[bool](Get-LocalGroupMember -Group 'Administrators' | Where-Object {$_.Name -like "*\$std"})
# The test executable and its folder have to be readable by the standard user.
icacls (Split-Path $exe) /grant "${std}:(OI)(CI)RX" /T /C | Out-Null
function Run-As([string]$user,[string]$expect,[bool]$withPassword) {
    $log="C:\Users\Public\elevate-$user-$expect.txt"
    Remove-Item $log -ErrorAction SilentlyContinue
    $envPart=$(if($expect -eq 'none'){''}else{"set ICU_EXPECT_CAN_ELEVATE=$expect&& "})
    # cmd /c strips the outer quotes of a string that starts with a quote.
    $cmd="/c `"$envPart`"$exe`" permissions --nocapture > `"$log`" 2>&1`""
    $action=New-ScheduledTaskAction -Execute 'cmd.exe' -Argument $cmd
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 5) -AllowStartIfOnBatteries
    $name="ICUElevate$user"
    if($withPassword){
        # A password logon runs a task for a user that is not signed in (the standard user is not).
        Register-ScheduledTask -TaskName $name -Action $action -Settings $settings -User "$env:COMPUTERNAME\$user" -Password $password -RunLevel Limited -Force | Out-Null
    } else {
        $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\$user" -LogonType Interactive -RunLevel Limited
        Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    }
    $started=Get-Date
    Start-ScheduledTask $name
    $deadline=(Get-Date).AddSeconds(120)
    do { Start-Sleep -Seconds 1; $info=Get-ScheduledTaskInfo $name } while(((Get-ScheduledTask $name).State -eq 'Running' -or $info.LastRunTime -lt $started) -and (Get-Date) -lt $deadline)
    $code=$info.LastTaskResult
    Unregister-ScheduledTask $name -Confirm:$false
    $text=$(if(Test-Path $log){Get-Content $log -Raw}else{''})
    $line=($text -split "`n" | Where-Object {$_ -match 'test result|the_signed_in_users'} | ForEach-Object {$_.Trim()}) -join ' | '
    return [ordered]@{user=$user;expect=$expect;exit=$code;passed=($code -eq 0);summary=$line}
}
$rows=@()
$rows+=Run-As 'ICUAudit' '1' $false
$rows+=Run-As 'ICUAudit' 'none' $false
$rows+=Run-As $std '0' $true
$rows+=Run-As $std 'none' $true
# The crossed runs must fail: the check can tell the two users apart.
$rows+=Run-As 'ICUAudit' '0' $false
$rows+=Run-As $std '1' $true
$report.runs=$rows
$report.ok=($rows[0].passed -and $rows[1].passed -and $rows[2].passed -and $rows[3].passed -and (-not $rows[4].passed) -and (-not $rows[5].passed))
Remove-LocalUser $std
$json=$report | ConvertTo-Json -Depth 5
$json | Set-Content $Out -Encoding UTF8
$json
