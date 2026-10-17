param(
    # The extracted pristine vendor tree (build.ps1's vendor.tar.gz) and the patch.
    [Parameter(Mandatory=$true)][string]$VendorArchive,
    [Parameter(Mandatory=$true)][string]$Patch,
    # The tree build.ps1 patched with core.autocrlf=false, to compare with the control.
    [string]$BuiltTree=''
)
# Run on the Windows host. Applies the driver patch twice to a fresh vendor tree: with the host's own git
# configuration (Git for Windows ships core.autocrlf=true) and with -c core.autocrlf=false, and counts the files
# that carry a CR. The second must have none and equal the tree build.ps1 patched.
$ErrorActionPreference='Stop'
$git='C:\Program Files\Git\cmd\git.exe'
$base='C:\ICUAudit\crlf'
if(Test-Path $base){ cmd /c "rd /s /q $base" }
New-Item -ItemType Directory $base | Out-Null
"git config core.autocrlf: system=[$(& $git config --system core.autocrlf)] global=[$(& $git config --global core.autocrlf)] effective=[$(& $git config core.autocrlf)]"
function Apply([string]$name,[string[]]$prefix){
    $dir="$base\$name"; New-Item -ItemType Directory $dir | Out-Null
    & tar.exe -xzf $VendorArchive -C $dir
    & $git @prefix -C $dir apply $Patch
    if($LASTEXITCODE){ throw "apply failed for $name" }
    return $dir
}
function CrFiles([string]$dir){
    $n=0; $total=0
    Get-ChildItem $dir -Recurse -File -Include *.rs,*.json,*.ts,*.py,*.toml,*.lock | ForEach-Object {
        $total++
        $bytes=[IO.File]::ReadAllBytes($_.FullName)
        if([Array]::IndexOf($bytes,[byte]13) -ge 0){ $n++ }
    }
    return "$n of $total files have a CR"
}
$pristine="$base\pristine"; New-Item -ItemType Directory $pristine | Out-Null
& tar.exe -xzf $VendorArchive -C $pristine
"pristine tree: $(CrFiles $pristine)"
$withDefault=Apply 'default' @()
$withFalse=Apply 'off' @('-c','core.autocrlf=false')
$withTrue=Apply 'on' @('-c','core.autocrlf=true')
"patched with the host's own configuration: $(CrFiles "$withDefault\libs\cua-driver\rust\crates")"
"patched with -c core.autocrlf=true:        $(CrFiles "$withTrue\libs\cua-driver\rust\crates")"
"patched with -c core.autocrlf=false:       $(CrFiles "$withFalse\libs\cua-driver\rust\crates")"
function TreeHash([string]$dir){
    $lines=Get-ChildItem "$dir\libs\cua-driver\rust\crates","$dir\libs\cua-driver\contract" -Recurse -File | Sort-Object FullName | ForEach-Object { (Get-FileHash $_.FullName -Algorithm SHA256).Hash + ' ' + $_.FullName.Substring($dir.Length) }
    $sha=[Security.Cryptography.SHA256]::Create()
    return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))) -replace '-','')
}
"tree hash, autocrlf=false: $(TreeHash $withFalse)"
"tree hash, autocrlf=true:  $(TreeHash $withTrue)"
if($BuiltTree){ "tree hash, the build's own: $(TreeHash $BuiltTree)" }
