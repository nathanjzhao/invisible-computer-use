param(
    [string[]]$Mutants=@('none','fwd','points','validate','raise'),
    [string]$Mut='C:\ICUAudit\mut'
)
# Run as SYSTEM (SSM) with the interactive user logged on. For each build in $Mut\<name> (mutbuild.ps1): sign it with the throwaway
# CA and install it under Program Files (uia-forward-install.ps1; the mutants differ from 'none' by one guard, not by signature),
# then run what covers the drag: the drag actions against a SYSTEM fixture (gaps3-run.ps1 -Target system) and the worker's own checks
# (gaps3-worker.ps1). Results: C:\Users\Public\mut-<name>-drag.json and mut-<name>-worker.json.
$ErrorActionPreference='Continue'
$here=Split-Path -Parent $MyInvocation.MyCommand.Path
$harness=Split-Path -Parent $here
foreach($m in $Mutants){
    "=== $m  $(Get-Date -Format u)"
    $dest="C:\Program Files\icu-mut-$m"
    if(Test-Path $dest){ Get-Process cua-driver,cua-driver-uia -ErrorAction SilentlyContinue | Stop-Process -Force; cmd /c "rd /s /q `"$dest`"" }
    & (Join-Path $harness 'uia-forward-install.ps1') -Driver "$Mut\$m\cua-driver.exe" -Worker "$Mut\$m\cua-driver-uia.exe" -Dest $dest | Select-Object -First 2
    $o1="C:\Users\Public\mut-$m-drag.json"; $o2="C:\Users\Public\mut-$m-worker.json"
    Remove-Item $o1,$o2 -ErrorAction SilentlyContinue
    & (Join-Path $harness 'gaps3-run.ps1') -Dir $dest -Target system -Only 'drag_px_fg,drag_px_bg,drag_px_fg_decoy' -MustRefuse 'drag_px_bg' -Out $o1 | Out-Null
    & (Join-Path $harness 'as-user.ps1') -Script (Join-Path $harness 'gaps3-worker.ps1') -Arguments "-Dir '$dest' -Out '$o2'" -TimeoutSeconds 240 | Out-Null
    foreach($o in $o1,$o2){
        if(Test-Path $o){
            $j=Get-Content $o -Raw | ConvertFrom-Json
            if($o -eq $o1){ "{0}: pass={1} silent_ok=[{2}] not_landed=[{3}] drag_fg={4}" -f $m,$j.pass,($j.silent_ok -join ','),($j.not_landed -join ','),(($j.actions | Where-Object {$_.name -eq 'drag_px_fg'} | ForEach-Object { 'refused='+$_.refused+' landed='+$_.landed })) + ' ; decoy ' + (($j.actions | Where-Object {$_.name -eq 'drag_px_fg_decoy'} | ForEach-Object { 'covered_by='+$_.covered_by+' refused='+$_.refused+' landed='+$_.landed })) }
            else { $bad=@($j.checks.PSObject.Properties | Where-Object { $_.Value -eq $false } | ForEach-Object { $_.Name }); "{0}: pass={1} failed checks=[{2}]" -f $m,$j.pass,($bad -join ',') }
        } else { "$m : no result at $o" }
    }
}
"done $(Get-Date -Format u)"
