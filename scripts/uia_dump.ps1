# UIA 树导出工具
#
# 用途：把指定进程主窗口的 UI Automation 树打印出来，用于编写 E2E 选择器、
#       或诊断"为什么自动化找不到某个元素"（尤其 WebView2 内容）。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/uia_dump.ps1 [-ProcessName mechrevo-max] [-MaxDepth 8]

param(
    [string]$ProcessName = 'mechrevo-max',
    [int]$MaxDepth = 8,
    [int]$WaitSec = 15
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$deadline = (Get-Date).AddSeconds($WaitSec)
$proc = $null
while ((Get-Date) -lt $deadline) {
    $proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
            Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($proc) { break }
    Start-Sleep -Milliseconds 500
}
if (-not $proc) { Write-Error "未找到进程窗口: $ProcessName"; exit 1 }

Write-Output ("主窗口句柄: 0x{0:X}" -f [int64]$proc.MainWindowHandle)

$root = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)

# WebView2/Chromium 的 UIA 树需要客户端连接后才构建，首次访问往往为空。
# 多试几次并稍作等待。
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
for ($attempt = 1; $attempt -le 6; $attempt++) {
    $desc = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                          [System.Windows.Automation.Condition]::TrueCondition)
    Write-Output ("第 {0} 次探测：后代元素数 = {1}" -f $attempt, $desc.Count)
    if ($desc.Count -gt 1) { break }
    Start-Sleep -Milliseconds 1500
}

function Dump-Node {
    param($Element, [int]$Depth, [int]$Indent)
    if ($Depth -gt $MaxDepth) { return }
    $pad = ' ' * ($Indent * 2)
    try {
        $ct = $Element.Current.ControlType.ProgrammaticName -replace '^ControlType\.', ''
        $nm = $Element.Current.Name
        $aid = $Element.Current.AutomationId
        $cls = $Element.Current.ClassName
        $line = "{0}{1}" -f $pad, $ct
        if ($nm)  { $line += " Name='$nm'" }
        if ($aid) { $line += " Id='$aid'" }
        if ($cls) { $line += " Class='$cls'" }
        Write-Output $line
    } catch {
        Write-Output ("{0}(读取失败: {1})" -f $pad, $_.Exception.Message)
    }

    $child = $walker.GetFirstChild($Element)
    while ($child) {
        Dump-Node -Element $child -Depth ($Depth + 1) -Indent ($Indent + 1)
        $child = $walker.GetNextSibling($child)
    }
}

Write-Output ''
Write-Output '=== UIA 树 ==='
Dump-Node -Element $root -Depth 0 -Indent 0
