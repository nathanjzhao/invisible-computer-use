param([string]$Out='C:\Users\Public\shot.png')
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$b=[Windows.Forms.SystemInformation]::VirtualScreen
$bmp=New-Object Drawing.Bitmap $b.Width,$b.Height
$g=[Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size)
$bmp.Save($Out,[Drawing.Imaging.ImageFormat]::Png)
"saved $Out $($b.Width)x$($b.Height)"
