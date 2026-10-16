"""从注册表读取 ACPI 固件表 (DSDT/SSDT), 搜索厂商设备标识.

Windows 在内核初始化后把 ACPI 表缓存到 HKLM\\HARDWARE\\ACPI\\<签名>\\<OEMID>\\<TABLEID>,
GetSystemFirmwareTable 拿不到 DSDT 时可用此路径.

用法: python -X utf8 -I tools/acpi_registry.py [关键字...]
"""
import re
import sys
import winreg

ACPI_ROOT = r"HARDWARE\ACPI"


def enum_subkeys(root: int, path: str) -> list[str]:
    """枚举注册表子键名."""
    out: list[str] = []
    try:
        with winreg.OpenKey(root, path) as k:
            i = 0
            while True:
                try:
                    out.append(winreg.EnumKey(k, i))
                    i += 1
                except OSError:
                    break
    except OSError:
        pass
    return out


def enum_values(root: int, path: str) -> list[tuple[str, bytes]]:
    """枚举注册表值, 返回 (名称, 二进制内容)."""
    out: list[tuple[str, bytes]] = []
    try:
        with winreg.OpenKey(root, path) as k:
            i = 0
            while True:
                try:
                    name, data, _ = winreg.EnumValue(k, i)
                    if isinstance(data, bytes):
                        out.append((name, data))
                    i += 1
                except OSError:
                    break
    except OSError:
        pass
    return out


def main() -> int:
    keywords = sys.argv[1:] or ["INOU", "UNIW", "MECH", "MGRE", "OU0000"]
    hklm = winreg.HKEY_LOCAL_MACHINE

    tables: list[tuple[str, str, bytes]] = []
    for sig in enum_subkeys(hklm, ACPI_ROOT):
        for oemid in enum_subkeys(hklm, rf"{ACPI_ROOT}\{sig}"):
            for tableid in enum_subkeys(hklm, rf"{ACPI_ROOT}\{sig}\{oemid}"):
                vals = enum_values(hklm, rf"{ACPI_ROOT}\{sig}\{oemid}\{tableid}")
                for vname, vdata in vals:
                    if len(vdata) > 36:
                        tables.append((f"{sig}/{oemid}/{tableid}", vname, vdata))

    print(f"=== 注册表中的 ACPI 表: {len(tables)} 个 ===")
    total = 0
    for name, vname, data in tables:
        print(f"  {name:<40} 值={vname:<16} {len(data):,} 字节")
        total += len(data)
    print(f"  合计 {total:,} 字节\n")

    blob = b"".join(d for _, _, d in tables)
    text = blob.decode("latin1")

    print("=== 关键字判定 ===")
    for k in keywords:
        print(f"  {k:<10}: {'存在' if k in text else '不存在'}")

    print()
    print("=== 所有厂商样式硬件 ID (非 PNP*/ACPI*/INTEL) ===")
    hids = sorted(set(re.findall(r"\b([A-Z]{3,4}[0-9A-F]{4})\b", text)))
    vendors = [h for h in hids if not h.startswith(("PNP", "ACPI"))]
    for h in vendors:
        print(f"    {h}")
    if not vendors:
        print("    (无)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
