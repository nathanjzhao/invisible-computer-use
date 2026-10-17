param([Parameter(Mandatory=$true)][string]$Msi)
$wi=New-Object -ComObject WindowsInstaller.Installer
$db=$wi.OpenDatabase($Msi,0)
function Q([string]$sql,[int]$n){
    $v=$db.OpenView($sql); $v.Execute()
    while($true){ $r=$v.Fetch(); if($r -eq $null){break}; (1..$n | ForEach-Object { $r.StringData($_) }) -join ' | ' }
    $v.Close()
}
"--- Directory (Directory | Parent | DefaultDir)"; Q 'SELECT Directory,Directory_Parent,DefaultDir FROM Directory' 3
"--- Property (selected)"; Q "SELECT Property,Value FROM Property WHERE Property='ALLUSERS' OR Property='MSIINSTALLPERUSER' OR Property='ProductName' OR Property='Manufacturer' OR Property='INSTALLDIR' OR Property='WIXUI_INSTALLDIR'" 2
"--- RegLocator / AppSearch"; Q 'SELECT Signature_,Root,Key,Name,Type FROM RegLocator' 5; Q 'SELECT Property,Signature_ FROM AppSearch' 2
"--- CustomAction"; Q 'SELECT Action,Type,Source,Target FROM CustomAction' 4
"--- InstallExecuteSequence (SetProperty-like)"; Q "SELECT Action,Condition,Sequence FROM InstallExecuteSequence WHERE Action LIKE 'Set%'" 3
$si=$db.SummaryInformation(0); "Summary: template $($si.Property(7)); word count $($si.Property(15)) (bit 2 = elevated privileges not required, 8 = per-machine install is for all users)"
