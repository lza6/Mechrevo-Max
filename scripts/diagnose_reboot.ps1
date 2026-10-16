# 重启原因诊断
#
# 用途：查明上一次系统重启/关机的真实原因（正常关机 / 崩溃 / 断电 / 过热）。
# 重点事件：
#   41    Kernel-Power          —— 未正常关机（掉电 / 死机 / 硬复位）
#   6008  EventLog              —— 上次关机是意外的
#   1074/1076                   —— 有进程主动发起的正常关机/重启
#   1001  BugCheck              —— 蓝屏（含错误码与转储路径）
#   WHEA-Logger                 —— 硬件错误（CPU/内存/PCIe/热事件）
#   6005/6006/6009              —— 事件日志服务启停（可推断宕机时刻）
#
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/diagnose_reboot.ps1

$ErrorActionPreference = 'SilentlyContinue'

function Show-Events {
    param([string]$Title, [hashtable]$Filter, [int]$Max = 10)
    Write-Output ""
    Write-Output ("=== {0} ===" -f $Title)
    $evts = Get-WinEvent -FilterHashtable $Filter -MaxEvents $Max
    if (-not $evts) { Write-Output "  (无记录)"; return }
    foreach ($e in $evts) {
        $msg = ($e.Message -split "`r?`n")[0]
        if ($msg.Length -gt 120) { $msg = $msg.Substring(0, 120) }
        Write-Output ("  {0}  [{1}]  {2}" -f $e.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'), $e.Id, $e.ProviderName)
        Write-Output ("      {0}" -f $msg)
    }
}

Write-Output "=== 当前时间 ==="
Get-Date
Write-Output ""
Write-Output "=== 本次启动时间 / 已运行时长 ==="
$os = Get-CimInstance Win32_OperatingSystem
Write-Output ("  上次启动: {0}" -f $os.LastBootUpTime)
Write-Output ("  已运行  : {0}" -f ((Get-Date) - $os.LastBootUpTime))

$since = (Get-Date).AddDays(-4)

Show-Events "Kernel-Power / 意外关机 (41)" @{LogName='System'; Id=41; StartTime=$since} 6
Show-Events "上次关机是意外的 (6008)" @{LogName='System'; Id=6008; StartTime=$since} 6
Show-Events "正常关机/重启 (1074)" @{LogName='System'; Id=1074; StartTime=$since} 8
Show-Events "蓝屏 (1001 BugCheck)" @{LogName='System'; Id=1001; StartTime=$since} 6
Show-Events "硬件错误 WHEA" @{LogName='System'; ProviderName='Microsoft-Windows-WHEA-Logger'; StartTime=$since} 10
Show-Events "热区/散热事件" @{LogName='System'; ProviderName='Microsoft-Windows-Kernel-Processor-Power'; StartTime=$since} 8
Show-Events "内核通用错误" @{LogName='System'; Level=1,2; StartTime=(Get-Date).AddHours(-24)} 15

Write-Output ""
Write-Output "=== 最近的 minidump（蓝屏转储）==="
$dumpDir = "$env:SystemRoot\Minidump"
if (Test-Path $dumpDir) {
    Get-ChildItem $dumpDir -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 5 |
        ForEach-Object { Write-Output ("  {0}  {1}" -f $_.LastWriteTime, $_.Name) }
} else {
    Write-Output "  (无 Minidump 目录 —— 未发生蓝屏)"
}

Write-Output ""
Write-Output "=== 内存转储设置 ==="
$cc = Get-CimInstance Win32_OSRecoveryConfiguration
Write-Output ("  DebugInfoType: {0}  (0=无 1=完整 2=内核 3=小 7=自动)" -f $cc.DebugInfoType)
Write-Output ("  MiniDumpDir  : {0}" -f $cc.MiniDumpDirectory)

Write-Output ""
Write-Output "=== 最近关机/启动轨迹（6005/6006/6009）==="
Get-WinEvent -FilterHashtable @{LogName='System'; Id=6005,6006,6008,6009; StartTime=$since} -MaxEvents 20 |
    Sort-Object TimeCreated -Descending | ForEach-Object {
        $tag = switch ($_.Id) {
            6005 { '事件日志服务启动（开机）' }
            6006 { '事件日志服务停止（关机）' }
            6008 { '上次关机异常' }
            6009 { '系统启动' }
        }
        Write-Output ("  {0}  [{1}] {2}" -f $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'), $_.Id, $tag)
    }
