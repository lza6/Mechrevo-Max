# Mekrevo-Max 界面端到端测试 (真实 UI 自动化)
#
# 目的：真实启动应用 → 真实点击界面按钮 → 真实改变硬件档位 → 断言界面与硬件状态
#       这是「UI 真的接通了后端」的唯一可信证据，取代人工截图验收。
#
# 依赖：Windows UIAutomation（系统自带，无需安装）
# 前置：管理员权限 + EC 设备就绪（sc.exe start UWACPIDriver; pnputil.exe /scan-devices）
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e_ui.ps1
#
# 退出码：0 = 全部通过；1 = 有断言失败；2 = 环境不具备（无法启动/EC 不可用）

param(
    [string]$ExePath = "$PSScriptRoot\..\app\src-tauri\target\release\mechrevo-max.exe",
    [int]$StartupWaitSec = 12
)

$ErrorActionPreference = 'Stop'
$script:Failures = @()
$script:Checks = 0

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Write-Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    $script:Checks++
    if ($Ok) {
        Write-Host ("  [PASS] {0}" -f $Name) -ForegroundColor Green
    } else {
        Write-Host ("  [FAIL] {0} {1}" -f $Name, $Detail) -ForegroundColor Red
        $script:Failures += "$Name $Detail"
    }
}

function Get-AppWindow {
    param([int]$TimeoutSec = 30)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $proc = Get-Process -Name 'mechrevo-max' -ErrorAction SilentlyContinue |
                Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($proc) {
            $el = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
            if ($el) { return @{ Proc = $proc; Element = $el } }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

# 在当前窗口的所有后代中，按控件类型 + 名称匹配
function Find-ByTypeAndName {
    param($Root, [string]$ControlType, [string]$Name)
    $condType = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::$ControlType)
    $all = $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condType)
    foreach ($e in $all) {
        if ($e.Current.Name -eq $Name) { return $e }
    }
    return $null
}

# 找出所有 Text 控件里以某前缀开头的那个（用于读取原始寄存器显示）
function Find-TextByPrefix {
    param($Root, [string]$Prefix)
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Text)
    $all = $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
    foreach ($e in $all) {
        if ($e.Current.Name -like "$Prefix*") { return $e.Current.Name }
    }
    return $null
}

function Invoke-Element {
    param($Element)
    if (-not $Element) { return $false }
    try {
        $pattern = $Element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
        return $true
    } catch {
        try {
            # 部分元素只支持 LegacyIAccessible / 需要点击坐标
            $rect = $Element.Current.BoundingRectangle
            Add-Type @"
using System; using System.Runtime.InteropServices;
public class Clicker { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,UIntPtr e); }
"@
            $x = [int]($rect.X + $rect.Width / 2)
            $y = [int]($rect.Y + $rect.Height / 2)
            [Clicker]::SetCursorPos($x, $y) | Out-Null
            [Clicker]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [Clicker]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            return $true
        } catch {
            return $false
        }
    }
}

# ---------------- 主流程 ----------------

Write-Host "`n=== Mekrevo-Max 界面 E2E 测试 ===`n" -ForegroundColor Cyan

if (-not (Test-Path $ExePath)) {
    Write-Host "找不到可执行文件: $ExePath" -ForegroundColor Red
    Write-Host "请先构建 release 版本: cargo build --release" -ForegroundColor Yellow
    exit 2
}

# 确保没有残留实例
Get-Process -Name 'mechrevo-max' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Write-Host "启动应用: $ExePath"
$p = Start-Process -FilePath $ExePath -PassThru
Start-Sleep -Seconds $StartupWaitSec

$win = Get-AppWindow
if (-not $win) {
    Write-Host "无法获取应用窗口" -ForegroundColor Red
    try { $p | Stop-Process -Force } catch {}
    exit 2
}
Write-Host ("窗口已就绪 (PID {0})`n" -f $win.Proc.Id)

try {
    # --- 1. EC 是否连通 ---
    Write-Host "1) EC 连通性"
    $ecText = Find-TextByPrefix $win.Element 'EC '
    Write-Check "顶栏显示 EC 状态" ($null -ne $ecText) "(实际: '$ecText')"
    Write-Check "EC 已连接" ($ecText -eq 'EC 已连接') "(实际: '$ecText')"
    if ($ecText -ne 'EC 已连接') {
        Write-Host "EC 未连接，无法继续 E2E。请先执行: sc.exe start UWACPIDriver; pnputil.exe /scan-devices" -ForegroundColor Yellow
        exit 2
    }

    # --- 2. 项目 ID ---
    Write-Host "2) 机型识别"
    $pid16 = Find-TextByPrefix $win.Element '16'
    Write-Check "项目 ID 显示为 16" ($pid16 -eq '16') "(实际: '$pid16')"

    # --- 3. 读数 ---
    Write-Host "3) 实时读数"
    $rpm = Find-TextByPrefix $win.Element '3'
    Write-Check "CPU 风扇转速为数字" ($null -ne $rpm -and $rpm -match '^\d+$') "(实际: '$rpm')"

    # --- 4. 档位切换（真实写硬件）---
    Write-Host "4) 档位切换（真实写 EC）"
    $rawBefore = Find-TextByPrefix $win.Element '751='
    Write-Check "能读到档位原始值" ($null -ne $rawBefore) "(实际: '$rawBefore')"
    Write-Host ("   切换前: {0}" -f $rawBefore)

    $btnTurbo = Find-ByTypeAndName $win.Element 'Button' '狂暴'
    Write-Check "找到『狂暴』按钮" ($null -ne $btnTurbo)

    if ($btnTurbo) {
        [void](Invoke-Element $btnTurbo)
        Start-Sleep -Seconds 3   # 等写硬件 + 2 秒轮询刷新
        $rawTurbo = Find-TextByPrefix $win.Element '751='
        Write-Host ("   切换到狂暴后: {0}" -f $rawTurbo)
        Write-Check "档位字段变为 0x10" ($rawTurbo -like '*751=0x10*') "(实际: '$rawTurbo')"
    }

    # --- 5. 切回均衡并验证复原 ---
    Write-Host "5) 切回均衡并验证复原"
    $btnBalance = Find-ByTypeAndName $win.Element 'Button' '均衡'
    Write-Check "找到『均衡』按钮" ($null -ne $btnBalance)

    if ($btnBalance) {
        [void](Invoke-Element $btnBalance)
        Start-Sleep -Seconds 3
        $rawBack = Find-TextByPrefix $win.Element '751='
        Write-Host ("   切回均衡后: {0}" -f $rawBack)
        Write-Check "档位复原为 0x00" ($rawBack -like '*751=0x00*') "(实际: '$rawBack')"
        Write-Check "与初始状态一致" ($rawBack -eq $rawBefore) "(初始 '$rawBefore' / 现在 '$rawBack')"
    }

    # --- 6. 功耗墙界面 ---
    Write-Host "6) 功耗墙界面"
    $pl1 = Find-TextByPrefix $win.Element '75 W'
    Write-Check "PL1 显示 75 W" ($null -ne $pl1) "(实际: '$pl1')"
    $btnApply = Find-ByTypeAndName $win.Element 'Button' '应用'
    Write-Check "找到『应用』按钮" ($null -ne $btnApply)
    $btnUndo = Find-ByTypeAndName $win.Element 'Button' '撤销上次'
    Write-Check "找到『撤销上次』按钮" ($null -ne $btnUndo)

} finally {
    Write-Host "`n清理：关闭应用"
    try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {}
}

# --- 汇总 ---
Write-Host "`n=== 结果 ===" -ForegroundColor Cyan
Write-Host ("检查项: {0}  失败: {1}" -f $script:Checks, $script:Failures.Count)
if ($script:Failures.Count -gt 0) {
    Write-Host "`n失败明细:" -ForegroundColor Red
    $script:Failures | ForEach-Object { Write-Host ("  - {0}" -f $_) -ForegroundColor Red }
    exit 1
}
Write-Host "`n全部通过 ✅" -ForegroundColor Green
exit 0
