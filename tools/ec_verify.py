"""EC 关键寄存器验证 —— 用官方配置交叉核对地址表.

验证逻辑:
  官方注册表 GamingCenter2.reg 记录本机 Gaming PL1/PL2/PL4 = 75/75/200,
  Office = 45/45/200, 且 ProjectID = 16.
  若通过 EC 读出的值与之吻合, 即证明 EC 地址表对本机正确.

安全: 全程只读.
用法: python -X utf8 -I tools/ec_verify.py
"""
import ctypes
import struct
import sys
from ctypes import wintypes

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
IOCTL_GPD_ACPI_ECREAD = 0x9C40A488

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.CreateFileW.restype = wintypes.HANDLE
k32.CreateFileW.argtypes = [
    wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
    wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE,
]
k32.DeviceIoControl.restype = wintypes.BOOL
k32.DeviceIoControl.argtypes = [
    wintypes.HANDLE, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD,
    wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID,
]


def open_ec():
    h = k32.CreateFileW(
        r"\\.\ACPIDriver", GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
    )
    if h == wintypes.HANDLE(-1).value or not h:
        raise OSError(f"打开失败 errno={ctypes.get_last_error()}")
    return h


def read_ec(h, addr: int) -> int:
    in_buf = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
    out_buf = ctypes.create_string_buffer(4)
    returned = wintypes.DWORD(0)
    ok = k32.DeviceIoControl(
        h, IOCTL_GPD_ACPI_ECREAD, in_buf, 4, out_buf, 4, ctypes.byref(returned), None
    )
    if not ok:
        return -1
    return struct.unpack("<I", out_buf.raw)[0] & 0xFF


# (地址, 名称, 期望值或 None)
CHECKS: list[tuple[int, str, str | None]] = [
    (0x0740, "项目 ID (官方 ProjectID=16)", "16"),
    (0x0741, "AP 存在标志", None),
    (0x0490, "电源来源 ecPowSource", None),
    (0x0464, "CPU 风扇 RPM 高", None),
    (0x0465, "CPU 风扇 RPM 低", None),
    (0x046C, "GPU 风扇 RPM 高", None),
    (0x046B, "GPU 风扇 RPM 低", None),
    (0x02E0, "BIOSFuncReg (LumaDesk: 0x476=1142 → 不同机型)", None),
    (0x0476, "BIOSFuncReg (LumaDesk)", None),
    (0x0783, "CPU PL1 (roj234: 1923)", "45 或 75"),
    (0x0784, "CPU PL2 (roj234: 1924)", "45 或 75"),
    (0x0785, "CPU PL4 (roj234: 1925)", "200"),
    (0x0786, "CPU TCC Offset (roj234: 1926)", None),
    (0x0730, "Gaming 默认 PL1 (LumaDesk: 730)", "75"),
    (0x0731, "Gaming 默认 PL2 (LumaDesk: 731)", "75"),
    (0x0732, "Gaming 默认 PL4 (LumaDesk: 732)", "200"),
    (0x0734, "Office 默认 PL1 (LumaDesk: 734)", "45"),
    (0x0735, "Office 默认 PL2 (LumaDesk: 735)", "45"),
    (0x0736, "Office 默认 PL4 (LumaDesk: 736)", "200"),
    (0x07B9, "电池充电上限 (roj234: 1977)", None),
    (0x07D0, "电池上限 (LumaDesk: 7D0)", None),
    (0x0768, "Win 键锁 (LumaDesk: 768 bit0)", None),
    (0x0770, "电池触发寄存器", None),
    (0x07A6, "电池模式", None),
    (0x0751, "强冷 (LumaDesk: 751 bit6)", None),
    (0x07C5, "独立风扇 (LumaDesk: 7C5 bit7)", None),
    (0x0743, "GPU 功耗", None),
    (0x0744, "GPU 功耗", None),
    (0x0746, "GPU 功耗", None),
    (0x06E2, "GPU 频率 (roj234: 3403=0xD4B)", None),
    (0x0D4B, "GPU 核心频率 (roj234: 3403)", None),
]


def main() -> int:
    print("=== EC 关键寄存器验证 (只读) ===\n")
    h = open_ec()

    print(f"{'地址':<9} {'十进制':>6}  {'值':>5} {'HEX':<5} {'期望':<14} 名称")
    print("-" * 95)
    hits = 0
    for addr, name, expect in CHECKS:
        v = read_ec(h, addr)
        if v < 0:
            shown, hx = "FAIL", "-"
        else:
            shown, hx = str(v), f"0x{v:02X}"
        mark = ""
        if expect and v >= 0:
            exp_vals = [int(x) for x in expect.replace(" 或 ", " ").split() if x.isdigit()]
            if exp_vals and v in exp_vals:
                mark = "  ✓ 吻合"
                hits += 1
            elif exp_vals:
                mark = f"  ✗ 期望 {exp_vals}"
        print(f"0x{addr:04X}   {addr:>6}  {shown:>5} {hx:<5} {str(expect or ''):<14} {name}{mark}")

    k32.CloseHandle(h)
    print(f"\n期望值吻合: {hits} 项")
    return 0


if __name__ == "__main__":
    sys.exit(main())
