# Dot-sourced by the gaps4 updater scripts, which run as the logged-on user in the interactive session.
# Needs $Dir (update server folder), $Harness (app\tests\windows), $install and $exe (the installed app) set first.
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Harness 'NativeObserver.cs')
$script:steps=@()
$script:appWindow=[IntPtr]::Zero
function Step([string]$what,$data=$null){ $script:steps += [ordered]@{t=(Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff');what=$what;data=$data}; Write-Output ("{0} {1} {2}" -f (Get-Date -Format 'HH:mm:ss'),$what,($data|ConvertTo-Json -Compress -Depth 4)) }
function Req { @(Get-Content (Join-Path $Dir 'requests.log') -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ }) }
function Version { try { (Get-Item $exe).VersionInfo.ProductVersion } catch { '<missing>' } }
function Wait-For($predicate,[int]$seconds,[string]$message){ $d=(Get-Date).AddSeconds($seconds); while((Get-Date) -lt $d){ $v=& $predicate; if($v){return $v}; Start-Sleep -Milliseconds 200 }; throw $message }
function Nodes { $root=[Windows.Automation.AutomationElement]::FromHandle($script:appWindow); return $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) }
function Find-Name([string]$text,[bool]$exact=$true){ foreach($n in (Nodes)){ try{ $name=$n.Current.Name; if(-not $n.Current.IsOffscreen -and (($exact -and $name -eq $text) -or (-not $exact -and $name.Contains($text)))){return $n} }catch{} }; return $null }
function Press($node){ for($i=0;$i -lt 8 -and $node;$i++){ $p=$null; if($node.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$p)){ $p.Invoke(); return }; $node=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node) }; throw 'No invoke action' }
function Texts { @(Nodes | ForEach-Object { try{$_.Current.Name}catch{} } | Where-Object { $_ -and $_.Length -lt 300 }) }
function App-Procs { @(Get-CimInstance Win32_Process -Filter "Name='app.exe'" | Where-Object { $_.ExecutablePath -eq $exe }) }
function Shot([string]$name){ try{ if($script:appWindow -ne [IntPtr]::Zero){ [NativeObserver]::SetWindowPos($script:appWindow,[IntPtr](-1),0,0,0,0,0x13) | Out-Null; Start-Sleep -Milliseconds 400 }; $b=[Windows.Forms.Screen]::PrimaryScreen.Bounds; $i=New-Object Drawing.Bitmap($b.Width,$b.Height); $g=[Drawing.Graphics]::FromImage($i); $g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size); $i.Save((Join-Path $script:work "$name.png"),[Drawing.Imaging.ImageFormat]::Png); $g.Dispose();$i.Dispose() }catch{} }
# Every file of the install with its hash: "nothing changed" is a comparison of two of these.
function Install-Fingerprint { @(Get-ChildItem $install -File -Recurse -ErrorAction SilentlyContinue | Sort-Object FullName | ForEach-Object { '{0}|{1}' -f $_.FullName,(Get-FileHash $_.FullName -Algorithm SHA256).Hash }) }
function Per-User-Copies { @(@("$env:LOCALAPPDATA\Programs\Copilot") | Where-Object { Test-Path $_ }) }
function Is-Admin-Member { [bool](Get-LocalGroupMember Administrators -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*\$env:USERNAME" }) }
function Start-UpdateServer { $s=Start-Process 'C:\Program Files\nodejs\node.exe' -ArgumentList "`"$Dir\upd-server.js`"","`"$Dir`"",'18080' -PassThru -WindowStyle Hidden; Start-Sleep -Milliseconds 800; return $s }
function Start-App {
    $a=Start-Process $exe -PassThru
    $script:appWindow=Wait-For { $w=[NativeObserver]::FindTopLevel($a.Id,'Tauri Window',200); if($w -ne [IntPtr]::Zero){$w} } 30 'app window did not appear'
    $screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds
    [NativeObserver]::SetWindowPos($script:appWindow,[IntPtr]::Zero,0,0,[Math]::Min(1200,$screen.Width-20),[Math]::Min(800,$screen.Height-20),0x14) | Out-Null
    return $a
}
