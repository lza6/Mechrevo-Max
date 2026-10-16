"""解析官方控制台注册表备份 (GamingCenter2.reg, UTF-16LE).

提取官方应用的能力协商表与配置项, 作为我们复刻功能的边界依据.

用法: python -I tools/parse_reg.py <path.reg>
"""
import re
import sys
from pathlib import Path

# .reg 文件的键值行: "Name"=dword:00000001 / "Name"="string"
RE_KEY = re.compile(r"^\[(.+)\]$")
RE_VAL = re.compile(r'^"([^"]+)"=(.*)$')


def parse_reg(path: Path) -> dict[str, dict[str, object]]:
    """解析 .reg 文件, 返回 {键路径: {值名: 值}}."""
    text = path.read_bytes().decode("utf-16-le", errors="replace")
    # 去掉 BOM 残留
    text = text.lstrip("﻿")
    result: dict[str, dict[str, object]] = {}
    current: str | None = None
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith(";"):
            continue
        m = RE_KEY.match(line)
        if m:
            current = m.group(1)
            result.setdefault(current, {})
            continue
        m = RE_VAL.match(line)
        if m and current is not None:
            name, val = m.group(1), m.group(2)
            if val.startswith("dword:"):
                result[current][name] = int(val[6:], 16)
            elif val.startswith("hex"):
                result[current][name] = val
            else:
                result[current][name] = val.strip('"')
    return result


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    path = Path(sys.argv[1])
    data = parse_reg(path)

    print(f"=== {path.name}: 共 {len(data)} 个注册表键 ===\n")
    for key, values in data.items():
        print(f"[{key}]  ({len(values)} 项)")
        for name, val in values.items():
            if isinstance(val, int):
                print(f"    {name:<48} = {val}  (0x{val:X})")
            else:
                print(f"    {name:<48} = {val!r}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
