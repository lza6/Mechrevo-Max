# 硬件健康快照：温度 / 风扇 / 电源 / EC 状态
#
# 用途：判断机器是否存在过热、供电异常、EC 异常（这些会导致"无转储的硬复位"）。
#
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File scripts/hw_health.ps1

$ErrorActionPreference = 'SilentlyContinue'

Write-Output "=== 电源与电池 ==="
$bat = Get-CimInstance Win32_Battery
if ($bat) {
    Write-Output ("  电池状态码 : {0}   (2=接AC, 1=放电)" -f $bat.BatteryStatus)
    Write-Output ("  剩余电量   : {0}%" -f $bat.EstimatedChargeRemaining)
}
$cs = Get-CimInstance -Namespace root\WMI -ClassName BatteryStatus -ErrorAction SilentlyContinue
if ($cs) {
    Write-Output ("  PowerOnline: {0}  Charging: {1}  Discharging: {2}" -f $cs.PowerOnline, $cs.Charging, $cs.Discharging)
}

Write-Output ""
Write-Output "=== 热区温度（ACPI）==="
$tz = Get-CimInstance -Namespace root/WMI -ClassName MSAcpi_ThermalZoneTemperature
if ($tz) {
    foreach ($z in $tz) {
        $c = [math]::Round(($z.CurrentTemperature / 10.0) - 273.15, 1)
        Write-Output ("  {0}: {1} °C" -f $z.InstanceName, $c)
    }
} else {
    Write-Output "  (ACPI 热区不可读)"
}

Write-Output ""
Write-Output "=== 热区设备 ==="
Get-CimInstance Win32_PnPEntity |
    Where-Object { $_.DeviceID -like '*THERMALZONE*' } |
    ForEach-Object { Write-Output ("  {0}" -f $_.DeviceID) }

Write-Output ""
Write-Output "=== CPU 当前频率与负载 ==="
$cpu = Get-CimInstance Win32_Processor
Write-Output ("  {0}" -f $cpu.Name)
Write-Output ("  当前频率: {0} MHz   最大: {1} MHz" -f $cpu.CurrentClockSpeed, $cpu.MaxClockSpeed)
Write-Output ("  负载    : {0}%" -f $cpu.LoadPercentage)

Write-Output ""
Write-Output "=== 风扇（WMI，可能不暴露）==="
$fan = Get-CimInstance Win32_Fan
if ($fan) { $fan | ForEach-Object { Write-Output ("  {0}: {1} RPM" -f $_.Name, $_.DesiredSpeed) } }
else { Write-Output "  (WMI 未暴露风扇转速)" }

Write-Output ""
Write-Output "=== 关键：EC 驱动与设备状态 ==="
$svc = Get-Service UWACPIDriver
Write-Output ("  UWACPIDriver 服务: {0}" -f $svc.Status)
$dev = Get-PnpDevice | Where-Object { $_.InstanceId -like '*INOU*' }
if ($dev) {
    $dev | ForEach-Object { Write-Output ("  设备 {0}: {1}" -f $_.InstanceId, $_.Status) }
} else {
    Write-Output "  ACPI\INOU0000 设备：未枚举"
}

Write-Output ""
Write-Output "=== 电源计划中的关键限制 ==="
powercfg /query SCHEME_CURRENT SUB_PROCESSOR 2>$null |
    Select-String -Pattern 'GUID 别名|当前交流电源设置|Current AC Power Setting' |
    Select-Object -First 20 | ForEach-Object { Write-Output ("  " + $_.Line.Trim()) }

Write-Output ""
Write-Output "=== 是否有过热/电源相关的近期告警 ==="
Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2,3; StartTime=(Get-Date).AddHours(-6)} -MaxEvents 200 |
    Where-Object { $_.ProviderName -match 'Thermal|Power|Kernel-Processor|WHEA|Battery|acpi' } |
    Select-Object -First 15 |
    ForEach-Object { Write-Output ("  {0} [{1}] {2}" -f $_.TimeCreated.ToString('HH:mm:ss'), $_.Id, $_.ProviderName) }
