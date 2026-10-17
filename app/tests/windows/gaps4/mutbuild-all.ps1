param([Parameter(Mandatory=$true)][string]$Root)
# Run as SYSTEM (as-system.ps1): the three drag-forwarding mutants, one after the other (mutbuild.ps1 each).
$ErrorActionPreference='Continue'
foreach($m in 'none','fwd','points','validate','raise'){
    "=== mutant $m  $(Get-Date -Format u)"
    & C:\ICUAudit\harness\gaps4\mutbuild.ps1 -Mutant $m -Root $Root
}
"done $(Get-Date -Format u)"
