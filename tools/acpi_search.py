"""在所有 ACPI 固件表中搜索指定标识, 定位设备定义与覆盖.

用途: 确认 \\_SB.INOU (HID=INOU0000) 是否被某个 SSDT 的 _STA 覆盖为禁用.

用法: python -X utf8 -I tools/acpi_search.py [标识...]
"""
import ctypes
import struct
import sys
from ctypes import wintypes

ACPI_PROVIDER = 0x41435049
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.EnumSystemFirmwareTables.restype = wintypes.UINT
kernel32.EnumSystemFirmwareTables.argtypes = [wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD]
kernel32.GetSystemFirmwareTable.restype = wintypes.UINT
kernel32.GetSystemFirmwareTable.argtypes = [
    wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD
]


def read_table(sig: int) -> bytes:
    size = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, None, 0)
    if not size:
        return b""
    buf = ctypes.create_string_buffer(size)
    got = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, buf, size)
    return buf.raw[:got]


def table_identity(data: bytes) -> str:
    """从 ACPI 表头取 OEMID/OEMTABLEID."""
    if len(data) < 36:
        return "(头太短)"
    oemid = data[10:16].decode("latin1").strip("\0 ")
    tableid = data[16:24].decode("latin1").strip("\0 ")
    return f"OEM={oemid} TABLE={tableid}"


def main() -> int:
    needles = sys.argv[1:] or ["INOU0000", "INOU", "UNIW0001"]

    # 枚举所有表 (含重复签名, 按索引区分)
    size = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, None, 0)
    buf = ctypes.create_string_buffer(size)
    got = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, buf, size)
    sigs = [struct.unpack_from("<I", buf.raw, i)[0] for i in range(0, got, 4)]

    tables: list[tuple[str, int, bytes]] = []
    for idx, sig in enumerate(sigs):
        name = struct.pack("<I", sig).decode("ascii", "replace")
        data = read_table(sig)
        if data:
            tables.append((name, idx, data))

    # DSDT 单独取
    dsdt = read_table(struct.unpack("<I", b"DSDT")[0])
    if dsdt:
        tables.insert(0, ("DSDT", -1, dsdt))

    print(f"共 {len(tables)} 张 ACPI 表\n")
    print(f"{'表':<6} {'#':<4} {'标识':<34} {'大小':>10}  命中")
    print("-" * 90)
    for name, idx, data in tables:
        text = data.decode("latin1")
        hits = [n for n in needles if n in text]
        ident = table_identity(data)
        mark = ", ".join(hits) if hits else ""
        print(f"{name:<6} {idx:<4} {ident:<34} {len(data):>10,}  {mark}")

    print("\n" + "=" * 72)
    for n in needles:
        print(f"\n### 标识 {n!r} 的出现情况")
        for name, idx, data in tables:
            text = data.decode("latin1")
            cnt = text.count(n)
            if cnt:
                print(f"  {name}#{idx} ({table_identity(data)}): {cnt} 次")
                # 打印首次出现的上下文
                i = text.find(n)
                lo, hi = max(0, i - 120), min(len(text), i + 160)
                print(f"      上下文: {text[lo:hi]!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
