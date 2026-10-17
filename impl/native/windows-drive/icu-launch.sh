#!/usr/bin/env bash
# Slice 12 Windows: launch an EC2 Windows Server box that runs icu-eval.ps1 (drives icu.exe on
# Notepad through MCP) at interactive logon. Bundles icu.exe + cua-driver.exe as one zip in S3.
#   icu-launch.sh <bundle-presigned-url>       -> prints INSTANCE_ID; results in C:\cua\icu-results.json
#   icu-launch.sh --fetch <instance-id>        -> fetch results/logs via SSM
#   icu-launch.sh --rerun <instance-id> <url>  -> push icu-eval.ps1 + rerun the logon task
#   icu-launch.sh --terminate <instance-id>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
AMI=${AMI:-ami-05eea18612b5be529}
PROFILE=${PROFILE:-cross-os-win-verify-ssm-role}
TYPE=${TYPE:-t3.large}
case "${1:-}" in
  --fetch)
    ID=$2
    CMD=$(aws ssm send-command --instance-ids "$ID" --document-name AWS-RunPowerShellScript --parameters 'commands=["if(Test-Path C:\cua\icu-results.json){Get-Content C:\cua\icu-results.json}else{\"NO_RESULTS\"; Get-Content C:\cua\icu-progress.log -Tail 25 -EA SilentlyContinue; Get-Content C:\cua\icu-error.txt -EA SilentlyContinue}"]' --query Command.CommandId --output text)
    sleep 9; aws ssm get-command-invocation --command-id "$CMD" --instance-id "$ID" --query StandardOutputContent --output text; exit 0 ;;
  --rerun)
    ID=$2; URL=$3
    EVAL_GZ64=$(EXE_URL="$URL" python3 -c 'import os,sys; sys.stdout.write(open(sys.argv[1]).read().replace("__EXE_URL__", os.environ["EXE_URL"]))' "$HERE/icu-eval.ps1" | gzip -9 | base64 | tr -d '\n')
    CMD=$(aws ssm send-command --instance-ids "$ID" --document-name AWS-RunPowerShellScript --parameters "commands=[\"\$gz=[Convert]::FromBase64String('$EVAL_GZ64'); \$ms=New-Object IO.MemoryStream(,\$gz); \$g=New-Object IO.Compression.GzipStream(\$ms,[IO.Compression.CompressionMode]::Decompress); \$o=New-Object IO.MemoryStream; \$g.CopyTo(\$o); [IO.File]::WriteAllBytes('C:\\cua\\icu-eval.ps1',\$o.ToArray()); Remove-Item C:\\cua\\icu-results.json,C:\\cua\\icu-error.txt,C:\\cua\\icu-eval.done,C:\\cua\\icu.exe,C:\\cua\\cua-driver.exe -EA SilentlyContinue; Start-ScheduledTask -TaskName CuaIcuEval; 'rerun-started'\"]" --query Command.CommandId --output text)
    echo "rerun: $(aws ssm get-command-invocation --command-id "$CMD" --instance-id "$ID" --query Status --output text 2>/dev/null || echo sent)"; exit 0 ;;
  --terminate)
    aws ec2 terminate-instances --instance-ids "$2" --query 'TerminatingInstances[0].CurrentState.Name' --output text; exit 0 ;;
esac
URL=$1
PW="Cua!$(openssl rand -hex 10)Aa9"
umask 077; echo "$PW" > "$HERE/.adminpw"
EVAL_GZ64=$(EXE_URL="$URL" python3 -c 'import os,sys; sys.stdout.write(open(sys.argv[1]).read().replace("__EXE_URL__", os.environ["EXE_URL"]))' "$HERE/icu-eval.ps1" | gzip -9 | base64 | tr -d '\n')
UD=$(cat <<PS
<powershell>
\$ErrorActionPreference="Continue"
New-Item -ItemType Directory -Force C:\cua | Out-Null
\$gz=[Convert]::FromBase64String("$EVAL_GZ64"); \$ms=New-Object IO.MemoryStream(,\$gz); \$g=New-Object IO.Compression.GzipStream(\$ms,[IO.Compression.CompressionMode]::Decompress); \$out=New-Object IO.MemoryStream; \$g.CopyTo(\$out); [IO.File]::WriteAllBytes("C:\cua\icu-eval.ps1",\$out.ToArray())
\$pw="$PW"
net user Administrator "\$pw" | Out-Null
net user Administrator /active:yes | Out-Null
\$rk="HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"
Set-ItemProperty \$rk AutoAdminLogon "1"; Set-ItemProperty \$rk DefaultUserName "Administrator"; Set-ItemProperty \$rk DefaultPassword "\$pw"; Set-ItemProperty \$rk DefaultDomainName \$env:COMPUTERNAME
\$act=New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -File C:\cua\icu-eval.ps1"
\$trg=New-ScheduledTaskTrigger -AtLogOn
\$pr=New-ScheduledTaskPrincipal -UserId "Administrator" -RunLevel Highest -LogonType Interactive
\$set=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
Register-ScheduledTask -TaskName "CuaIcuEval" -Action \$act -Trigger \$trg -Principal \$pr -Settings \$set -Force | Out-Null
"bootstrap-done \$(Get-Date -Format o)" | Out-File C:\cua\bootstrap.txt
shutdown /r /t 8 /f
</powershell>
PS
)
ID=$(aws ec2 run-instances --image-id "$AMI" --instance-type "$TYPE" --iam-instance-profile "Name=$PROFILE" \
  --user-data "$UD" --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=icu-win-product-drive}]' \
  --block-device-mappings '[{"DeviceName":"/dev/sda1","Ebs":{"VolumeSize":40,"VolumeType":"gp3"}}]' \
  --query 'Instances[0].InstanceId' --output text)
echo "INSTANCE_ID=$ID"
