param(
    [Parameter(Mandatory=$true)][ValidateSet('start','stop','policy-default','policy-nosecure','start-clicker','stop-clicker')][string]$Action,
    [int]$Minutes=20,
    [ValidateSet('escape','return','alt+y')][string]$Key='escape'
)
# Run as SYSTEM (SSM). `policy-default` puts the UAC policy back to the default consent prompt (ConsentPromptBehaviorAdmin 5,
# ConsentPromptBehaviorUser 3) and makes sure ICUAudit is an administrator; restart the host after that for the group
# change. `start` registers a SYSTEM scheduled task that kills consent.exe (the elevation prompt's UI) whenever it
# appears and logs each kill with a time to C:\Users\Public\consent-kills.txt; `stop` removes it. The scheduled task
# survives the SSM command; it stops itself after -Minutes.
$ErrorActionPreference='Stop'
$k='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
if($Action -eq 'policy-default'){
    Set-ItemProperty $k ConsentPromptBehaviorAdmin 5 -Type DWord
    Set-ItemProperty $k ConsentPromptBehaviorUser 3 -Type DWord
    if(-not (Get-LocalGroupMember Administrators | Where-Object { $_.Name -like '*\ICUAudit' })){ Add-LocalGroupMember -Group Administrators -Member ICUAudit }
    "ConsentPromptBehaviorAdmin: " + (Get-ItemProperty $k).ConsentPromptBehaviorAdmin
    Set-ItemProperty $k PromptOnSecureDesktop 1 -Type DWord
    "ConsentPromptBehaviorUser: " + (Get-ItemProperty $k).ConsentPromptBehaviorUser
    "ICUAudit in Administrators: " + [bool](Get-LocalGroupMember Administrators | Where-Object { $_.Name -like '*\ICUAudit' })
    return
}
if($Action -eq 'policy-nosecure'){
    # The consent prompt on the user's own desktop (a policy enterprises use), so it is a window a key press can answer.
    Set-ItemProperty $k ConsentPromptBehaviorAdmin 5 -Type DWord
    Set-ItemProperty $k ConsentPromptBehaviorUser 3 -Type DWord
    Set-ItemProperty $k PromptOnSecureDesktop 0 -Type DWord
    if(-not (Get-LocalGroupMember Administrators | Where-Object { $_.Name -like '*\ICUAudit' })){ Add-LocalGroupMember -Group Administrators -Member ICUAudit }
    "PromptOnSecureDesktop: " + (Get-ItemProperty $k).PromptOnSecureDesktop
    return
}
if($Action -eq 'start-clicker'){
    # as ICUAudit (interactive, normal integrity): answers the prompt through the signed worker (consent-answer.ps1)
    Remove-Item 'C:\Users\Public\consent-answers.txt','C:\Users\Public\consent-*.png' -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force 'C:\Program Files\icu-clicker' | Out-Null
    Copy-Item 'C:\Program Files\Copilot\cua-driver-uia.exe' 'C:\Program Files\icu-clicker\cua-driver-uia.exe' -Force
    $script=Join-Path $PSScriptRoot 'consent-answer.ps1'
    $taskAction=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`" -Minutes $Minutes -Key $Key"
    $principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\ICUAudit" -LogonType Interactive -RunLevel Limited
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes ($Minutes+2)) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName ICUConsentClicker -Action $taskAction -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask ICUConsentClicker
    'consent clicker started'
    return
}
if($Action -eq 'stop-clicker'){
    Stop-ScheduledTask ICUConsentClicker -ErrorAction SilentlyContinue
    Unregister-ScheduledTask ICUConsentClicker -Confirm:$false -ErrorAction SilentlyContinue
    'consent clicker removed'
    if(Test-Path 'C:\Users\Public\consent-answers.txt'){ Get-Content 'C:\Users\Public\consent-answers.txt' }
    return
}
if($Action -eq 'start'){
    Remove-Item 'C:\Users\Public\consent-kills.txt' -ErrorAction SilentlyContinue
    $loop = "`$end=(Get-Date).AddMinutes($Minutes); while((Get-Date) -lt `$end){ foreach(`$p in @(Get-Process consent -ErrorAction SilentlyContinue)){ try{ Stop-Process -Id `$p.Id -Force; (Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff')+' killed consent.exe pid '+`$p.Id | Add-Content 'C:\Users\Public\consent-kills.txt' }catch{} }; Start-Sleep -Milliseconds 100 }"
    $taskAction=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -ExecutionPolicy Bypass -Command "'+$loop+'"')
    $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes ($Minutes+2)) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName ICUConsentKiller -Action $taskAction -User 'SYSTEM' -RunLevel Highest -Settings $settings -Force | Out-Null
    Start-ScheduledTask ICUConsentKiller
    'consent killer started'
}
if($Action -eq 'stop'){
    Stop-ScheduledTask ICUConsentKiller -ErrorAction SilentlyContinue
    Unregister-ScheduledTask ICUConsentKiller -Confirm:$false -ErrorAction SilentlyContinue
    'consent killer removed'
    if(Test-Path 'C:\Users\Public\consent-kills.txt'){ Get-Content 'C:\Users\Public\consent-kills.txt' }
}
