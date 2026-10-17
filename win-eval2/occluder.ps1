Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$o=New-Object Windows.Forms.Form; $o.Text="OccluderTop"; $o.WindowState="Maximized"; $o.BackColor=[Drawing.Color]::DarkSlateGray
$o.Add_Shown({ $o.Activate(); $o.BringToFront() })
[Windows.Forms.Application]::Run($o)
