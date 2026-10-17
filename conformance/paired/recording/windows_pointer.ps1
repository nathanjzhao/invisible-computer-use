param([string]$Root='C:\ICUAudit\pointer',[Parameter(Mandatory=$true)][string]$Out)
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'disposable interactive desktop required'}
if(Test-Path $Out){throw 'fresh output required'}
New-Item -Type Directory $Out|Out-Null
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $Root 'NativeObserver.cs')
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class PointerInput{
 [StructLayout(LayoutKind.Sequential)]struct MOUSEINPUT{public int dx,dy;public uint data,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)]struct INPUT{[FieldOffset(0)]public uint type;[FieldOffset(8)]public MOUSEINPUT mouse;}
 [DllImport("user32.dll",SetLastError=true)]static extern uint SendInput(uint n,INPUT[] a,int size);
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 public static void Mouse(uint flags,int data){var i=new INPUT{type=0,mouse=new MOUSEINPUT{flags=flags,data=unchecked((uint)data)}};if(SendInput(1,new[]{i},Marshal.SizeOf(i))!=1)throw new Exception("SendInput failed");}
}
'@
$r=[ordered]@{passed=$false;actor='synthetic_fixture';observer_sha256=(Get-FileHash "$Root\observer.exe").Hash.ToLowerInvariant()};$fx=$null;$rec=$null
try{
 $fx=Start-Process "$Root\PointerFixture.exe" -ArgumentList "$Out\native.json" -PassThru
 Start-Sleep -Seconds 2;$fx.Refresh();[NativeObserver]::FocusGuardForSetup($fx.MainWindowHandle)
 if([NativeObserver]::Read().foreground -ne $fx.MainWindowHandle.ToInt64()){throw 'fixture did not take the foreground during setup'}
 $r.native_baseline=Get-Content "$Out\native.json" -Raw -Encoding UTF8|ConvertFrom-Json
 $baselineCount=0;foreach($event in $r.native_baseline){$baselineCount++}
 $rec=Start-Process "$Root\observer.exe" -ArgumentList "record --out $Out --session pointer --secs 15 --screen false --input false --net false --devices false --audio false --ui true --ui-values false" -PassThru -NoNewWindow -RedirectStandardOutput "$Out\observer.out" -RedirectStandardError "$Out\observer.err"
 $rec.Handle|Out-Null
 Start-Sleep -Seconds 3
 if([NativeObserver]::Read().foreground -ne $fx.MainWindowHandle.ToInt64()){throw 'another app covered the fixture before recording input'}
 [PointerInput]::SetCursorPos(230,240)|Out-Null
 foreach($pair in @(@(2,4),@(8,16),@(32,64))){[PointerInput]::Mouse($pair[0],0);Start-Sleep -Milliseconds 12;[PointerInput]::Mouse($pair[1],0);Start-Sleep -Milliseconds 450}
 [PointerInput]::Mouse(2,0);Start-Sleep -Milliseconds 30;[PointerInput]::SetCursorPos(320,280)|Out-Null;Start-Sleep -Milliseconds 30;[PointerInput]::Mouse(4,0);Start-Sleep -Milliseconds 450
 [PointerInput]::Mouse(2048,120);Start-Sleep -Milliseconds 450
 [PointerInput]::Mouse(2,0);Start-Sleep -Milliseconds 12;[PointerInput]::Mouse(4,0)
 if(-not $rec.WaitForExit(30000)){throw 'observer did not stop'}
 $events=@(Get-Content "$Out\recording-pointer\events.jsonl" -Encoding UTF8|ForEach-Object{$_|ConvertFrom-Json})
 $captured=@($events|Where-Object{$_.kind -in @('ui.click','ui.drag','ui.scroll') -and $_.payload.pid -eq $fx.Id})
 $r.captured=@($captured|ForEach-Object{@{kind=$_.kind;button=$_.payload.button;label=$_.payload.label;dy=$_.payload.dy}})
 $nativeAll=Get-Content "$Out\native.json" -Raw -Encoding UTF8|ConvertFrom-Json
 $r.native=@();$index=0;$nativeDown=0;$nativeScroll=0
 foreach($event in $nativeAll){
  if($index++ -lt $baselineCount){continue}
  $r.native+=@{kind=$event.kind;button=$event.button;delta=$event.delta}
  if($event.kind -eq 'down'){$nativeDown++};if($event.kind -eq 'scroll'){$nativeScroll++}
 }
 $r.native_down=$nativeDown;$r.native_scroll=$nativeScroll
 $r.gaps=@($events|Where-Object{$_.kind -eq 'ui.pointer_gap'})
 $r.raw_input_events=@($events|Where-Object{$_.kind -like 'input.*'}).Count
 $r.values=@($events|Where-Object{$_.kind -like 'ui.*' -and $_.payload.PSObject.Properties.Name -contains 'value'}).Count
 $r.passed=(@($captured|ForEach-Object{$_.kind}) -join ',') -eq 'ui.click,ui.click,ui.click,ui.click,ui.drag,ui.scroll,ui.click' -and $nativeDown -eq 5 -and $nativeScroll -eq 1 -and -not $r.raw_input_events -and -not $r.values -and -not $r.gaps.Count
 $b=[Windows.Forms.SystemInformation]::VirtualScreen;$bitmap=New-Object Drawing.Bitmap($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$bitmap.Save("$Out\after.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
}catch{$r.error=$_.Exception.Message;$r.location=$_.InvocationInfo.PositionMessage}
finally{foreach($p in @($rec,$fx)){if($p -and -not $p.HasExited){Stop-Process -Id $p.Id -Force;$p.WaitForExit()}};$r|ConvertTo-Json -Depth 12|Set-Content "$Out\result.json" -Encoding UTF8;$r|ConvertTo-Json -Depth 8}
if(-not $r.passed){exit 1}
