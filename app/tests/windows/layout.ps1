param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Command)
# Standard-user display setup: ICUDisplayLayout.exe list|scale|arrange ...
$ErrorActionPreference='Stop'
$tool=Join-Path $env:LOCALAPPDATA 'ICUAudit\ICUDisplayLayout.exe'
New-Item -ItemType Directory -Force (Split-Path $tool) | Out-Null
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /target:exe /main:DisplayLayout /reference:System.Web.Extensions.dll /reference:System.Windows.Forms.dll /reference:System.Drawing.dll ("/out:"+$tool) (Join-Path $PSScriptRoot 'DisplayLayout.cs') (Join-Path $PSScriptRoot 'DpiFixture.cs')
if($LASTEXITCODE){throw 'Display tool compilation failed'}
if(-not $Command){$Command=@('list')}
if($Command[0] -eq 'standard'){
    # The audit layout, by detected device names (a fresh host names the two
    # virtual monitors DISPLAY2/3, an older one DISPLAY4/5): primary at 0,0
    # at 100%, the first virtual monitor at -1920,0 at 150%, the second at
    # 0,-1080 at 125%.
    $monitors=@(& $tool list | ConvertFrom-Json | ForEach-Object {$_})
    if($LASTEXITCODE){throw 'Display list failed'}
    $primary=@($monitors | Where-Object {$_.primary})
    $others=@($monitors | Where-Object {-not $_.primary} | Sort-Object {[int]($_.device -replace '\D','')})
    if($primary.Count -ne 1 -or $others.Count -ne 2){throw "Expected one primary and two virtual monitors, found: $(($monitors | ForEach-Object {$_.device}) -join ', ')"}
    $p=$primary[0].device;$a=$others[0].device;$b=$others[1].device
    & $tool arrange $p $p 0 0 0 0 $a -1920 0 1920 1080 $b 0 -1080 1920 1080 | Out-Null
    if($LASTEXITCODE){throw 'arrange failed'}
    Start-Sleep -Seconds 2
    & $tool scale $a 150 | Out-Null;if($LASTEXITCODE){throw "scale $a failed"}
    & $tool scale $b 125 | Out-Null;if($LASTEXITCODE){throw "scale $b failed"}
    Start-Sleep -Seconds 2
    $Command=@('list')
}
# Native stderr (an exception trace) must not abort before it is shown.
$ErrorActionPreference='Continue'
& $tool @Command 2>&1 | ForEach-Object {"$_"}
exit $LASTEXITCODE
