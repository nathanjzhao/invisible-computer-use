param(
    [Parameter(Mandatory=$true)][string]$IcuExe,
    [Parameter(Mandatory=$true)][string]$Driver,
    [string]$Out='C:\Users\Public\skypipe-access.json'
)
# Who can open the `icu sky-pipe` named pipe. Run as SYSTEM (SSM). The server runs as the logged-on standard user
# ICUAudit in the interactive session; the clients are ICUAudit (must open), a second standard local user
# ICUOther (must be refused: the DACL names the server's user and SYSTEM), and SYSTEM (allowed by the DACL).
# Then a squatter: ICUOther owns the name first and the server must refuse to come up on it, leaving the
# squatter with nothing to read.
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$pass='Aa1!'+[guid]::NewGuid().ToString('N')
if(-not (Get-LocalUser ICUOther -ErrorAction SilentlyContinue)){ New-LocalUser -Name ICUOther -Password (ConvertTo-SecureString $pass -AsPlainText -Force) -PasswordNeverExpires | Out-Null; Add-LocalGroupMember -Group Users -Member ICUOther -ErrorAction SilentlyContinue }
else { Set-LocalUser ICUOther -Password (ConvertTo-SecureString $pass -AsPlainText -Force) }
# a second ADMINISTRATOR (elevated): the default DACL of a pipe gives Administrators full control, ours does not. A standard
# user is refused either way, so this is the actor that tells the explicit DACL from the default one.
if(-not (Get-LocalUser ICUAdmin2 -ErrorAction SilentlyContinue)){ New-LocalUser -Name ICUAdmin2 -Password (ConvertTo-SecureString $pass -AsPlainText -Force) -PasswordNeverExpires | Out-Null; Add-LocalGroupMember -Group Administrators -Member ICUAdmin2 -ErrorAction SilentlyContinue }
else { Set-LocalUser ICUAdmin2 -Password (ConvertTo-SecureString $pass -AsPlainText -Force) }
# a standard local user has no "log on as a batch job" right, which a scheduled task needs: grant it to this one account only
$sid=(@('ICUOther','ICUAdmin2') | ForEach-Object { (New-Object Security.Principal.NTAccount("$env:COMPUTERNAME\$_")).Translate([Security.Principal.SecurityIdentifier]).Value }) -join ',*' 
secedit /export /cfg "$env:TEMP\ur.inf" /areas USER_RIGHTS | Out-Null
$inf=Get-Content "$env:TEMP\ur.inf"; $inf=$inf | ForEach-Object { if($_ -match '^SeBatchLogonRight\s*='){ $_+",*$sid" } else { $_ } }
Set-Content "$env:TEMP\ur2.inf" $inf -Encoding Unicode
secedit /configure /db "$env:TEMP\ur.sdb" /cfg "$env:TEMP\ur2.inf" /areas USER_RIGHTS | Out-Null
$work='C:\Users\Public\skypipe-access'; if(Test-Path $work){ cmd /c "rd /s /q $work" }; New-Item -ItemType Directory $work | Out-Null
icacls $work /grant 'Users:(OI)(CI)M' | Out-Null
$r=[ordered]@{icu=$IcuExe;icu_sha256=(Get-FileHash $IcuExe -Algorithm SHA256).Hash}
function RunAs([string]$user,[string]$password,[string]$script,[int]$wait=60,[string]$level='Limited'){
    $name='ICUSky'+[guid]::NewGuid().ToString('N').Substring(0,6)
    $action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`""
    $set=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $wait) -AllowStartIfOnBatteries
    if($password){ Register-ScheduledTask -TaskName $name -Action $action -Settings $set -User "$env:COMPUTERNAME\$user" -Password $password -RunLevel $level -Force | Out-Null }
    else { Register-ScheduledTask -TaskName $name -Action $action -Settings $set -Principal (New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\$user" -LogonType Interactive -RunLevel Limited) -Force | Out-Null }
    Start-ScheduledTask $name
    $d=(Get-Date).AddSeconds($wait+10); Start-Sleep 2
    while((Get-ScheduledTask $name).State -eq 'Running' -and (Get-Date) -lt $d){ Start-Sleep 1 }
    Unregister-ScheduledTask $name -Confirm:$false
}
function Attempt([string]$label,[string]$user,[string]$password,[string]$pipe,[string]$level='Limited'){
    $o="$work\open-$label.json"; Remove-Item $o -ErrorAction SilentlyContinue
    if($user -eq 'SYSTEM'){ & "$here\sky-open.ps1" -Name $pipe -Out $o }
    else {
        $s="$work\run-$label.ps1"; "& '$here\sky-open.ps1' -Name '$pipe' -Out '$o'" | Set-Content $s
        RunAs $user $password $s 40 $level
    }
    if(Test-Path $o){ return (Get-Content $o -Raw | ConvertFrom-Json) } else { return @{opened=$null;error='no result file'} }
}
$icuProc=$null
try {
    # ---- the server, as ICUAudit, in the interactive session
    $pipe='icu-sky-access-'+[guid]::NewGuid().ToString('N').Substring(0,8)
    $server="$work\server.ps1"
    @"
`$env:ICU_DRIVER='$Driver'; `$env:ICU_HOME='$work\icu-home'; `$env:ICU_REQUIRE_APP_APPROVAL='0'
`$p=Start-Process '$IcuExe' -ArgumentList 'sky-pipe','--socket','$pipe' -PassThru -WindowStyle Hidden -RedirectStandardError '$work\server.err'
`$p.Id | Set-Content '$work\server.pid'
`$p.WaitForExit(120000) | Out-Null
"@ | Set-Content $server
    $sname='ICUSkyServer'
    Register-ScheduledTask -TaskName $sname -Action (New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$server`"") -Principal (New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel Limited) -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 300)) -Force | Out-Null
    Start-ScheduledTask $sname
    $d=(Get-Date).AddSeconds(30); while(-not (Test-Path "\\.\pipe\$pipe") -and (Get-Date) -lt $d){ Start-Sleep -Milliseconds 200 }
    $r.server_pipe_exists=Test-Path "\\.\pipe\$pipe"
    $r.server_owner=(Get-CimInstance Win32_Process -Filter "ProcessId=$(Get-Content $work\server.pid)" | ForEach-Object { (Invoke-CimMethod -InputObject $_ -MethodName GetOwner | ForEach-Object { "$($_.Domain)\$($_.User)" }) })
    $r.as_icuaudit=Attempt 'icuaudit' 'ICUAudit' '' $pipe
    $r.as_icuother=Attempt 'icuother' 'ICUOther' $pass $pipe
    $r.as_admin2_elevated=Attempt 'admin2' 'ICUAdmin2' $pass $pipe 'Highest'
    $r.as_system=Attempt 'system' 'SYSTEM' '' $pipe
    # the server still answers its owner after the refused attempt
    $r.as_icuaudit_again=Attempt 'icuaudit2' 'ICUAudit' '' $pipe
    Stop-Process -Id (Get-Content "$work\server.pid") -Force -ErrorAction SilentlyContinue
    Start-Sleep 2
    # ---- the squatter: ICUOther owns the name first
    $pipe2='icu-sky-squat-'+[guid]::NewGuid().ToString('N').Substring(0,8)
    $squat="$work\squat.ps1"
    @"
`$s=New-Object IO.Pipes.NamedPipeServerStream('$pipe2',[IO.Pipes.PipeDirection]::InOut,4,[IO.Pipes.PipeTransmissionMode]::Byte)
'listening' | Set-Content '$work\squat.state'
`$s.WaitForConnection()
'GOT A CONNECTION' | Set-Content '$work\squat.state'
`$buf=New-Object byte[] 4096; `$n=`$s.Read(`$buf,0,4096); "read `$n bytes" | Add-Content '$work\squat.state'
"@ | Set-Content $squat
    $qn='ICUSkySquat'
    Register-ScheduledTask -TaskName $qn -Action (New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$squat`"") -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 120)) -User "$env:COMPUTERNAME\ICUOther" -Password $pass -RunLevel Limited -Force | Out-Null
    Start-ScheduledTask $qn
    # (Test-Path on a pipe connects to it, and would use up the squatter's one connection: wait on its own state file)
    $d=(Get-Date).AddSeconds(120); while(-not (Test-Path "$work\squat.state") -and (Get-Date) -lt $d){ Start-Sleep -Milliseconds 300 }
    $r.squatter_listening=(Test-Path "$work\squat.state") -and ((Get-Content "$work\squat.state") -join "" -eq "listening")
    # the server asked to come up on the squatted name, as ICUAudit in the interactive session (the layer needs a desktop to start its driver)
    $s2="$work\server2.ps1"
    @"
`$env:ICU_DRIVER='$Driver'; `$env:ICU_HOME='$work\icu-home2'; `$env:ICU_REQUIRE_APP_APPROVAL='0'
`$i=New-Object Diagnostics.ProcessStartInfo '$IcuExe'; `$i.Arguments='sky-pipe --socket $pipe2'; `$i.UseShellExecute=`$false; `$i.RedirectStandardError=`$true; `$i.CreateNoWindow=`$true
`$p=[Diagnostics.Process]::Start(`$i); `$e=`$p.StandardError.ReadToEndAsync(); `$done=`$p.WaitForExit(25000)
if(-not `$done){ `$p.Kill() }
(@{exited=`$done;exit_code=`$(if(`$done){`$p.ExitCode});stderr=`$e.Result} | ConvertTo-Json) | Set-Content '$work\server2.json'
"@ | Set-Content $s2
    RunAs 'ICUAudit' '' $s2 60
    $r.server_on_squatted_name=$(if(Test-Path "$work\server2.json"){ Get-Content "$work\server2.json" -Raw | ConvertFrom-Json } else { @{exited=$null;exit_code=$null;stderr='no result'} })
    # a client of the squatted name reaches only the squatter: nothing the server would have said
    $r.squatter_state=(Get-Content "$work\squat.state" -ErrorAction SilentlyContinue) -join '; '
    Unregister-ScheduledTask $qn -Confirm:$false -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -match 'squat.ps1' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    $icuOk = $r.as_icuaudit.opened -and $r.as_icuaudit_again.opened -and $r.as_system.opened
    $other = (-not $r.as_icuother.opened) -and (-not $r.as_admin2_elevated.opened)
    $r.ok = [bool]($icuOk -and $other -and $r.server_on_squatted_name.exited -and $r.server_on_squatted_name.exit_code -ne 0 -and $r.server_on_squatted_name.stderr -match 'already being served' -and $r.squatter_state -notmatch 'GOT A CONNECTION')
} catch { $r.ok=$false; $r.error=$_.Exception.Message }
finally {
    Unregister-ScheduledTask ICUSkyServer -Confirm:$false -ErrorAction SilentlyContinue
    Get-Process -Name icu -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $IcuExe } | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-LocalUser ICUOther -ErrorAction SilentlyContinue; Remove-LocalUser ICUAdmin2 -ErrorAction SilentlyContinue
    $r | ConvertTo-Json -Depth 8 | Set-Content $Out -Encoding UTF8
}
if(-not $r.ok){ exit 1 }
