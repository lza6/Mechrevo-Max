# 枚举本机所有第三方 (OEM) 驱动包, 找出厂商硬件接口驱动
# 用途: 确定本机 EC/ACPI 访问应当走哪个驱动

$out = pnputil.exe /enum-drivers 2>&1 | Out-String
$blocks = $out -split "`r?`n`r?`n"

Write-Output "===== 第三方 (非 Microsoft) 驱动包 ====="
$count = 0
foreach ($b in $blocks) {
    if ($b -notmatch 'oem\d+\.inf') { continue }
    $provider = ''
    $origName = ''
    $className = ''
    $ver = ''
    foreach ($line in ($b -split "`r?`n")) {
        if ($line -match '提供程序|Provider') { $provider = ($line -split ':',2)[1].Trim() }
        if ($line -match '原始名称|Original Name') { $origName = ($line -split ':',2)[1].Trim() }
        if ($line -match '类:|Class:') { $className = ($line -split ':',2)[1].Trim() }
        if ($line -match '版本|Version') { $ver = ($line -split ':',2)[1].Trim() }
    }
    if ($provider -and $provider -notmatch 'Microsoft') {
        $count++
        Write-Output ("[{0}] provider={1}  class={2}  ver={3}" -f $origName, $provider, $className, $ver)
    }
}
Write-Output ""
Write-Output ("第三方驱动包总数: {0}" -f $count)

Write-Output ""
Write-Output "===== 名称含 ACPI / Uniwill / INOU / Control 的驱动包 ====="
foreach ($b in $blocks) {
    if ($b -match 'uwacpi|acpidriver|inou|uniwill|controlcenter|gamingcenter') {
        Write-Output "----------------------------------------"
        Write-Output $b.Trim()
    }
}
