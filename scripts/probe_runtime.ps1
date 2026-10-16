# 采集机械革命官方控制台运行态信息
# 用途: 确定官方程序实际加载了哪些组件、窗口状态、是否正常初始化

Write-Output "===== 官方相关进程 ====="
$targets = Get-Process | Where-Object {
    $_.ProcessName -match 'GamingCenter3|SystrayComponent|PowerToys|ControlCenter'
}

foreach ($p in $targets) {
    Write-Output ("--- {0} (PID {1}) ---" -f $p.ProcessName, $p.Id)
    Write-Output ("  内存占用 : {0} MB" -f [int]($p.WS / 1MB))
    Write-Output ("  主窗口标题: '{0}'" -f $p.MainWindowTitle)
    Write-Output ("  启动时间 : {0}" -f $p.StartTime)
    try {
        $mods = $p.Modules | Where-Object {
            $_.FileName -notlike '*\Windows\System32\*' -and
            $_.FileName -notlike '*\Windows\SysWOW64\*' -and
            $_.FileName -notlike '*\Windows\WinSxS\*'
        }
        Write-Output ("  非系统模块 ({0} 个):" -f $mods.Count)
        $mods | ForEach-Object { Write-Output ("    {0}" -f $_.FileName) }
    } catch {
        Write-Output ("  模块枚举失败: {0}" -f $_.Exception.Message)
    }
    Write-Output ""
}

Write-Output "===== 所有可见顶层窗口 ====="
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Win32Enum {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
}
"@
[Win32Enum]::EnumWindows({
    param($h, $l)
    if ([Win32Enum]::IsWindowVisible($h)) {
        $sb = New-Object System.Text.StringBuilder 512
        [void][Win32Enum]::GetWindowTextW($h, $sb, 512)
        $t = $sb.ToString()
        if ($t.Length -gt 0) {
            $pid = 0
            [void][Win32Enum]::GetWindowThreadProcessId($h, [ref]$pid)
            $pn = (Get-Process -Id $pid -ErrorAction SilentlyContinue).ProcessName
            Write-Output ("  [{0}] {1}" -f $pn, $t)
        }
    }
    return $true
}, [IntPtr]::Zero) | Out-Null
