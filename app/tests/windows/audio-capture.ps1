param([string]$Work='C:\Users\Public\icu-evidence\audio-capture',[string]$Observer='C:\Program Files\ICU Gap4\taiku-observer.exe')
$ErrorActionPreference='Stop'
if($env:ICU_DISPOSABLE_HOST -ne '1' -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0){throw 'owned interactive test desktop required'}
if(Test-Path $Work){throw 'fresh evidence directory required'}
New-Item -Type Directory $Work|Out-Null
$report=[ordered]@{passed=$false;observer_sha256=(Get-FileHash $Observer).Hash;mode='synthetic tone through native Windows audio';cases=@()}
Add-Type @'
using System;using System.IO;
public class TestTone {
 public static void Write(string path) {
  using(var w=new BinaryWriter(File.Create(path))){int count=48000*3;w.Write(System.Text.Encoding.ASCII.GetBytes("RIFF"));w.Write(36+count*4);w.Write(System.Text.Encoding.ASCII.GetBytes("WAVEfmt "));w.Write(16);w.Write((short)1);w.Write((short)2);w.Write(48000);w.Write(48000*4);w.Write((short)4);w.Write((short)16);w.Write(System.Text.Encoding.ASCII.GetBytes("data"));w.Write(count*4);
   for(int i=0;i<count;i++){short sample=(short)(12000*Math.Sin(2*Math.PI*440*i/48000));w.Write(sample);w.Write(sample);}
  }
 }
}
'@
[TestTone]::Write("$Work\tone.wav")
$player=New-Object System.Media.SoundPlayer("$Work\tone.wav")
try {
 $player.PlayLooping();Start-Sleep -Milliseconds 700
 foreach($source in @('system','microphone','both')) {
  $out=Join-Path $Work $source;New-Item -Type Directory $out|Out-Null
  $info=New-Object Diagnostics.ProcessStartInfo $Observer
  $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
  $info.Arguments="audio --out `"$out`" --source $source --secs 30"
  $watch=[Diagnostics.Stopwatch]::StartNew();$p=[Diagnostics.Process]::Start($info)
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  try {
   Start-Sleep -Milliseconds 2200
   if(!$p.HasExited){$p.StandardInput.WriteLine('stop');$p.StandardInput.Close()}
   if(!$p.WaitForExit(8000)){$p.Kill();$p.WaitForExit();throw 'audio did not stop within eight seconds'}
   $watch.Stop();$stdout.Result|Set-Content "$out\stdout.txt";$stderr.Result|Set-Content "$out\stderr.txt"
   $manifests=@(Get-ChildItem $out -Filter manifest.json -Recurse)
   if($p.ExitCode -ne 0 -or $manifests.Count -ne 1){throw 'audio recorder failed to finalize one recording'}
   $manifest=Get-Content $manifests[0].FullName -Raw|ConvertFrom-Json
   $names=@($manifest.audio.sources|ForEach-Object{$_.name}|Sort-Object)
   $expected=switch($source){system{@('system')} microphone{@('mic')} both{@('mic','system')}}
   $row=@{source=$source;passed=(($names -join ',') -eq ($expected -join ',')) -and $watch.Elapsed.TotalSeconds -lt 8;seconds=$watch.Elapsed.TotalSeconds;captured=$names;manifest=$manifest}
   $report.cases+=,$row
   foreach($entry in $manifest.audio.sources){if($entry.duration_sec -le 0 -or !(Test-Path (Join-Path $manifests[0].DirectoryName $entry.file))){$row.passed=$false}}
  } finally {if(!$p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()}
 }
 $report.passed=$report.cases.Count -eq 3 -and @($report.cases|Where-Object{!$_.passed}).Count -eq 0
} catch {$report.error=$_.Exception.Message;$report.location=$_.InvocationInfo.PositionMessage}
finally {$player.Stop();$player.Dispose();$report|ConvertTo-Json -Depth 14|Set-Content "$Work\result.json" -Encoding UTF8;$report|ConvertTo-Json -Depth 6}
if(!$report.passed){exit 1}
