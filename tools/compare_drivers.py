"""对比 DriverStore 中所有 UWACPIDriver 版本的硬件 ID 与二进制哈希.

用途: 确定哪个版本的驱动能绑定本机实际存在的 ACPI 设备.

用法: python -X utf8 -I tools/compare_drivers.py
"""
import hashlib
import os
import re
import sys
from pathlib import Path

DRIVER_STORE = Path(r"C:\Windows\System32\DriverStore\FileRepository")


def parse_inf(text: str) -> dict[str, object]:
    """从 INF 文本提取关键字段."""
    info: dict[str, object] = {}

    m = re.search(r"DriverVer\s*=\s*(.+)", text)
    info["driver_ver"] = m.group(1).strip() if m else "?"

    m = re.search(r"KmdfLibraryVersion\s*=\s*(.+)", text)
    info["kmdf"] = m.group(1).strip() if m else "?"

    # [Standard.NTamd64] 段的绑定行
    binds: list[str] = []
    in_sec = False
    for raw in text.splitlines():
        s = raw.strip()
        if s.startswith("["):
            in_sec = "Standard" in s and "NT" in s
            continue
        if in_sec and s and not s.startswith(";"):
            binds.append(s)
    info["binds"] = binds

    # 所有含 ACPI\ 的硬件 ID
    hwids = re.findall(r"\bACPI\\[A-Za-z0-9_]+", text)
    info["hwids"] = sorted(set(hwids))

    return info


def main() -> int:
    dirs = sorted(DRIVER_STORE.glob("uwacpidriver.inf_amd64_*"))
    if not dirs:
        print("未找到 uwacpidriver 包")
        return 1

    for d in dirs:
        inf = d / "UWACPIDriver.inf"
        print("=" * 72)
        print(f"目录: {d.name}")
        if not inf.exists():
            print("  (无 INF)")
            continue

        info = parse_inf(inf.read_text(encoding="utf-8", errors="replace"))
        print(f"  DriverVer  : {info['driver_ver']}")
        print(f"  KMDF 版本  : {info['kmdf']}")
        print(f"  绑定行     : {info['binds']}")
        print(f"  硬件 ID    : {info['hwids']}")

        sys_file = d / "UWACPIDriver.sys"
        if sys_file.exists():
            data = sys_file.read_bytes()
            print(f"  SYS SHA256 : {hashlib.sha256(data).hexdigest()}")
            print(f"  SYS 大小   : {len(data):,} 字节")

    print("=" * 72)
    print(f"共 {len(dirs)} 个版本")

    # 与本机驱动目录的实际文件比对
    print("\n本机实际安装位置 C:\\Windows\\System32\\drivers\\UWACPIDriver.sys:")
    actual = Path(r"C:\Windows\System32\drivers\UWACPIDriver.sys")
    if actual.exists():
        data = actual.read_bytes()
        print(f"  SHA256: {hashlib.sha256(data).hexdigest()}")
        print(f"  大小  : {len(data):,} 字节")
    return 0


if __name__ == "__main__":
    sys.exit(main())
