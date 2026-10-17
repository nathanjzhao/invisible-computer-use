Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$log="C:\cua\fx2.log"
$script:f=$null
function L($h,$v){ (@{hit=$h;v="$v";procid=$PID;focused=($script:f -and $script:f.ContainsFocus)}|ConvertTo-Json -Compress)|Out-File -Append -Encoding utf8 $log }
$f=New-Object Windows.Forms.Form; $script:f=$f; $f.Text="WinTarget2"; $f.StartPosition="Manual"
$f.ClientSize=New-Object Drawing.Size(340,220); $f.Location=New-Object Drawing.Point(160,160)
$b=New-Object Windows.Forms.Button; $b.Text="Press me"; $b.Name="pressbtn"; $b.Location=New-Object Drawing.Point(20,20); $b.Size=New-Object Drawing.Size(260,32); $b.Add_Click({ L "buttonPressed" "" })
$t=New-Object Windows.Forms.TextBox; $t.Name="entry"; $t.Location=New-Object Drawing.Point(20,70); $t.Size=New-Object Drawing.Size(300,26); $t.Add_TextChanged({ L "textChanged" $t.Text })
$f.Controls.AddRange(@($b,$t))
$f.Add_Activated({ L "activated" "" })
$f.Add_Deactivate({ L "deactivated" "" })
$tm=New-Object Windows.Forms.Timer; $tm.Interval=25000; $tm.Add_Tick({ $tm.Stop(); $t.Text="changed by fixture"; L "selfChanged" $t.Text }); $tm.Start()
$f.Add_Shown({ L "ready" ($f.Handle.ToInt64()); $f.Activate() })
[Windows.Forms.Application]::Run($f)
