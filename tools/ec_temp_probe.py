"""EC 温度寄存器探测（**只读**，强制限速）.

用途：用「与 ACPI 热区温度（`MSAcpi_ThermalZoneTemperature`）的同步相关性」来判定
哪些 EC 地址是温度寄存器 —— 而不是靠"数值看起来像温度"去猜。

为什么必须这么干：社区参考实现（roj234/mechrevo_ec_api 44 个 REG_、LumaDesk
ec_protocol.py）**都没有温度寄存器**，本机必须先测出来再登记。

安全约束（见 docs/SPEC.md §5.1）：
  - 只读，不写任何寄存器
  - 速率硬上限 100 次/秒（`MIN_INTERVAL_S`），低于即被夹回，调用方无法绕过
  - 默认 50 次/秒
  - 不对 CPU/GPU 施加载荷；本工具只观察

用法:
  # 1) 全量只读扫描（用于挑候选：值落在参照温度附近的地址）
  python -X utf8 -I tools/ec_temp_probe.py scan --lo 0x0000 --hi 0x0FFF --csv out.csv

  # 2) 对候选地址做时间序列采样（用于与 ACPI 温度做相关性分析）
  python -X utf8 -I tools/ec_temp_probe.py sample --addr 0x600-0x620 0x6CF --samples 60 --interval 2 --csv series.csv

输出 CSV 第一列是 unix 时间戳（秒，浮点），随后每列是一个地址。
"""
from __future__ import annotations

import argparse
import csv
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

#: 默认访问间隔（秒）—— 50 次/秒
DEFAULT_INTERVAL_S = 0.02
#: 硬上限：最小间隔 0.01s = 100 次/秒。再快有让 EC 停摆的风险，不给开关。
MIN_INTERVAL_S = 0.01

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
    """只读 EC 访问器，内部强制限速."""

    def __init__(self, interval: float = DEFAULT_INTERVAL_S) -> None:
        if interval < MIN_INTERVAL_S:
            print(
                f"⚠️  间隔 {interval:.4f}s 过小，已强制夹回 {MIN_INTERVAL_S:.2f}s"
                f"（上限 {1 / MIN_INTERVAL_S:.0f} 次/秒）",
                flush=True,
            )
            interval = MIN_INTERVAL_S
        self.interval = interval
        self._last = 0.0
        self.ok = 0
        self.fail = 0
        self.handle = k32.CreateFileW(
            DEVICE, GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
        )
        if self.handle in (None, wintypes.HANDLE(-1).value, 0xFFFFFFFFFFFFFFFF):
            raise OSError(f"无法打开 {DEVICE}: errno={ctypes.get_last_error()}")

    def close(self) -> None:
        if self.handle:
            k32.CloseHandle(self.handle)
            self.handle = None

    def read(self, addr: int) -> int | None:
        gap = time.monotonic() - self._last
        if gap < self.interval:
            time.sleep(self.interval - gap)
        self._last = time.monotonic()

        buf = struct.pack("<I", addr)
        out = ctypes.create_string_buffer(4)
        returned = wintypes.DWORD(0)
        ok = k32.DeviceIoControl(
            self.handle, IOCTL_READ, buf, len(buf), out, 4,
            ctypes.byref(returned), None,
        )
        if not ok or returned.value < 4:
            self.fail += 1
            return None
        self.ok += 1
        return out.raw[0]


def parse_addrs(tokens: list[str]) -> list[int]:
    addrs: list[int] = []
    for token in tokens:
        if "-" in token:
            lo, hi = token.split("-", 1)
            addrs.extend(range(int(lo, 0), int(hi, 0) + 1))
        else:
            addrs.append(int(token, 0))
    return addrs


def cmd_scan(ec: Ec, lo: int, hi: int, csv_path: str) -> int:
    addrs = list(range(lo, hi + 1))
    started = time.time()
    values: dict[int, int | None] = {}
    for i, addr in enumerate(addrs, 1):
        values[addr] = ec.read(addr)
        if i % 256 == 0:
            print(f"  已读 {i}/{len(addrs)} …", flush=True)
    elapsed = time.time() - started

    with open(csv_path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        writer.writerow(["timestamp", "addr", "value"])
        ts = time.time()
        for addr, value in values.items():
            writer.writerow([f"{ts:.3f}", f"0x{addr:04X}", "" if value is None else value])

    ok = sum(1 for v in values.values() if v is not None)
    print(
        f"\n扫描完成: {lo:#06x}-{hi:#06x} 共 {len(addrs)} 地址，"
        f"成功 {ok}，失败 {ec.fail}，耗时 {elapsed:.1f}s "
        f"（{len(addrs) / max(elapsed, 1e-9):.0f} 次/秒）"
    )
    print(f"CSV 已写入 {csv_path}")
    return 0 if ec.fail == 0 else 1


def cmd_sample(ec: Ec, addrs: list[int], samples: int, interval: float, csv_path: str) -> int:
    header = ["timestamp"] + [f"0x{a:04X}" for a in addrs]
    with open(csv_path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        writer.writerow(header)
        for n in range(samples):
            row = [f"{time.time():.3f}"]
            for addr in addrs:
                value = ec.read(addr)
                row.append("" if value is None else value)
            writer.writerow(row)
            fh.flush()
            print(f"  采样 {n + 1}/{samples} 完成", flush=True)
            if n + 1 < samples:
                time.sleep(max(0.0, interval - len(addrs) * ec.interval))
    print(f"\n采样完成: {samples} 组 × {len(addrs)} 地址 → {csv_path}")
    print(f"读取成功 {ec.ok}，失败 {ec.fail}")
    return 0 if ec.fail == 0 else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="EC 温度寄存器探测（只读、限速）")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_scan = sub.add_parser("scan", help="全量/区间只读扫描，导出 CSV")
    p_scan.add_argument("--lo", type=lambda s: int(s, 0), default=0x0000)
    p_scan.add_argument("--hi", type=lambda s: int(s, 0), default=0x0FFF)
    p_scan.add_argument("--csv", required=True)
    p_scan.add_argument("--interval", type=float, default=DEFAULT_INTERVAL_S)

    p_sample = sub.add_parser("sample", help="对候选地址做时间序列采样")
    p_sample.add_argument("--addr", nargs="+", required=True, help="如 0x600-0x620 0x6CF")
    p_sample.add_argument("--samples", type=int, default=60)
    p_sample.add_argument("--interval", type=float, default=2.0, help="每组之间的间隔（秒）")
    p_sample.add_argument("--csv", required=True)
    p_sample.add_argument("--interval-read", type=float, default=DEFAULT_INTERVAL_S,
                          help="单次寄存器读取之间的最小间隔（秒，硬下限 0.01）")

    args = parser.parse_args(argv)
    ec = Ec(getattr(args, "interval", None) or getattr(args, "interval_read", DEFAULT_INTERVAL_S))
    try:
        if args.cmd == "scan":
            return cmd_scan(ec, args.lo, args.hi, args.csv)
        addrs = parse_addrs(args.addr)
        return cmd_sample(ec, addrs, args.samples, args.interval, args.csv)
    finally:
        ec.close()


if __name__ == "__main__":
    sys.exit(main())
