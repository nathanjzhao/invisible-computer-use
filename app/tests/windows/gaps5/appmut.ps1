param([Parameter(Mandatory=$true)][string]$Root)
# Run as SYSTEM (SSM). Each mutant removes one guard of app\src-tauri\src\elevated_update.rs, runs the elevated_update tests
# (apptest.ps1) and puts the file back. Each must make at least one test fail.
$ErrorActionPreference='Stop'
$file="$Root\app\src-tauri\src\elevated_update.rs"
$backup='C:\ICUAudit\elevated_update.rs.orig'
Copy-Item $file $backup -Force
$mutants=[ordered]@{
    'a per-user install is started elevated'=@('matches!(kind, InstallKind::AllUsersNsis | InstallKind::Msi)','matches!(kind, InstallKind::AllUsersNsis | InstallKind::PerUserNsis | InstallKind::Msi)');
    'the unelevated start does not carry RunAsInvoker'=@('command.raw_arg(parameters).env(RUN_AS_INVOKER.0, RUN_AS_INVOKER.1);','command.raw_arg(parameters);');
    'any package is run for an MSI install'=@('InstallKind::Msi => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),','InstallKind::Msi => true,');
    'the MSI bundle marker is not looked at'=@('    if msi_bundle {
        return InstallKind::Msi;
    }','    let _ = msi_bundle;');
    'the MSI starts the app itself (AUTOLAUNCHAPP), at High integrity'=@('format!("/i \"{msi}\" /passive /promptrestart")','format!("/i \"{msi}\" /passive /promptrestart AUTOLAUNCHAPP=True")');
    'the app is not restarted after a failed install'=@(' & \"{explorer}\"',' && \"{explorer}\"');
    'an ampersand in a path is accepted'=@("matches!(c, '`"' | '&' | '%' |","matches!(c, '`"' | '%' |")
}
try {
    foreach($name in $mutants.Keys){
        $text=[IO.File]::ReadAllText($backup)
        $old=$mutants[$name][0] -replace "`r`n","`n"; $new=$mutants[$name][1] -replace "`r`n","`n"
        $text=$text -replace "`r`n","`n"
        $i=$text.IndexOf($old); if($i -lt 0){ throw "anchor not found: $name" }
        if($text.IndexOf($old,$i+1) -ge 0){ throw "anchor not unique: $name" }
        [IO.File]::WriteAllText($file,$text.Remove($i,$old.Length).Insert($i,$new),(New-Object Text.UTF8Encoding($false)))
        "=== mutant: $name"
        & "$PSScriptRoot\apptest.ps1" -Root $Root 2>&1 | Where-Object { $_ -match 'FAILED|test result|exit|error' } | Select-Object -First 8
    }
} finally {
    Copy-Item $backup $file -Force
    "restored: " + ((Get-FileHash $file).Hash -eq (Get-FileHash $backup).Hash)
}
