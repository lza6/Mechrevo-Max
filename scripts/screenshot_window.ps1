# 截取指定进程主窗口的截图
# 用途: 对 UI 做真实视觉验证
# 用法: powershell -File scripts/screenshot_window.ps1 -ProcessName mechrevo-max -OutFile shot.png

param(
    [string]$ProcessName = "mechrevo-max",
    [string]$OutFile = "shot.png"
)

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinCap {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1

if (-not $proc) {
    Write-Error "未找到带主窗口的进程: $ProcessName"
    exit 1
}

$hwnd = $proc.MainWindowHandle
[void][WinCap]::ShowWindow($hwnd, 9)      # SW_RESTORE
[void][WinCap]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 900

$rect = New-Object WinCap+RECT
[void][WinCap]::GetWindowRect($hwnd, [ref]$rect)

$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) {
    Write-Error "窗口尺寸无效: ${w}x${h}"
    exit 1
}

$bmp = New-Object System.Drawing.Bitmap $w, $h
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose()
$bmp.Dispose()

Write-Output ("已保存: {0} ({1}x{2})" -f $OutFile, $w, $h)
