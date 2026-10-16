"""从 DSDT/SSDT 提取 EmbeddedControl 操作区及其字段映射.

原理:
  厂商在 DSDT 里定义 OperationRegion(..., EmbeddedControl, ...) 和对应的 Field(),
  把 EC 偏移映射成具名变量 (如 Fn 键、灯光、风扇相关).
  这是**固件层权威的 EC 寄存器地图**, 比社区逆向更可信.

AML 关键操作码:
  OperationRegion  5B 80 <NameString 4B> <RegionSpace 1B> <Offset TermArg> <Length TermArg>
  Field            5B 81 <PkgLength> <NameString> <Flags(1B)> <FieldElements...>
  IndexField       5B 86 ...
  RegionSpace: 0=SystemMemory 1=SystemIO 2=PCIConfig 3=EmbeddedControl 4=SMBus
  FieldElement: 0=ReservedField(后跟PkgLength) 1=AccessField 2=ConnectField 3=ExtendedAccessField
                其他=NamedField, 后跟 1 或 2 或 4 字节位宽 (由 Flags 的 bit0-1 决定)

用法: python -X utf8 -I tools/aml_ecmap.py [--raw]
"""
import ctypes
import re
import struct
import sys
from ctypes import wintypes

ACPI_PROVIDER = 0x41435049
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.GetSystemFirmwareTable.restype = wintypes.UINT
kernel32.GetSystemFirmwareTable.argtypes = [
    wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD
]

OP_REGION = b"\x5b\x80"
OP_FIELD = b"\x5b\x81"
OP_INDEXFIELD = b"\x5b\x86"
REGION_NAMES = {
    0: "SystemMemory",
    1: "SystemIO",
    2: "PCIConfig",
    3: "EmbeddedControl",
    4: "SMBus",
    5: "CMOS",
    6: "PCIBarTarget",
    7: "IPMI",
    8: "GeneralPurposeIO",
    9: "GenericSerialBus",
    10: "PCC",
}

NAME_CHARS = set(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")


def read_table(sig_name: str) -> bytes:
    sig = struct.unpack("<I", sig_name.encode("ascii"))[0]
    size = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, None, 0)
    if not size:
        return b""
    buf = ctypes.create_string_buffer(size)
    got = kernel32.GetSystemFirmwareTable(ACPI_PROVIDER, sig, buf, size)
    return buf.raw[:got]


def enum_all_tables() -> list[tuple[str, int, bytes]]:
    size = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, None, 0)
    buf = ctypes.create_string_buffer(size)
    got = kernel32.EnumSystemFirmwareTables(ACPI_PROVIDER, buf, size)
    sigs = [struct.unpack_from("<I", buf.raw, i)[0] for i in range(0, got, 4)]
    out = []
    for i, s in enumerate(sigs):
        name = struct.pack("<I", s).decode("ascii", "replace")
        data = read_table(name)
        if data:
            out.append((name, i, data))
    dsdt = read_table("DSDT")
    if dsdt:
        out.insert(0, ("DSDT", -1, dsdt))
    return out


def decode_pkglength(data: bytes, off: int) -> tuple[int, int]:
    """解析 PkgLength, 返回 (总长度, 已消耗字节数)."""
    if off >= len(data):
        return 0, 0
    lead = data[off]
    follow = lead >> 6
    if follow == 0:
        return lead & 0x3F, 1
    nbytes = follow
    if off + nbytes >= len(data):
        return 0, 1 + nbytes
    value = lead & 0x0F
    for i in range(nbytes):
        value |= data[off + 1 + i] << (4 + 8 * i)
    return value, 1 + nbytes


def read_name(data: bytes, off: int) -> tuple[str, int]:
    """读 4 字符 NameString, 返回 (名称, 新偏移)."""
    if off + 4 > len(data):
        return "", off + 4
    raw = data[off : off + 4]
    name = "".join(chr(c) if c in NAME_CHARS else "?" for c in raw)
    return name, off + 4


def find_regions(data: bytes) -> list[tuple[str, int, int, int]]:
    """查找所有 OperationRegion, 返回 (名称, 空间, 偏移, 长度)."""
    out = []
    start = 0
    while True:
        i = data.find(OP_REGION, start)
        if i < 0:
            break
        start = i + 2
        p = i + 2
        name, p = read_name(data, p)
        if not name or "?" in name:
            continue
        if p >= len(data):
            continue
        space = data[p]
        out.append((name, space, i, p + 1))
    return out


def parse_fields_for(data: bytes, region_name: str) -> list[tuple[str, int]]:
    """解析引用指定操作区的 Field, 返回 [(字段名, 起始位)]. 只处理 4 字符名的简单情况."""
    results: list[tuple[str, int]] = []
    start = 0
    target = region_name.encode("ascii", errors="replace")
    while True:
        i = data.find(OP_FIELD, start)
        if i < 0:
            break
        start = i + 2
        # 5B 81 <PkgLength> <NameString(region)> <Flags>
        pkglen, used = decode_pkglength(data, i + 2)
        p = i + 2 + used
        rname, p = read_name(data, p)
        if rname != region_name:
            continue
        if p >= len(data):
            continue
        flags = data[p]
        p += 1
        bit_offset = 0
        end = min(i + 2 + pkglen, len(data))
        while p < end:
            op = data[p]
            if op == 0x00:  # ReservedField
                p += 1
                rlen, used2 = decode_pkglength(data, p)
                p += used2
                bit_offset += rlen
            elif op == 0x01:  # AccessField
                p += 3
            elif op == 0x02:  # ConnectField
                p += 1
                if p < len(data) and data[p] == 0x11:
                    clen, used2 = decode_pkglength(data, p + 1)
                    p += 1 + used2 + clen
                else:
                    p += 1
            elif op == 0x03:  # ExtendedAccessField
                p += 4
            else:
                fname, np = read_name(data, p)
                if "?" in fname or not fname:
                    break
                p = np
                width_bytes = 1 << (flags & 0x03)
                if p + width_bytes > len(data):
                    break
                width = int.from_bytes(data[p : p + width_bytes], "little")
                p += width_bytes
                results.append((fname, bit_offset))
                bit_offset += width
    return results


def main() -> int:
    print("=== 提取固件 EC 寄存器地图 ===\n")
    tables = enum_all_tables()

    ec_regions: list[tuple[str, str, int]] = []
    for name, idx, data in tables:
        for rname, space, _, _ in find_regions(data):
            if space == 3:  # EmbeddedControl
                ec_regions.append((f"{name}#{idx}", rname, idx))
    print(f"找到 {len(ec_regions)} 个 EmbeddedControl 操作区")

    # 去重 (同名区在多张表里重复)
    seen = {}
    for tbl, rname, idx in ec_regions:
        seen.setdefault(rname, []).append(tbl)
    print("操作区名称:", ", ".join(sorted(seen)))

    for rname in sorted(seen):
        print(f"\n{'=' * 70}")
        print(f"操作区 {rname}  (出现于: {', '.join(seen[rname][:6])})")
        print("=" * 70)
        all_fields: dict[str, int] = {}
        for name, idx, data in tables:
            for fname, bitoff in parse_fields_for(data, rname):
                if fname not in all_fields:
                    all_fields[fname] = bitoff
        if not all_fields:
            print("  (未解析出字段)")
            continue
        print(f"  共 {len(all_fields)} 个字段:")
        for fname in sorted(all_fields, key=lambda k: all_fields[k]):
            bitoff = all_fields[fname]
            byteoff = bitoff // 8
            print(f"    {fname:<6} 位偏移 {bitoff:>5}  字节 0x{byteoff:03X} ({byteoff})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
