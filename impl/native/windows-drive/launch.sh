#!/usr/bin/env bash
# Launch a Windows Server 2022 EC2 instance that runs eval7.ps1 at interactive logon, then poll results via SSM.
#   launch.sh <presigned-exe-url>       -> prints INSTANCE_ID; results land in C:\cua\results7.json
#   launch.sh --fetch <instance-id>     -> fetch results/logs via SSM
#   launch.sh --rerun <instance-id> <presigned-exe-url>  -> push the current eval7.ps1/fixture.ps1 and start the logon task again
#   launch.sh --terminate <instance-id>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
AMI=${AMI:-ami-05eea18612b5be529}   # Windows_Server-2022-English-Full-Base-2026.08.12 (us-west-2)
PROFILE=${PROFILE:-cross-os-win-verify-ssm-role}
TYPE=${TYPE:-t3.large}
case "${1:-}" in
  --fetch)
    ID=$2
    CMD=$(aws ssm send-command --instance-ids "$ID" --document-name AWS-RunPowerShellScript --parameters 'commands=["if(Test-Path C:\cua\results7.json){Get-Content C:\cua\results7.json}else{\"NO_RESULTS\"; Get-Content C:\cua\bootstrap7.txt -EA SilentlyContinue; Get-Content C:\cua\error7.txt -EA SilentlyContinue; Get-Content C:\cua\eval7.log -Tail 40 -EA SilentlyContinue}"]' --query Command.CommandId --output text)
    sleep 6
    aws ssm get-command-invocation --command-id "$CMD" --instance-id "$ID" --query StandardOutputContent --output text
    exit 0;;
  --rerun)
    ID=$2; URL=$3
    EVAL_B64=$(EXE_URL="$URL" python3 -c 'import os,sys; sys.stdout.write(open(sys.argv[1]).read().replace("__EXE_URL__", os.environ["EXE_URL"]))' "$HERE/eval7.ps1" | base64 | tr -d '\n')
    FIX_B64=$(base64 < "$HERE/fixture.ps1" | tr -d '\n')
    CMD=$(aws ssm send-command --instance-ids "$ID" --document-name AWS-RunPowerShellScript --parameters "commands=[\"[IO.File]::WriteAllBytes('C:\\cua\\eval7.ps1',[Convert]::FromBase64String('$EVAL_B64')); [IO.File]::WriteAllBytes('C:\\cua\\fixture.ps1',[Convert]::FromBase64String('$FIX_B64')); Remove-Item C:\\cua\\results7.json,C:\\cua\\error7.txt,C:\\cua\\eval7.done -EA SilentlyContinue; Start-ScheduledTask -TaskName CuaEval7; 'rerun-started'\"]" --query Command.CommandId --output text)
    sleep 5; aws ssm get-command-invocation --command-id "$CMD" --instance-id "$ID" --query '[Status,StandardOutputContent,StandardErrorContent]' --output text
    exit 0;;
  --terminate)
    aws ec2 terminate-instances --instance-ids "$2" --query 'TerminatingInstances[].CurrentState.Name' --output text; exit 0;;
esac
URL=$1
PW="Cua!$(openssl rand -hex 10)Aa9"
umask 077; echo "$PW" > "$HERE/.adminpw"   # gitignored; only needed for RDP debugging
FIX_B64=$(base64 < "$HERE/fixture.ps1" | tr -d '\n')
# user data is capped at 16 KB, so the eval travels gzip+base64
EVAL_GZ64=$(EXE_URL="$URL" python3 -c 'import os,sys; sys.stdout.write(open(sys.argv[1]).read().replace("__EXE_URL__", os.environ["EXE_URL"]))' "$HERE/eval7.ps1" | gzip -9 | base64 | tr -d '\n')
UD=$(cat <<PS
<powershell>
\$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
[IO.File]::WriteAllBytes("C:\cua\fixture.ps1",[Convert]::FromBase64String("$FIX_B64"))
\$gz=[Convert]::FromBase64String("$EVAL_GZ64"); \$ms=New-Object IO.MemoryStream(,\$gz); \$g=New-Object IO.Compression.GzipStream(\$ms,[IO.Compression.CompressionMode]::Decompress); \$out=New-Object IO.MemoryStream; \$g.CopyTo(\$out); [IO.File]::WriteAllBytes("C:\cua\eval7.ps1",\$out.ToArray())
\$pw="$PW"
net user Administrator "\$pw" | Out-Null
net user Administrator /active:yes | Out-Null
\$rk="HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"
Set-ItemProperty \$rk AutoAdminLogon "1"; Set-ItemProperty \$rk DefaultUserName "Administrator"; Set-ItemProperty \$rk DefaultPassword "\$pw"; Set-ItemProperty \$rk DefaultDomainName \$env:COMPUTERNAME
\$act=New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -File C:\cua\eval7.ps1"
\$trg=New-ScheduledTaskTrigger -AtLogOn
\$pr=New-ScheduledTaskPrincipal -UserId "Administrator" -RunLevel Highest -LogonType Interactive
\$set=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
Register-ScheduledTask -TaskName "CuaEval7" -Action \$act -Trigger \$trg -Principal \$pr -Settings \$set -Force | Out-Null
"bootstrap7-done \$(Get-Date -Format o)" | Out-File C:\cua\bootstrap7.txt
shutdown /r /t 8 /f
</powershell>
PS
)
ID=$(aws ec2 run-instances --image-id "$AMI" --instance-type "$TYPE" --iam-instance-profile "Name=$PROFILE" \
  --user-data "$UD" --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=icu-win-lineage-drive}]' \
  --block-device-mappings '[{"DeviceName":"/dev/sda1","Ebs":{"VolumeSize":40,"VolumeType":"gp3"}}]' \
  --query 'Instances[0].InstanceId' --output text)
echo "INSTANCE_ID=$ID"
