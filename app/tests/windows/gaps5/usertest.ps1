param(
    [Parameter(Mandatory=$true)][string]$Root,
    [string[]]$Filter=@('a_real_child_of_the_target')
)
# Run as SYSTEM (SSM). The unit tests of platform-windows that need a visible window run as the interactive standard user
# (a service session's windows are not visible): the test executable cargo built for `-p platform-windows --lib` is copied
# to C:\Users\Public and started with as-user.ps1.
$ErrorActionPreference='Stop'
$deps="$Root\vendor-cua\libs\cua-driver\rust\target\debug\deps"
$exe=@(Get-ChildItem $deps -Filter 'platform_windows-*.exe' | Sort-Object LastWriteTime -Descending)[0]
Copy-Item $exe.FullName C:\Users\Public\pw-tests.exe -Force
foreach($f in $Filter){
    "== $f"
    & (Join-Path (Split-Path -Parent $PSScriptRoot) 'as-user.ps1') -Script C:\Users\Public\pw-tests.exe -Arguments "--test-threads=1 $f" -TimeoutSeconds 120 | Select-String -Pattern '^test |test result|panicked|visible|task_result' | ForEach-Object { $_.Line }
}
