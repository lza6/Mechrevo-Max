"""极限与稳定性测试 —— 面向真实的失败模式（**带强制限速**）.

# ⚠️ 安全前提：EC 必须限速访问

EC（Embedded Controller）是一颗 8051 级单片机，通过 LPC/eSPI 与主机相连。
它同时负责**风扇控制与温控**。以数百次/秒的频率泛洪 IOCTL 可能让它停止响应：
EC 一挂，风扇/温控即失控，平台会以"无转储的硬复位"方式关机
（Windows 事件日志表现为 Event 41 Kernel-Power，无 BugCheck、无 WHEA）。

这不是理论风险 —— 本项目开发过程中出现过一次这样的意外关机。

因此本脚本**强制限速**，默认 20 次/秒（远低于危险阈值），
且 `--max-rate` 上限被硬性限制在 100 次/秒，防止再次误用。

另外：**不要在笔记本上做 CPU 满载压力测试**。i9-12900HX 全核满载会让机器
逼近温度墙，同样可能触发无转储的硬复位。

桌面硬件工具的"压测"不是 QPS，而是：
  1. 长时**低速**轮询是否稳定、是否泄漏句柄
  2. 反复写入/复原后硬件状态是否仍然精确一致（无漂移）
  3. 设备中途消失时是否优雅失败而非崩溃
  4. 边界值/非法值是否被正确拦截

用法:
  python -X utf8 -I tools/ec_stress.py read            # 只读压测（默认，限速）
  python -X utf8 -I tools/ec_stress.py write           # 读写往复压测（会短暂改档位）
  python -X utf8 -I tools/ec_stress.py edge            # 边界/非法值测试（不写硬件）
  python -X utf8 -I tools/ec_stress.py soak 10         # 长时轮询 10 分钟（默认）
"""
from __future__ import annotations

import ctypes
import json
import struct
import sys
import time
from ctypes import wintypes
from pathlib import Path

# ---------------- 安全限速 ----------------

#: 默认访问间隔（秒）。20 次/秒对 EC 是安全的，与官方控制台的轮询频率同量级。
DEFAULT_INTERVAL_S = 0.05

#: 允许的最小间隔（= 最高 100 次/秒）。再快就有让 EC 停摆的风险。
MIN_INTERVAL_S = 0.01


def clamp_interval(interval: float) -> float:
    """把调用方给的间隔夹到安全范围，并告知被夹取。"""
    if interval < MIN_INTERVAL_S:
        print(
            f"⚠️  请求的间隔 {interval:.4f}s 过小（>={1 / MIN_INTERVAL_S:.0f} 次/秒），"
            f"已强制调整为 {MIN_INTERVAL_S:.2f}s。\n"
            "    原因：EC 是单片机，高速泛洪会令其停止响应，进而导致风扇/温控失控与硬复位。"
        )
        return MIN_INTERVAL_S
    return interval


GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
IOCTL_READ = 0x9C40A488
IOCTL_WRITE = 0x9C40A48C
DEVICE = "\\\\.\\ACPIDriver"

# 已验证的只读寄存器
READ_ADDRS = [0x0740, 0x0490, 0x0464, 0x0465, 0x046C, 0x046B, 0x0730, 0x0731, 0x0732, 0x0734, 0x0735, 0x0736]
# 档位相关（写入压测用）
MODE_ADDRS = [0x0751, 0x07AB, 0x0767]

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

# GetProcessHandleCount 必须显式声明签名，否则调用静默失败并返回 0，
# 会让"句柄泄漏"检查变成永远通过的假保证。
k32.GetCurrentProcess.restype = wintypes.HANDLE
k32.GetCurrentProcess.argtypes = []
k32.GetProcessHandleCount.restype = wintypes.BOOL
k32.GetProcessHandleCount.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]


class Ec:
    """最小 EC 访问器（**强制限速**）.

    每次访问后按 `interval` 休眠，确保不会泛洪 EC。
    限速在类内部强制执行，调用方无法绕过 —— 这是刻意的设计。
    """

    def __init__(self, interval: float = DEFAULT_INTERVAL_S) -> None:
        self.interval = clamp_interval(interval)
        self.handle = self._open()
        self._last = 0.0

    def _open(self) -> int:
        h = k32.CreateFileW(
            DEVICE, GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None,
        )
        if h == wintypes.HANDLE(-1).value or not h:
            raise OSError(f"无法打开 {DEVICE}: errno={ctypes.get_last_error()}")
        return h

    def _throttle(self) -> None:
        """保证两次访问之间至少间隔 self.interval 秒。"""
        delta = time.monotonic() - self._last
        if delta < self.interval:
            time.sleep(self.interval - delta)
        self._last = time.monotonic()

    def read(self, addr: int) -> int:
        self._throttle()
        ib = ctypes.create_string_buffer(struct.pack("<I", addr), 4)
        ob = ctypes.create_string_buffer(4)
        r = wintypes.DWORD(0)
        if not k32.DeviceIoControl(self.handle, IOCTL_READ, ib, 4, ob, 4, ctypes.byref(r), None):
            raise OSError(f"读取 0x{addr:04X} 失败: errno={ctypes.get_last_error()}")
        return struct.unpack("<I", ob.raw)[0] & 0xFF

    def write(self, addr: int, value: int) -> None:
        self._throttle()
        ib = ctypes.create_string_buffer(struct.pack("<II", addr, value & 0xFF), 8)
        ob = ctypes.create_string_buffer(4)
        r = wintypes.DWORD(0)
        k32.DeviceIoControl(self.handle, IOCTL_WRITE, ib, 8, ob, 4, ctypes.byref(r), None)

    def close(self) -> None:
        if self.handle:
            k32.CloseHandle(self.handle)
            self.handle = None


def handle_count() -> int:
    """本进程当前句柄数（用于检测泄漏）.

    若调用失败返回 -1 —— 绝不返回 0 假装"没有句柄"，那会让泄漏检查失真.
    """
    n = wintypes.DWORD(0)
    if not k32.GetProcessHandleCount(k32.GetCurrentProcess(), ctypes.byref(n)):
        return -1
    return int(n.value)


def test_read(iters: int, interval: float = DEFAULT_INTERVAL_S) -> dict:
    print(f"=== 只读压测: {iters} 轮 × {len(READ_ADDRS)} 地址 ===")
    ec = Ec(interval)
    h0 = handle_count()
    t0 = time.time()
    errors = 0
    total = 0
    first: dict[int, int] = {}

    for i in range(iters):
        for a in READ_ADDRS:
            try:
                v = ec.read(a)
                if i == 0:
                    first[a] = v
                total += 1
            except OSError:
                errors += 1
        if (i + 1) % max(1, iters // 10) == 0:
            print(f"  {i + 1}/{iters} 轮, 错误 {errors}", flush=True)

    elapsed = time.time() - t0
    h1 = handle_count()
    ec.close()

    leak = "测量失败" if (h0 < 0 or h1 < 0) else max(0, h1 - h0)
    res = {
        "迭代": iters, "总读取": total, "错误数": errors,
        "耗时秒": round(elapsed, 2),
        "吞吐(次/秒)": round(total / elapsed, 1) if elapsed else 0,
        "句柄_开始": h0, "句柄_结束": h1,
        "句柄泄漏": leak,
        "句柄测量有效": h0 > 0 and h1 > 0,
    }
    for k, v in res.items():
        print(f"  {k}: {v}")
    return res


def test_write(iters: int, interval: float = DEFAULT_INTERVAL_S) -> dict:
    print(f"=== 读写往复压测: {iters} 轮 ===")
    ec = Ec(interval)

    # 先存初始状态
    original = {a: ec.read(a) for a in MODE_ADDRS}
    print(f"  初始状态: {{{', '.join(f'0x{a:04X}={v:#04x}' for a, v in original.items())}}}")

    errors = 0
    t0 = time.time()
    try:
        for i in range(iters):
            for target in ((0xA0, 0x00), (0x10, 0x10), (0x00, 0x00)):
                ec.write(0x0751, target[0])
                ec.write(0x07AB, target[1])
                ec.write(0x0767, 0x01)
                time.sleep(0.05)
                got = ec.read(0x0751)
                if got != target[0]:
                    errors += 1
            if (i + 1) % max(1, iters // 10) == 0:
                print(f"  {i + 1}/{iters} 轮, 校验失败 {errors}", flush=True)
    finally:
        # 无论成败都还原
        print("  还原初始状态…")
        for a, v in original.items():
            ec.write(a, v)
        restored = {a: ec.read(a) for a in MODE_ADDRS}
        drift = {a: (original[a], restored[a]) for a in MODE_ADDRS if original[a] != restored[a]}
        ec.close()

    elapsed = time.time() - t0
    res = {
        "迭代": iters, "校验失败": errors, "耗时秒": round(elapsed, 2),
        "状态漂移": drift, "还原成功": len(drift) == 0,
    }
    for k, v in res.items():
        print(f"  {k}: {v}")
    return res


def test_edge(interval: float = DEFAULT_INTERVAL_S) -> dict:
    """边界与非法值测试（不写硬件）.

    区分两个概念，二者不可混淆：
      - encode_ok : 值能否被 EC 协议编码（地址 u16 / 值 u8）
      - allowed   : 是否在该寄存器的**安全语义范围**内（由上层校验，越界必须拦截）
    """
    print("=== 边界与语义范围测试（不写硬件）===")

    # (用例名, 地址, 值, 期望可编码, 期望语义合法)
    cases = [
        ("PL1=0（低于安全下限）",   0x0730, 0,    True,  False),
        ("PL1=4（低于安全下限）",   0x0730, 4,    True,  False),
        ("PL1=5（下限）",           0x0730, 5,    True,  True),
        ("PL1=75（典型值）",        0x0730, 75,   True,  True),
        ("PL1=200（上限）",         0x0730, 200,  True,  True),
        ("PL1=201（高于安全上限）", 0x0730, 201,  True,  False),
        ("PL1=255（u8 极限）",      0x0730, 255,  True,  False),
        ("PL1=256（超 u8）",        0x0730, 256,  False, False),
        ("PL1=-1（负数）",          0x0730, -1,   False, False),
        ("PL4=0（下限）",           0x0732, 0,    True,  True),
        ("PL4=255（上限）",         0x0732, 255,  True,  True),
        ("PL4=256（超 u8）",        0x0732, 256,  False, False),
        ("地址 0x10000（超 u16）",  0x10000, 0,   False, False),
    ]

    def pl1_pl2_ok(v: int) -> bool:
        return 5 <= v <= 200

    def pl4_ok(v: int) -> bool:
        return 0 <= v <= 255

    results = []
    for name, addr, val, exp_encode, exp_allowed in cases:
        encode_ok = (0 <= addr <= 0xFFFF) and (0 <= val <= 0xFF)
        if not encode_ok:
            allowed = False
        elif addr in (0x0730, 0x0731):
            allowed = pl1_pl2_ok(val)
        elif addr in (0x0732, 0x0734, 0x0735, 0x0736):
            allowed = pl4_ok(val)
        else:
            allowed = True
        ok = (encode_ok == exp_encode) and (allowed == exp_allowed)
        results.append({"用例": name, "可编码": encode_ok, "语义合法": allowed, "通过": ok})
        print(
            f"  [{'PASS' if ok else 'FAIL'}] {name}: "
            f"编码={encode_ok}(期望{exp_encode}) 语义={allowed}(期望{exp_allowed})"
        )

    # 实读值的语义范围校验
    ec = Ec(interval)
    pl1 = ec.read(0x0730)
    pl2 = ec.read(0x0731)
    pl4 = ec.read(0x0732)
    sem_ok = pl1_pl2_ok(pl1) and pl1_pl2_ok(pl2) and pl4_ok(pl4)
    print(f"  [{'PASS' if sem_ok else 'FAIL'}] 实读值在语义范围内: PL1={pl1} PL2={pl2} PL4={pl4}")
    results.append({"用例": "实读值语义范围", "通过": sem_ok})
    ec.close()

    passed = sum(1 for r in results if r["通过"])
    print(f"  合计: {passed}/{len(results)} 通过")
    return {"通过": passed, "总数": len(results), "明细": results}


def test_soak(minutes: float, interval: float = DEFAULT_INTERVAL_S) -> dict:
    print(f"=== 长时轮询压测: {minutes} 分钟 ===")
    ec = Ec(interval)
    end = time.time() + minutes * 60
    rounds = 0
    errors = 0
    samples: list[tuple[float, int]] = []
    try:
        while time.time() < end:
            try:
                v = ec.read(0x0464) << 8 | ec.read(0x0465)
                samples.append((time.time(), v))
                rounds += 1
            except OSError:
                errors += 1
            time.sleep(2.0)
    except KeyboardInterrupt:
        print("  用户中断")
    finally:
        ec.close()

    if samples:
        rpms = [s[1] for s in samples]
        print(f"  轮数={rounds} 错误={errors}")
        print(f"  CPU 风扇 RPM: 最小={min(rpms)} 最大={max(rpms)} 平均={sum(rpms) // len(rpms)}")
    return {"轮数": rounds, "错误": errors, "样本数": len(samples)}


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "read"
    args = sys.argv[2:]

    # 允许 --interval 覆盖，但会被 clamp_interval 夹到安全范围
    interval = DEFAULT_INTERVAL_S
    if "--interval" in args:
        i = args.index("--interval")
        try:
            interval = float(args[i + 1])
        except (IndexError, ValueError):
            print("--interval 需要一个秒数参数")
            return 2
        args = args[:i] + args[i + 2:]

    print(
        f"限速间隔: {clamp_interval(interval):.3f}s"
        f" (≈{1 / clamp_interval(interval):.0f} 次/秒，上限 {1 / MIN_INTERVAL_S:.0f} 次/秒)\n"
    )

    try:
        if mode == "read":
            # 默认值刻意保守：500 轮 × 12 地址 @20 次/秒 ≈ 5 分钟
            n = int(args[0]) if args else 500
            res = test_read(n, interval)
            ok = res["错误数"] == 0 and res["句柄测量有效"] and res["句柄泄漏"] == 0
        elif mode == "write":
            n = int(args[0]) if args else 10
            res = test_write(n, interval)
            ok = res["校验失败"] == 0 and res["还原成功"]
        elif mode == "edge":
            res = test_edge(interval)
            ok = res["通过"] == res["总数"]
        elif mode == "soak":
            m = float(args[0]) if args else 10
            res = test_soak(m, interval)
            ok = res["错误"] == 0
        else:
            print(__doc__)
            return 2
    except OSError as e:
        print(f"\n❌ EC 访问失败: {e}")
        print("请先执行: sc.exe start UWACPIDriver; pnputil.exe /scan-devices")
        return 2

    out = Path(__file__).resolve().parent.parent / "reverse" / "evidence" / "stress-result.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"mode": mode, "ok": ok, "result": res}, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n{'✅ 通过' if ok else '❌ 失败'}  (结果已存 {out.name})")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
