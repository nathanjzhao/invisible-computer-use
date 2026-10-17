param([Parameter(Mandatory=$true)][string]$Name,[Parameter(Mandatory=$true)][string]$Out,[string]$ApiVersion='CodexComputerUseIPC-5')
# Connect to \\.\pipe\<Name> as whoever runs this, send a ping frame (u32 little-endian length + JSON), write what happened.
$r=[ordered]@{user="$env:USERDOMAIN\$env:USERNAME";pipe=$Name}
try {
    $c=New-Object IO.Pipes.NamedPipeClientStream('.',$Name,[IO.Pipes.PipeDirection]::InOut)
    $c.Connect(4000)
    $json=[Text.Encoding]::UTF8.GetBytes((@{id=1;jsonrpc='2.0';method='ping';params=@{clientApiVersion=$ApiVersion}} | ConvertTo-Json -Compress))
    $frame=[BitConverter]::GetBytes([uint32]$json.Length)+$json
    $c.Write($frame,0,$frame.Length); $c.Flush()
    $head=New-Object byte[] 4; $n=0; while($n -lt 4){ $k=$c.Read($head,$n,4-$n); if($k -le 0){ throw 'closed before a reply' }; $n+=$k }
    $len=[BitConverter]::ToUInt32($head,0); $body=New-Object byte[] $len; $n=0; while($n -lt $len){ $k=$c.Read($body,$n,$len-$n); if($k -le 0){ throw 'closed mid reply' }; $n+=$k }
    $r.opened=$true; $r.reply=[Text.Encoding]::UTF8.GetString($body)
    $c.Dispose()
} catch { $r.opened=$false; $r.error=$_.Exception.GetType().Name+': '+$_.Exception.Message; if($_.Exception.InnerException){ $r.error+=' / '+$_.Exception.InnerException.Message } }
$r | ConvertTo-Json | Set-Content $Out -Encoding UTF8
