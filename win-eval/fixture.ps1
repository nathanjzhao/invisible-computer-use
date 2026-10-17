Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$log="C:\cua\fx.log"
function L($h,$v){ (@{hit=$h;v="$v";procid=$PID}|ConvertTo-Json -Compress)|Out-File -Append -Encoding utf8 $log }
$f=New-Object Windows.Forms.Form; $f.Text="WinTarget"; $f.StartPosition="Manual"
$f.ClientSize=New-Object Drawing.Size(340,220); $f.Location=New-Object Drawing.Point(140,140)
$b=New-Object Windows.Forms.Button; $b.Text="Press me"; $b.Name="pressbtn"; $b.Location=New-Object Drawing.Point(20,20); $b.Size=New-Object Drawing.Size(260,32); $b.Add_Click({ L "buttonPressed" "" })
$t=New-Object Windows.Forms.TextBox; $t.Name="entry"; $t.Location=New-Object Drawing.Point(20,70); $t.Size=New-Object Drawing.Size(300,26); $t.Add_TextChanged({ L "textChanged" $t.Text })
$f.Controls.AddRange(@($b,$t))
$f.Add_Shown({ L "ready" $PID; $f.Activate() })
[Windows.Forms.Application]::Run($f)
