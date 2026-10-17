param([Parameter(Mandatory=$true)][ValidateSet('standard-deny','admin-silent','show')][string]$Mode)
# The audit user is a standard user. A per-machine update needs elevation, which on a headless host has no one to
# answer the prompt, so the two cases the policy can express are set explicitly (and reported):
#   standard-deny : ICUAudit is a standard user, an elevation request is refused without a prompt
#                   (ConsentPromptBehaviorUser 0). What a standard user without admin credentials meets.
#   admin-silent  : ICUAudit is an administrator (filtered token, still medium integrity), elevation is granted
#                   without a prompt (ConsentPromptBehaviorAdmin 0). What an administrator who clicks Yes meets.
# Group changes apply at the next logon: restart the host after changing the mode.
$k='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
if($Mode -eq 'standard-deny'){
    Remove-LocalGroupMember -Group 'Administrators' -Member 'ICUAudit' -ErrorAction SilentlyContinue
    Set-ItemProperty $k ConsentPromptBehaviorUser 0 -Type DWord
    Set-ItemProperty $k ConsentPromptBehaviorAdmin 5 -Type DWord
}
if($Mode -eq 'admin-silent'){
    Add-LocalGroupMember -Group 'Administrators' -Member 'ICUAudit' -ErrorAction SilentlyContinue
    Set-ItemProperty $k ConsentPromptBehaviorAdmin 0 -Type DWord
}
"mode $Mode"
"ICUAudit in Administrators: " + [bool](Get-LocalGroupMember Administrators | Where-Object { $_.Name -like '*\ICUAudit' })
"ConsentPromptBehaviorAdmin: " + (Get-ItemProperty $k).ConsentPromptBehaviorAdmin
"ConsentPromptBehaviorUser: " + (Get-ItemProperty $k).ConsentPromptBehaviorUser
"EnableLUA: " + (Get-ItemProperty $k).EnableLUA
