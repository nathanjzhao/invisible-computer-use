param(
    [string]$Dir='C:\ICUAudit\upd',
    [string]$Out='C:\ICUAudit'
)
# After gaps2\updprep.ps1 keys: write the two Tauri config overlays the signed builds use. Both point the app's updater at
# the local test server and trust the throwaway key k1 (NOT the release key, which this run never touches):
#   cfg-B.json = the 0.1.0 build that is installed (the version in tauri.conf.json)
#   cfg-C.json = the 0.2.0 build the server offers
$ErrorActionPreference='Stop'
$pub=(Get-Content "$Dir\k1.key.pub" -Raw).Trim()
if(-not $pub){ throw 'k1.key.pub is empty: run updprep.ps1 -Step keys first' }
$updater=@{updater=@{endpoints=@('http://127.0.0.1:18080/latest.json');pubkey=$pub;dangerousInsecureTransportProtocol=$true}}
@{bundle=@{useLocalToolsDir=$true};plugins=$updater} | ConvertTo-Json -Depth 8 -Compress | Set-Content "$Out\cfg-B.json" -Encoding ASCII
@{version='0.2.0';bundle=@{useLocalToolsDir=$true};plugins=$updater} | ConvertTo-Json -Depth 8 -Compress | Set-Content "$Out\cfg-C.json" -Encoding ASCII
"cfg-B.json and cfg-C.json written; k1 public key length $($pub.Length)"
