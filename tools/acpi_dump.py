"""导出并搜索 ACPI 固件表 (DSDT/SSDT), 查找厂商设备定义.

用途: 确认 BIOS 是否真的暴露 ACPI\\INOU0000 设备节点.
这是判定 UWACPIDriver 能否工作的最终依据 —— 设备 ID 来自固件而非 Windows.

原理: GetSystemFirmwareTable(ACPI) 枚举固件表, 再按签名取表内容.
用法: python -X utf8 -I tools/acpi_dump.py [关键字...]
"""
import ctypes
import re
import struct
import sys
from ctypes import wintypes

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.EnumSystemFirmwareTables.restype = wintypes.UINT
kernel32.EnumSystemFirmwareTables.argtypes = [wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD]
kernel32.GetSystemFirmwareTable.restype = wintypes.UINT
kernel32.GetSystemFirmwareTable.argtypes = [
    wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD
]

# Windows API 中的 "ACPI" 提供商标识
ACPI_PROVIDER = 0x41435049  # 'ACPI'
RSMB_PROVIDER = 0x52534D42  # 'RSMB' (原始固件表)


def enum_tables() -> list[int]:
    """枚举 ACPI 固件表签名."""
    size = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, None, 0)
    if not size:
        return []
    buf = ctypes.create_string_buffer(size)
    got = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, buf, size)
    sigs = []
    for i in range(0, got, 4):
        sigs.append(struct.unpack_from("<I", buf.raw, i)[0])
    return sigs


def get_table(sig: int) -> bytes:
    """按签名读取 ACPI 表内容."""
    size = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, None, 0)
    if not size:
        return b""
    buf = ctypes.create_string_buffer(size)
    got = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, buf, size)
    return buf.raw[:got]


def sig_str(sig: int) -> str:
    return struct.pack("<I", sig).decode("ascii", errors="replace")


def find_hids(data: bytes) -> set[str]:
    """从 ACPI 表字节中提取看起来像硬件 ID 的标记."""
    text = data.decode("latin1")
    # ACPI 里的 _HID/_CID 是 4-8 字符 ASCII, 以及 ACPI 命名空间中的 AML 名称
    hids = set()
    # 形如 INOU0000 / UNIW0001 / PNP0C14 等
    for m in re.finditer(r"\b([A-Z]{3,4}[0-9A-F]{4})\b", text):
        hids.add(m.group(1))
    return hids


def main() -> int:
    keywords = sys.argv[1:] or ["INOU", "UNIW", "MECH", "MGRE"]

    # 先尝试直接用签名读 DSDT (EnumSystemFirmwareTables 常不返回 DSDT)
    print("=== 直接按签名读取关键表 ===")
    dsdt = b""
    for sig_name in ("DSDT", "FACP", "SSDT"):
        sig = struct.unpack("<I", sig_name.encode("ascii"))[0]
        data = get_table(sig)
        print(f"  GetSystemFirmwareTable({sig_name}): {len(data):,} 字节")
        if sig_name == "DSDT" and not data:
            # 用 RSMB provider 再试
            global ACPI_PROVIDER
            old = ACPI_PROVIDER
            ACPI_PROVIDER = RSMB_PROVIDER
            data = get_table(sig)
            print(f"  RSMB provider DSDT: {len(data):,} 字节")
            ACPI_PROVIDER = old
        if sig_name == "DSDT":
            dsdt = data
    if dsdt:
        text = dsdt.decode("latin1")
        print()
        print("=== DSDT 关键字判定 ===")
        for k in keywords + ["INOU0000", "UNIW"]:
            print(f"  {k:<10}: {'存在' if k in text else '不存在'}")
        hids = sorted(set(re.findall(r"\b([A-Z]{3,4}[0-9A-F]{4})\b", text)))
        vendors = [h for h in hids if not h.startswith(("PNP", "ACPI"))]
        print(f"  DSDT 中厂商样式 ID: {vendors}")

    print()
    sigs = enum_tables()
    print(f"=== ACPI 固件表: 共 {len(sigs)} 张 ===")
    names = [sig_str(s) for s in sigs]
    print(f"签名: {names}\n")

    all_text = ""
    for sig, name in zip(sigs, names):
        data = get_table(sig)
        if not data:
            continue
        text = data.decode("latin1")
        all_text += text
        hits = [k for k in keywords if k in text]
        if hits and name not in ("FACP",):
            print(f"--- {name}: {len(data):,} 字节, 关键词命中 {hits}")

    print()
    print(f"=== 全部硬件 ID 样式标记 (去重) ===")
    hids = sorted(find_hids(all_text.encode("latin1")))
    vendor_like = [h for h in hids if not h.startswith("PNP") and not h.startswith("ACPI")]
    print(f"非标准(非 PNP*/ACPI*)标记 {len(vendor_like)} 个:")
    for h in vendor_like:
        print(f"    {h}")

    print()
    print("=== 关键判定 ===")
    for k in ("INOU0000", "INOU", "UNIW"):
        found = k in all_text
        print(f"  {k:<10}: {'✅ 存在于固件表中' if found else '❌ 固件表中不存在'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
