"""EC 寄存器全量只读扫描 —— 建立本机 EC 地址地图.

# ⚠️ 限速说明（重要）

EC 是 8051 级单片机，同时负责风扇与温控。**以数百次/秒泛洪 IOCTL 会让它停止响应**，
进而导致风扇/温控失控、平台以"无转储硬复位"方式关机
（Windows 事件日志为 Event 41 Kernel-Power，无 BugCheck / 无 WHEA）。

本脚本因此**强制限速**：
  - 默认 50 次/秒（`--interval 0.02`），全量 4096 地址约需 82 秒
  - 最快 100 次/秒（`--interval 0.01`），再快会被强制夹回

安全说明:
  - 仅发送 ECREAD IOCTL (0x9C40A488), 不做任何写入
  - 结果保存为 JSON, 供后续对比与协议分析

用法: python -X utf8 -I tools/ec_scan.py [起始] [结束] [--delta] [--interval 秒]
  --delta     连续读两轮, 标记值发生变化的地址 (找出动态寄存器)
  --interval  两次访问之间的间隔秒数, 默认 0.02 (50 次/秒)
"""
import ctypes
import json
import struct
import sys
import time
from ctypes import wintypes
from pathlib import Path

#: 默认访问间隔（秒）= 50 次/秒
DEFAULT_INTERVAL_S = 0.02
#: 允许的最小间隔 = 100 次/秒，再快有让 EC 停摆的风险
MIN_INTERVAL_S = 0.01

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
IOCTL_GPD_ACPI_ECREAD = 0x9C40A488

DEVICE_PATH = r"\\.\ACPIDriver"
OUT_DIR = Path(__file__).resolve().parent.parent / "reverse" / "evidence"

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


class Ec:
    """EC 只读访问器（**强制限速**，防止泛洪导致 EC 停摆）."""

    def __init__(self, path: str = DEVICE_PATH, interval: float = DEFAULT_INTERVAL_S) -> None:
        if interval < MIN_INTERVAL_S:
            print(
                f"⚠️  间隔 {interval:.4f}s 过小，已强制改为 {MIN_INTERVAL_S:.2f}s"
                f"（上限 {1 / MIN_INTERVAL_S:.0f} 次/秒）。"
                "EC 泛洪会导致风扇/温控失控与硬复位。"
            )
            interval = MIN_INTERVAL_S
        self.interval = interval
        self._last = 0.0
        self.handle = k32.CreateFileW(
            path, GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
        )
        if self.handle == wintypes.HANDLE(-1).value or not self.handle:
            err = ctypes.get_last_error()
            raise OSError(f"无法打开 {path}: errno={err}")

    def read(self, addr: int) -> int | None:
        """读 1 字节（已限速）, 失败返回 None."""
        delta = time.monotonic() - self._last
        if delta < self.interval:
            time.sleep(self.interval - delta)
        self._last = time.monotonic()

        in_buf = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
        out_buf = ctypes.create_string_buffer(4)
        returned = wintypes.DWORD(0)
        ok = k32.DeviceIoControl(
            self.handle, IOCTL_GPD_ACPI_ECREAD,
            in_buf, 4, out_buf, 4, ctypes.byref(returned), None,
        )
        if not ok:
            return None
        return struct.unpack("<I", out_buf.raw)[0] & 0xFF

    def close(self) -> None:
        if self.handle:
            k32.CloseHandle(self.handle)
            self.handle = None


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    do_delta = "--delta" in sys.argv
    start = int(args[0], 0) if len(args) > 0 else 0x0000
    end = int(args[1], 0) if len(args) > 1 else 0x0FFF

    print(f"=== EC 全量只读扫描 0x{start:04X}-0x{end:04X} ===")
    ec = Ec()
    print("设备已打开\n")

    total = end - start + 1
    first: dict[int, int] = {}
    t0 = time.time()
    for addr in range(start, end + 1):
        v = ec.read(addr)
        if v is not None:
            first[addr] = v
        if (addr - start) % 512 == 0 and addr > start:
            print(f"  进度 {addr - start}/{total} ...", flush=True)

    elapsed = time.time() - t0
    nonzero = {a: v for a, v in first.items() if v != 0}
    print(f"\n读取完成: {len(first)}/{total} 个地址, 耗时 {elapsed:.1f}s")
    print(f"非零地址: {len(nonzero)} 个\n")

    print("--- 非零地址 (0x0000-0x03FF) ---")
    for a in sorted(nonzero):
        if a < 0x400:
            print(f"  0x{a:04X} ({a:5d}) = 0x{nonzero[a]:02X} ({nonzero[a]:3d})")

    print("\n--- 非零地址 (0x0400-0x0FFF) ---")
    for a in sorted(nonzero):
        if a >= 0x400:
            print(f"  0x{a:04X} ({a:5d}) = 0x{nonzero[a]:02X} ({nonzero[a]:3d})")

    delta: dict[int, tuple[int, int]] = {}
    if do_delta:
        print("\n=== 第二轮读取 (找动态寄存器) ===")
        second: dict[int, int] = {}
        for addr in range(start, end + 1):
            v = ec.read(addr)
            if v is not None:
                second[addr] = v
        delta = {
            a: (first[a], second[a])
            for a in first
            if a in second and first[a] != second[a]
        }
        print(f"值发生变化的地址: {len(delta)} 个")
        for a in sorted(delta):
            print(f"  0x{a:04X} ({a:5d}): {delta[a][0]} -> {delta[a][1]}")

    ec.close()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    out = OUT_DIR / "ec-scan.json"
    payload = {
        "range": [start, end],
        "nonce": elapsed,
        "all_values": {f"0x{a:04X}": v for a, v in sorted(first.items())},
        "nonzero": {f"0x{a:04X}": v for a, v in sorted(nonzero.items())},
        "delta": {f"0x{a:04X}": list(v) for a, v in sorted(delta.items())} if do_delta else {},
    }
    out.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n结果已保存: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
