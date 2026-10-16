"""EC 驱动连接探测 —— 只读，不写任何寄存器.

用途: 验证能否打开 \\\\.\\ACPIDriver 设备并成功读取 EC 寄存器.
安全: 本脚本只发送 ECREAD IOCTL (0x9C40A488), 不执行任何写入.

用法: python -I tools/ec_probe.py
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
k32.CloseHandle.restype = wintypes.BOOL
k32.CloseHandle.argtypes = [wintypes.HANDLE]


def open_device(path=r"\\.\ACPIDriver"):
    """打开驱动设备对象, 返回句柄或 None."""
    h = k32.CreateFileW(
        path, GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None, OPEN_EXISTING, 0, None,
    )
    if h == wintypes.HANDLE(-1).value or not h:
        err = ctypes.get_last_error()
        print(f"  CreateFile 失败: errno={err} ({ctypes.FormatError(err).strip()})")
        return None
    return h


def read_ec(h, addr):
    """从 EC 地址读 1 字节. 返回 (成功?, 值)."""
    in_buf = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
    out_buf = ctypes.create_string_buffer(4)
    returned = wintypes.DWORD(0)
    ok = k32.DeviceIoControl(
        h, IOCTL_GPD_ACPI_ECREAD,
        in_buf, 4, out_buf, 4, ctypes.byref(returned), None,
    )
    if not ok:
        return False, None
    return True, struct.unpack("<I", out_buf.raw)[0] & 0xFF


# 已知跨机型通用的只读寄存器 (来自 LumaDesk / roj234 逆向成果)
PROBE_ADDRS = [
    (740, "项目代号 ProjectID"),
    (464, "CPU 风扇 RPM 高字节"),
    (465, "CPU 风扇 RPM 低字节"),
    (1132, "GPU 风扇 RPM 高字节"),
    (1131, "GPU 风扇 RPM 低字节"),
    (1168, "电源来源 ecPowSource"),
    (1856, "项目 ID (roj234)"),
    (1857, "AP 存在标志"),
]


def main():
    print("=== EC 驱动连接探测 (只读) ===")
    print(f"设备路径: \\\\.\\ACPIDriver")
    print(f"IOCTL: 0x{IOCTL_GPD_ACPI_ECREAD:08X} (ECREAD)")
    print()

    h = open_device()
    if not h:
        print()
        print("结论: 无法打开设备. 可能原因:")
        print("  1. UWACPIDriver 服务未启动 (当前状态: Stopped)")
        print("     启动命令: sc start UWACPIDriver")
        print("  2. 需要管理员权限")
        return 1

    print(f"设备已打开, 句柄 = {h}")
    print()
    print(f"{'地址':<10} {'名称':<32} {'值':<8} {'十六进制'}")
    print("-" * 68)

    ok_count = 0
    for addr, name in PROBE_ADDRS:
        ok, val = read_ec(h, addr)
        if ok:
            ok_count += 1
        shown = val if ok else "FAIL"
        hx = f"0x{val:02X}" if ok else "-"
        print(f"0x{addr:04X}     {name:<32} {shown!s:<8} {hx}")

    k32.CloseHandle(h)
    print()
    print(f"成功读取 {ok_count}/{len(PROBE_ADDRS)} 个寄存器")
    return 0 if ok_count else 2


if __name__ == "__main__":
    sys.exit(main())
