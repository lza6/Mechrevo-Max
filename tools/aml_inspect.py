"""从 DSDT 中提取厂商设备节点的 AML 定义, 分析其 _HID/_STA.

背景: 本机 BIOS 的 DSDT 中存在 INOU0000 定义, 但 Windows 未枚举该设备.
需要确认其 _STA 方法为何返回 0 (即被 BIOS 禁用).

用法: python -X utf8 -I tools/aml_inspect.py <设备ID> [更多ID...]
"""
import ctypes
import struct
import sys
from ctypes import wintypes

ACPI_PROVIDER = 0x41435049
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.GetSystemFirmwareTable.restype = wintypes.UINT
kernel32.GetSystemFirmwareTable.argtypes = [
    wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD
]

# AML 操作码
NAME_OP = b"\x08"
DEVICE_OP = b"\x5b\x82"
METHOD_OP = b"\x14"
NAME_HID = b"_HID"
NAME_STA = b"_STA"
NAME_UID = b"_UID"
NAME_CID = b"_CID"
NAME_DDN = b"_DDN"


def get_dsdt() -> bytes:
    sig = struct.unpack("<I", b"DSDT")[0]
    size = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, None, 0)
    if not size:
        return b""
    buf = ctypes.create_string_buffer(size)
    got = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, buf, size)
    return buf.raw[:got]


def context(dsdt: bytes, needle: bytes, before: int = 320, after: int = 420) -> None:
    """打印 needle 每次出现处的 AML 上下文."""
    start = 0
    n = 0
    while True:
        i = dsdt.find(needle, start)
        if i < 0:
            break
        n += 1
        lo = max(0, i - before)
        hi = min(len(dsdt), i + after)
        chunk = dsdt[lo:hi]
        print(f"\n--- 第 {n} 处 @ 0x{i:06X} (上下文中相对偏移 {i - lo}) ---")
        print(f"    ASCII: {chunk.decode('latin1', errors='replace')!r}")
        # 判断附近是否有 _STA 方法
        near = dsdt[lo:hi]
        marks = []
        for nm, label in ((NAME_HID, "_HID"), (NAME_STA, "_STA"),
                          (NAME_UID, "_UID"), (NAME_CID, "_CID"),
                          (NAME_DDN, "_DDN")):
            if nm in near:
                marks.append(label)
        print(f"    附近标记: {marks}")
        start = i + 1
    if n == 0:
        print(f"  未找到 {needle!r}")


def main() -> int:
    ids = sys.argv[1:] or ["INOU0000", "UNIW0001", "CUST0000", "PWRC0000"]
    dsdt = get_dsdt()
    if not dsdt:
        print("无法读取 DSDT")
        return 1
    print(f"DSDT: {len(dsdt):,} 字节")

    for dev in ids:
        print("\n" + "=" * 72)
        print(f"设备 ID: {dev}")
        print("=" * 72)
        context(dsdt, dev.encode("ascii"))

    # 统计 DSDT 中所有 _STA 方法数量
    print("\n" + "=" * 72)
    print(f"DSDT 中 '_STA' 出现次数: {dsdt.count(NAME_STA)}")
    print(f"DSDT 中 '_HID' 出现次数: {dsdt.count(NAME_HID)}")
    print(f"DSDT 中 Device 操作码 (5B 82) 次数: {dsdt.count(DEVICE_OP)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
