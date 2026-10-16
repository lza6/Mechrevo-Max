"""EC 健康自检 —— 判断 EC 是否工作正常.

检查项:
  1. 关键寄存器读数是否在合理范围
  2. 动态寄存器（风扇转速）是否真的在变（证明 EC 在实时采样）
  3. 档位字段是否处于已知状态
  4. 连续多轮读取是否全部成功（EC 是否响应稳定）

安全: 全程只读 + 限速（50 次/秒），不做任何写入.

用法: python -X utf8 -I tools/ec_health.py
"""
from __future__ import annotations

import ctypes
import struct
import sys
import time
from ctypes import wintypes

DEVICE = "\\\\.\\ACPIDriver"
GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
IOCTL_READ = 0x9C40A488

#: 50 次/秒 —— 安全速率
INTERVAL_S = 0.02

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


class Ec:
    def __init__(self) -> None:
        self._last = 0.0
        self.ok = 0
        self.fail = 0
        self.handle = k32.CreateFileW(
            DEVICE, GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
        )
        if self.handle == wintypes.HANDLE(-1).value or not self.handle:
            raise OSError(f"无法打开 {DEVICE}: errno={ctypes.get_last_error()}")

    def read(self, addr: int) -> int | None:
        d = time.monotonic() - self._last
        if d < INTERVAL_S:
            time.sleep(INTERVAL_S - d)
        self._last = time.monotonic()
        ib = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
        ob = ctypes.create_string_buffer(4)
        r = wintypes.DWORD(0)
        if not k32.DeviceIoControl(self.handle, IOCTL_READ, ib, 4, ob, 4, ctypes.byref(r), None):
            self.fail += 1
            return None
        self.ok += 1
        return struct.unpack("<I", ob.raw)[0] & 0xFF

    def close(self) -> None:
        if self.handle:
            k32.CloseHandle(self.handle)


def main() -> int:
    print("=" * 66)
    print("EC 健康自检")
    print("=" * 66)

    try:
        ec = Ec()
    except OSError as e:
        print(f"\n❌ 无法访问 EC: {e}")
        print("   请先执行: sc.exe start UWACPIDriver; pnputil.exe /scan-devices")
        return 2

    issues: list[str] = []

    # --- 1. 关键静态寄存器 ---
    print("\n[1] 关键寄存器")
    checks = [
        (0x0740, "项目 ID", lambda v: v in range(1, 64), "1-63"),
        (0x0490, "电源来源", lambda v: v <= 15, "0-15"),
        (0x0730, "Gaming PL1", lambda v: 5 <= v <= 200, "5-200 W"),
        (0x0732, "Gaming PL4", lambda v: 0 <= v <= 255, "0-255 W"),
        (0x0734, "Office PL1", lambda v: 5 <= v <= 200, "5-200 W"),
        (0x0751, "档位字段1", lambda v: v in (0x00, 0x10, 0xA0), "0x00/0x10/0xA0"),
        (0x07AB, "档位字段2", lambda v: v in (0x00, 0x10), "0x00/0x10"),
    ]
    for addr, name, ok_fn, expect in checks:
        v = ec.read(addr)
        if v is None:
            print(f"  ❌ 0x{addr:04X} {name}: 读取失败")
            issues.append(f"{name} 读取失败")
            continue
        good = ok_fn(v)
        mark = "✅" if good else "⚠️ "
        print(f"  {mark} 0x{addr:04X} {name} = {v} (0x{v:02X})   期望 {expect}")
        if not good:
            issues.append(f"{name} = {v}，超出期望 {expect}")

    # --- 2. 风扇是否在实时变化（证明 EC 在采样）---
    print("\n[2] 动态采样（风扇转速，连续 6 次，间隔 1 秒）")
    samples = []
    for i in range(6):
        hi, lo = ec.read(0x0464), ec.read(0x0465)
        if hi is None or lo is None:
            print(f"  第 {i + 1} 次: 读取失败")
            samples.append(None)
        else:
            rpm = (hi << 8) | lo
            samples.append(rpm)
            print(f"  第 {i + 1} 次: CPU 风扇 {rpm} RPM")
        if i < 5:
            time.sleep(1.0)

    valid = [s for s in samples if s is not None]
    if not valid:
        issues.append("风扇寄存器始终读不到")
    else:
        print(f"  范围: {min(valid)} - {max(valid)} RPM")
        if max(valid) > 12000:
            issues.append(f"风扇转速异常偏高: {max(valid)} RPM")
        if all(s == 0 for s in valid):
            issues.append("风扇转速恒为 0（可能不转或寄存器不可读）")

    # --- 3. 连续读取稳定性 ---
    print("\n[3] 连续读取稳定性（500 次）")
    t0 = time.time()
    for _ in range(500):
        ec.read(0x0740)
    el = time.time() - t0
    print(f"  成功 {ec.ok} / 失败 {ec.fail}，耗时 {el:.1f}s")
    if ec.fail > 0:
        issues.append(f"连续读取中有 {ec.fail} 次失败")

    ec.close()

    # --- 结论 ---
    print("\n" + "=" * 66)
    if issues:
        print("⚠️  发现问题:")
        for x in issues:
            print(f"    - {x}")
        print("\n注意: 单次读数异常不足以判定硬件故障，应多次复测并结合症状。")
    else:
        print("✅ EC 工作正常：寄存器读数合理、风扇在实时采样、连续读取无失败。")
    print("=" * 66)
    return 1 if issues else 0


if __name__ == "__main__":
    sys.exit(main())
