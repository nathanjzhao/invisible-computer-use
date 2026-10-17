param([Parameter(Mandatory=$true)][string]$ProcessName,[string]$Out='C:\Users\Public\dumpwin.txt')
# Run through as-user.ps1: bring the top-level window of a process to the front, take a screenshot and dump the text of its UIA tree.
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,System.Drawing
$p=@(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
$lines=@("processes: " + (($p | ForEach-Object { "$($_.Id) '$($_.MainWindowTitle)' handle $($_.MainWindowHandle)" }) -join '; '))
foreach($proc in $p){
    $cond=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty,$proc.Id)
    $roots=[Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$cond)
    foreach($root in $roots){
        $lines+="window: '$($root.Current.Name)' class $($root.Current.ClassName) rect $($root.Current.BoundingRectangle)"
        foreach($n in $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)){ try{ $lines+="   $($n.Current.ControlType.ProgrammaticName) '$($n.Current.Name)'" }catch{} }
    }
}
$lines | Set-Content $Out -Encoding UTF8
$lines
