"""模式切换实验 —— 带严格回滚保障.

目的: 确认本机 0x751 / 0x7AB / 0x767 是否为有效的档位控制寄存器.

安全措施:
  1. 运行前完整快照所有涉及寄存器
  2. 任何异常/中断都通过 finally 恢复原值
  3. 每步写入后读回校验
  4. 全程打印, 便于人工判断行为变化

用法:
  python -X utf8 -I tools/ec_mode_exp.py            # 只读, 显示当前档位状态
  python -X utf8 -I tools/ec_mode_exp.py --apply    # 执行切换实验 (会短暂改变风扇/功耗行为)
"""
import ctypes
import struct
import sys
import time
from ctypes import wintypes

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
IOCTL_READ = 0x9C40A488
IOCTL_WRITE = 0x9C40A48C

DEVICE = "\\\\.\\ACPIDriver"

# 档位控制涉及的全部寄存器
MODE_REGS = {
    0x0726: "辅助覆盖 (bit7)",
    0x0727: "自定义模式 (bit5/bit6)",
    0x0751: "档位字段 1",
    0x0767: "提交寄存器",
    0x07AB: "档位字段 2",
    0x0464: "CPU 风扇 RPM 高",
    0x0465: "CPU 风扇 RPM 低",
    0x046C: "GPU 风扇 RPM 高",
    0x046B: "GPU 风扇 RPM 低",
}

# LumaDesk 记录的档位编码 (EC 项目 0x1A)
MODES = {
    "balance": {0x0751: 0x00, 0x07AB: 0x00},
    "office": {0x0751: 0xA0, 0x07AB: 0x00},
    "turbo": {0x0751: 0x10, 0x07AB: 0x10},
}

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


def open_ec() -> int:
    h = k32.CreateFileW(
        DEVICE, GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
    )
    if h == wintypes.HANDLE(-1).value or not h:
        raise OSError(
            f"无法打开 {DEVICE}: errno={ctypes.get_last_error()}\n"
            "请先执行: sc.exe start UWACPIDriver; pnputil.exe /scan-devices"
        )
    return h


def read_ec(h: int, addr: int) -> int:
    ib = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
    ob = ctypes.create_string_buffer(4)
    r = wintypes.DWORD(0)
    if not k32.DeviceIoControl(h, IOCTL_READ, ib, 4, ob, 4, ctypes.byref(r), None):
        return -1
    return struct.unpack("<I", ob.raw)[0] & 0xFF


def write_ec(h: int, addr: int, value: int) -> None:
    ib = ctypes.create_string_buffer(struct.pack("<II", addr, value & 0xFF), 8)
    ob = ctypes.create_string_buffer(4)
    r = wintypes.DWORD(0)
    k32.DeviceIoControl(h, IOCTL_WRITE, ib, 8, ob, 4, ctypes.byref(r), None)
    time.sleep(0.01)


def snapshot(h: int) -> dict[int, int]:
    return {a: read_ec(h, a) for a in MODE_REGS}


def show(label: str, snap: dict[int, int]) -> None:
    print(f"--- {label} ---")
    for a, name in MODE_REGS.items():
        v = snap[a]
        print(f"  0x{a:04X}  {v:>5}  0x{v:02X}   {name}")
    cpu = (snap[0x0464] << 8) | snap[0x0465]
    gpu = (snap[0x046C] << 8) | snap[0x046B]
    print(f"  => CPU 风扇 {cpu} RPM, GPU 风扇 {gpu} RPM")


def identify(snap: dict[int, int]) -> str:
    v751 = snap[0x0751]
    for name, regs in MODES.items():
        if v751 == regs[0x0751]:
            return name
    return f"未知 (751=0x{v751:02X})"


def main() -> int:
    apply_changes = "--apply" in sys.argv

    h = open_ec()
    before = snapshot(h)
    show("当前状态", before)
    print(f"  => 识别为: {identify(before)}")

    if not apply_changes:
        print("\n(只读模式。加 --apply 执行切换实验)")
        k32.CloseHandle(h)
        return 0

    print("\n=== 开始切换实验 ===")
    try:
        for mode in ("office", "turbo", "balance"):
            regs = MODES[mode]
            print(f"\n>>> 切换到 {mode}: {[(hex(a), hex(v)) for a, v in regs.items()]}")

            for addr, val in regs.items():
                write_ec(h, addr, val)
            write_ec(h, 0x0767, 0x01)  # 提交

            time.sleep(0.3)
            after = snapshot(h)
            ok = all(read_ec(h, a) == v for a, v in regs.items())
            show(f"写入 {mode} 后", after)
            print(f"  => 寄存器读回{'一致 ✅' if ok else '不一致 ❌'}")
            print(f"  => 识别为: {identify(after)}")

            time.sleep(2)
            later = snapshot(h)
            cpu = (later[0x0464] << 8) | later[0x0465]
            gpu = (later[0x046C] << 8) | later[0x046B]
            print(f"  => 2 秒后风扇: CPU {cpu} RPM, GPU {gpu} RPM")

    finally:
        print("\n=== 恢复原始状态 ===")
        for addr, val in before.items():
            if val >= 0:
                write_ec(h, addr, val)
                got = read_ec(h, addr)
                status = "✅" if got == val else f"❌ 读到 {got}"
                print(f"  0x{addr:04X} <- 0x{val:02X} {status}")
        final = snapshot(h)
        restored = all(final[a] == v for a, v in before.items() if v >= 0)
        print(f"\n最终状态{'已完全恢复 ✅' if restored else '未完全恢复 ❌'}")
        show("最终", final)
        k32.CloseHandle(h)

    return 0


if __name__ == "__main__":
    sys.exit(main())
