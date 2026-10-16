"""EC 寄存器读取工具 —— 可复用的命令行入口.

用法:
  python -X utf8 -I tools/ec_read.py                 # 读预置的关键寄存器表
  python -X utf8 -I tools/ec_read.py 0x751 0x7AB     # 读指定地址
  python -X utf8 -I tools/ec_read.py --bits 0x751    # 额外打印 8 个位

安全: 只读, 不写任何寄存器.
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

DEVICE = "\\\\.\\ACPIDriver"

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
k32.CloseHandle.argtypes = [wintypes.HANDLE]


# 预置的寄存器表: (地址, 名称, 来源说明)
PRESET: list[tuple[int, str, str]] = [
    (0x0740, "项目 ID", "本机已验证 = 16"),
    (0x0490, "电源来源", "本机已验证 = 7"),
    (0x0464, "CPU 风扇 RPM 高", "本机已验证"),
    (0x0465, "CPU 风扇 RPM 低", "本机已验证"),
    (0x046C, "GPU 风扇 RPM 高", "本机已验证"),
    (0x046B, "GPU 风扇 RPM 低", "本机已验证"),
    (0x0730, "Gaming 默认 PL1", "本机已验证 = 75"),
    (0x0731, "Gaming 默认 PL2", "本机已验证 = 75"),
    (0x0732, "Gaming 默认 PL4", "本机已验证 = 200"),
    (0x0734, "Office 默认 PL1", "本机已验证 = 45"),
    (0x0735, "Office 默认 PL2", "本机已验证 = 45"),
    (0x0736, "Office 默认 PL4", "本机已验证 = 200"),
    (0x0726, "辅助覆盖位 7", "LumaDesk: 726 bit7 覆盖"),
    (0x0727, "自定义模式", "LumaDesk: 727 bit6 覆盖 / roj234: bit5"),
    (0x0751, "风扇/档位控制", "LumaDesk: 档位字段, bit6=强冷"),
    (0x0767, "提交寄存器", "LumaDesk: 改动后提交"),
    (0x0768, "状态标志", "LumaDesk: bit0 Win 键, bit1 灯带"),
    (0x0769, "未知", "LumaDesk 基线 0"),
    (0x076A, "未知", "LumaDesk 基线 50"),
    (0x076B, "未知", "LumaDesk 基线 50"),
    (0x0770, "电池触发", "LumaDesk: 电池触发寄存器"),
    (0x0783, "PL1 (社区)", "本机不适用, 读 0"),
    (0x0784, "PL2 (社区)", "本机不适用, 读 0"),
    (0x0785, "PL4 (社区)", "本机不适用, 读 0"),
    (0x0786, "TCC (社区)", "本机不适用, 读 0"),
    (0x078C, "键盘亮度", "LumaDesk: 78C"),
    (0x07A6, "电池模式", "LumaDesk: 7A6"),
    (0x07AB, "MyFanCCI 模式", "LumaDesk: 档位第二字段"),
    (0x07B9, "电池充电上限", "roj234: 1977"),
    (0x07C5, "独立风扇", "LumaDesk: 7C5 bit7"),
    (0x07D0, "电池上限", "LumaDesk: 7D0"),
    (0x0D4B, "GPU 核心频率 (社区)", "本机不适用, 读 255"),
]


def open_device() -> int:
    """打开 EC 设备, 失败时抛出带修复提示的异常."""
    h = k32.CreateFileW(
        DEVICE, GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
    )
    if h == wintypes.HANDLE(-1).value or not h:
        err = ctypes.get_last_error()
        msg = (
            f"无法打开 {DEVICE}: errno={err}\n"
            "修复步骤:\n"
            "  1) sc.exe start UWACPIDriver\n"
            "  2) pnputil.exe /scan-devices"
        )
        raise OSError(msg)
    return h


def read_ec(h: int, addr: int) -> int | None:
    """读 1 字节, 失败返回 None."""
    in_buf = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
    out_buf = ctypes.create_string_buffer(4)
    returned = wintypes.DWORD(0)
    ok = k32.DeviceIoControl(
        h, IOCTL_GPD_ACPI_ECREAD, in_buf, 4, out_buf, 4, ctypes.byref(returned), None
    )
    if not ok:
        return None
    return struct.unpack("<I", out_buf.raw)[0] & 0xFF


def main() -> int:
    args = sys.argv[1:]
    show_bits = "--bits" in args
    args = [a for a in args if not a.startswith("--")]

    if args:
        targets = [(int(a, 0), "", "") for a in args]
    else:
        targets = PRESET

    h = open_device()
    print(f"{'地址':<8} {'十进制':>6} {'值':>5} {'HEX':<6} 名称")
    print("-" * 78)
    for addr, name, note in targets:
        v = read_ec(h, addr)
        if v is None:
            print(f"0x{addr:04X}   {addr:>6}  FAIL")
            continue
        bits = ""
        if show_bits:
            bits = "  bits=" + "".join(
                str(i) if (v >> i) & 1 else "." for i in range(7, -1, -1)
            )
        label = f"{name}  ({note})" if name else ""
        print(f"0x{addr:04X}   {addr:>6} {v:>5} 0x{v:02X}   {label}{bits}")

    k32.CloseHandle(h)
    return 0


if __name__ == "__main__":
    sys.exit(main())
