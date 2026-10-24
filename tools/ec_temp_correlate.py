"""EC 温度寄存器相关性探测（只读，分阶段，**不并发轮询 ACPI**）.

# 为什么要分阶段

实测发现：`MSAcpi_ThermalZoneTemperature`（WMI）和
`\\Thermal Zone Information(*)\\Temperature`（性能计数器）**都会经过同一条 EC 通道**。
在它们轮询期间，直连 EC 的读取从 50 次/秒掉到 ~0.03 次/秒（单次读取约 1.5 s，慢 75 倍）。
因此参照温度只能在**阶段边界**读一次，不能与 EC 采样并行。

# 实验设计（三步，全程只读）

    A. 静置  60 s：EC 每 2 s 采一轮           → 基线
    B. 温和升温 40 s：**单线程**忙循环（≈1/24 核 ≈ 4% CPU，不是满载压测）
      期间只采 EC，不读 ACPI                  → 温度应上升
    C. 冷却  60 s：EC 每 2 s 采一轮           → 温度应回落

在 A 末、B 末、C 末各读一次 ACPI 参照温度。
判定：某个地址的值沿 A→B→C 走出「平 → 升 → 降」且幅度与参照温度变化同向，
才可能是温度寄存器。**只按数值范围猜是不允许的。**

# 安全

- 只读；
- EC 读取速率 50 次/秒（硬上限 100 次/秒，内部夹取）；
- 不对 CPU/GPU 施加载荷：B 段只有 1 个 Python 线程空转；
- 任一步出错立即停止，不重试堆积。

用法:
  python -X utf8 -I tools/ec_temp_correlate.py --addr 0x300 0x320 0x3A5 --csv out.csv
  python -X utf8 -I tools/ec_temp_correlate.py --auto     # 自动挑候选（见 --auto 说明）
"""
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ec_temp_probe import DEFAULT_INTERVAL_S, Ec, parse_addrs  # noqa: E402

POWERSHELL = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"


def read_reference_temp() -> float | None:
    """单次读 ACPI 热区温度（°C）。失败返回 None，绝不重试堆积."""
    script = (
        "try { $s=(Get-Counter '\\Thermal Zone Information(*)\\Temperature' -ErrorAction Stop).CounterSamples;"
        " $c=($s | Where-Object { $_.InstanceName -like '*ectz*' } | Select-Object -First 1).CookedValue;"
        " if ($c) { [math]::Round($c-273.15,2) } else { '' } } catch { '' }"
    )
    try:
        out = subprocess.run(
            [POWERSHELL, "-NoProfile", "-Command", script],
            capture_output=True, text=True, timeout=60, check=False,
        )
        text = out.stdout.strip().replace("\ufeff", "")
        return float(text) if text else None
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def sample_block(ec: Ec, addrs: list[int], seconds: float, period: float, rows: list[dict]) -> None:
    """在 seconds 秒内每 period 秒采一轮（EC 独占，不读 ACPI）."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        row = {"ts": round(time.time(), 3)}
        for addr in addrs:
            row[f"0x{addr:04X}"] = ec.read(addr)
        rows.append(row)
        remaining = period - len(addrs) * ec.interval
        if remaining > 0:
            time.sleep(remaining)


def gentle_load(seconds: float) -> None:
    """单线程空转 —— 约 1/核数 的 CPU 占用，不构成满载压测."""
    stop = time.monotonic() + seconds
    while time.monotonic() < stop:
        pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="EC 温度相关性探测（只读、分阶段）")
    parser.add_argument("--addr", nargs="+", help="候选地址，如 0x300 0x320 0x3A5-0x3A6")
    parser.add_argument("--csv", required=True)
    parser.add_argument("--baseline-s", type=float, default=60.0)
    parser.add_argument("--load-s", type=float, default=40.0)
    parser.add_argument("--cooldown-s", type=float, default=60.0)
    parser.add_argument("--period", type=float, default=2.0)
    args = parser.parse_args(argv)

    if not args.addr:
        parser.error("必须给 --addr（先跑 ec_temp_probe.py scan 挑候选）")

    addrs = parse_addrs(args.addr)
    ec = Ec(DEFAULT_INTERVAL_S)
    rows: list[dict] = []
    refs: list[dict] = []
    try:
        print(f"候选 {len(addrs)} 个地址；EC 采样 {50:.0f} 次/秒（只读）")
        print("阶段 A：静置基线 …", flush=True)
        sample_block(ec, addrs, args.baseline_s, args.period, rows)
        refs.append({"phase": "A_idle", "t": round(time.time(), 3), "ref_c": read_reference_temp()})
        print(f"  参照温度 = {refs[-1]['ref_c']} °C", flush=True)

        print(f"阶段 B：单线程升温 {args.load_s:.0f}s（≈4% CPU）…", flush=True)
        worker = threading.Thread(target=gentle_load, args=(args.load_s,), daemon=True)
        worker.start()
        sample_block(ec, addrs, args.load_s, args.period, rows)
        worker.join()
        refs.append({"phase": "B_load", "t": round(time.time(), 3), "ref_c": read_reference_temp()})
        print(f"  参照温度 = {refs[-1]['ref_c']} °C", flush=True)

        print("阶段 C：冷却 …", flush=True)
        sample_block(ec, addrs, args.cooldown_s, args.period, rows)
        refs.append({"phase": "C_cool", "t": round(time.time(), 3), "ref_c": read_reference_temp()})
        print(f"  参照温度 = {refs[-1]['ref_c']} °C", flush=True)
    finally:
        ec.close()

    path = Path(args.csv)
    with path.open("w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)
    ref_path = path.with_suffix(".ref.json")
    ref_path.write_text(json.dumps(refs, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"\n采样 {len(rows)} 组 → {path}")
    print(f"参照温度 → {ref_path}")
    print(f"读取成功 {ec.ok}，失败 {ec.fail}")

    # 粗筛：按三段均值变化排序，打印最像温度的地址
    if len(rows) >= 6:
        third = max(1, len(rows) // 3)
        heads = {"A": rows[:third], "B": rows[third:2 * third], "C": rows[2 * third:]}
        stats = []
        for key in rows[0]:
            if key == "ts":
                continue
            means = {
                name: sum(r[key] for r in part if r[key] is not None) / max(1, len([r for r in part if r[key] is not None]))
                for name, part in heads.items()
            }
            stats.append((key, means["A"], means["B"], means["C"]))
        print("\n地址       A(静置)  B(升温)  C(冷却)   B-A    C-B")
        for key, a, b, c in sorted(stats, key=lambda s: -(abs(s[2] - s[1]) + abs(s[3] - s[2])))[:15]:
            print(f"{key}   {a:7.2f}  {b:7.2f}  {c:7.2f}  {b - a:+6.2f}  {c - b:+6.2f}")
    return 0 if ec.fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
