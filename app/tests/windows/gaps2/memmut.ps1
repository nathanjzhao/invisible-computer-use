param(
    [Parameter(Mandatory=$true)][ValidateSet('none','nolock','nolock-update-settings')][string]$Mutant,
    [string]$Filter='xproc',
    [string]$Root='C:\ICUAudit\source-660cc19',
    [string]$Out='C:\ICUAudit\memmut'
)
# Build the app crate's test executable with ONE guard removed from memory.rs, run the cross-process tests, restore.
# SYSTEM, same toolchain and manifest steps as build.ps1.
$ErrorActionPreference='Stop'
$base='C:\ICUAudit'
New-Item -ItemType Directory -Force $Out | Out-Null
$src="$Root\app\src-tauri\src\memory.rs"
$orig="$Out\memory.rs.orig"
if(-not (Test-Path $orig)){ Copy-Item $src $orig }
Copy-Item $orig $src -Force; (Get-Item $src).LastWriteTime=Get-Date
function Replace-Once([string]$text,[string]$old,[string]$new){
    $i=$text.IndexOf($old); if($i -lt 0){ throw "mutation anchor not found: $old" }
    if($text.IndexOf($old,$i+1) -ge 0){ throw "mutation anchor not unique: $old" }
    return $text.Substring(0,$i)+$new+$text.Substring($i+$old.Length)
}
$t=Get-Content $src -Raw -Encoding UTF8
switch($Mutant){
  'nolock' { $t=Replace-Once $t 'file.ok().filter(|f| f.lock().is_ok())' 'file.ok().filter(|_| false)' }
  'nolock-update-settings' {
      $t=Replace-Once $t "        let fields = patch.as_object().ok_or(`"settings must be an object`")?;`n        let _g = self.store();" "        let fields = patch.as_object().ok_or(`"settings must be an object`")?;"
  }
}
if($Mutant -ne 'none'){ [IO.File]::WriteAllText($src,$t,(New-Object Text.UTF8Encoding($false))) }
"mutant $Mutant : memory.rs differs from the original: " + ((Get-FileHash $src).Hash -ne (Get-FileHash $orig).Hash)
$env:CARGO_HOME="$base\cargo"; $env:RUSTUP_HOME="$base\rustup"
$env:PATH="$base\cargo\bin;C:\Program Files\nodejs;C:\Program Files\Git\cmd;C:\Program Files\Git\bin;"+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CARGO_BUILD_JOBS='8'; $env:CI='true'
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
Set-Location "$Root\app"
$ErrorActionPreference='Continue'
& cargo.exe test --locked --manifest-path src-tauri/Cargo.toml --lib --no-run *> "$Out\build-$Mutant.log"
if($LASTEXITCODE){ Get-Content "$Out\build-$Mutant.log" -Tail 30; throw 'test build failed' }
$exes=@(Get-ChildItem "$Root\app\src-tauri\target\debug\deps\app_lib-*.exe" | Where-Object { $_.Extension -eq '.exe' } | Sort-Object LastWriteTime -Descending)
$exe=$exes[0]
& mt.exe -nologo -manifest "$base\test-app.manifest" ("-outputresource:"+$exe.FullName+';#1') | Out-Null
"test exe: $($exe.Name)"
$t0=Get-Date
& $exe.FullName $Filter --test-threads=1 *> "$Out\run-$Mutant.log"
$code=$LASTEXITCODE
"exit $code after $([int]((Get-Date)-$t0).TotalSeconds) s"
Get-Content "$Out\run-$Mutant.log" | Where-Object { $_ -match '^test |test result|panicked|assertion' } | Select-Object -First 40
Copy-Item $orig $src -Force; (Get-Item $src).LastWriteTime=Get-Date
"restored memory.rs: " + ((Get-FileHash $src).Hash -eq (Get-FileHash $orig).Hash)
