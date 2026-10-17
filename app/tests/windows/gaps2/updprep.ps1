param(
    [Parameter(Mandatory=$true)][ValidateSet('keys','manifests')][string]$Step,
    [string]$Root='C:\ICUAudit\source-660cc19',
    [string]$Dir='C:\ICUAudit\upd'
)
$ErrorActionPreference='Stop'
$env:CARGO_HOME='C:\ICUAudit\cargo'; $env:RUSTUP_HOME='C:\ICUAudit\rustup'
$env:PATH='C:\Program Files\nodejs;C:\Program Files\Git\cmd;'+[Environment]::GetEnvironmentVariable('Path','Machine')
$env:CI='true'
New-Item -ItemType Directory -Force "$Dir\files" | Out-Null
$tauri="$Root\app\node_modules\.bin\tauri.cmd"
if($Step -eq 'keys'){
    foreach($k in 'k1','k2'){
        Remove-Item "$Dir\$k.key","$Dir\$k.key.pub" -ErrorAction SilentlyContinue
        & $tauri signer generate --ci -w "$Dir\$k.key" -p '""' 2>&1 | Out-Null
        "$k : private $((Get-Item "$Dir\$k.key").Length) bytes, public $((Get-Item "$Dir\$k.key.pub").Length) bytes"
    }
    "k1 public (base64 minisign public key, not secret): " + (Get-Content "$Dir\k1.key.pub" -Raw).Trim()
    "k2 public: " + (Get-Content "$Dir\k2.key.pub" -Raw).Trim()
    "the two public keys differ: " + ((Get-Content "$Dir\k1.key.pub" -Raw) -ne (Get-Content "$Dir\k2.key.pub" -Raw))
}
if($Step -eq 'manifests'){
    # inputs: C:\ICUAudit\upd\build-0.2.0\Copilot_0.2.0_x64-setup.exe(.sig), built with k1
    $good="$Dir\build-0.2.0\Copilot_0.2.0_x64-setup.exe"
    $sig=(Get-Content "$good.sig" -Raw).Trim()
    Copy-Item $good "$Dir\files\Copilot_0.2.0_x64-setup.exe" -Force
    # a tampered copy: one byte changed in the middle, the .sig of the original kept
    $bytes=[IO.File]::ReadAllBytes($good); $mid=[int]($bytes.Length/2); $bytes[$mid]=$bytes[$mid] -bxor 0xFF
    [IO.File]::WriteAllBytes("$Dir\files\Copilot_0.2.0_x64-setup-tampered.exe",$bytes)
    # the same archive signed by another key (a throwaway that is NOT the app's), for the same version
    & $tauri signer sign -f "$Dir\k2.key" -p '""' --app-version 0.2.0 $good 2>&1 | Out-Null
    $wrong=(Get-Content "$good.sig" -Raw).Trim()
    # restore k1's signature file (the sign command above overwrote it)
    & $tauri signer sign -f "$Dir\k1.key" -p '""' --app-version 0.2.0 $good 2>&1 | Out-Null
    $sigk1=(Get-Content "$good.sig" -Raw).Trim()
    # the same archive signed for 0.0.9 by the right key: what a replayed old release would carry
    & $tauri signer sign -f "$Dir\k1.key" -p '""' --app-version 0.0.9 $good 2>&1 | Out-Null
    $old=(Get-Content "$good.sig" -Raw).Trim()
    & $tauri signer sign -f "$Dir\k1.key" -p '""' --app-version 0.2.0 $good 2>&1 | Out-Null
    function M([string]$version,[string]$signature,[string]$file){
        @{version=$version;notes='updater test';pub_date='2026-10-04T00:00:00Z';platforms=@{'windows-x86_64'=@{signature=$signature;url="http://127.0.0.1:18080/files/$file"}}} | ConvertTo-Json -Depth 6
    }
    M '0.2.0' $sigk1 'Copilot_0.2.0_x64-setup.exe'          | Set-Content "$Dir\manifest-good.json" -Encoding UTF8
    M '0.2.0' $wrong 'Copilot_0.2.0_x64-setup.exe'          | Set-Content "$Dir\manifest-wrongkey.json" -Encoding UTF8
    M '0.2.0' $sigk1 'Copilot_0.2.0_x64-setup-tampered.exe' | Set-Content "$Dir\manifest-tampered.json" -Encoding UTF8
    M '0.0.9' $old   'Copilot_0.2.0_x64-setup.exe'          | Set-Content "$Dir\manifest-downgrade.json" -Encoding UTF8
    M '0.3.0' $sigk1 'Copilot_0.2.0_x64-setup.exe'          | Set-Content "$Dir\manifest-versionmismatch.json" -Encoding UTF8
    "signatures differ (good/wrong/old): " + (@($sigk1,$wrong,$old | Select-Object -Unique).Count)
    Get-ChildItem $Dir | Select-Object Name,Length | Format-Table -AutoSize | Out-String
    Get-ChildItem "$Dir\files" | Select-Object Name,Length | Format-Table -AutoSize | Out-String
}
