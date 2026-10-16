"""枚举 Windows 内核对象命名空间, 查找 ACPI 驱动设备对象.

通过 NtOpenDirectoryObject + NtQueryDirectoryObject 枚举 \\Device 与 \\DosDevices,
确认 UWACPIDriver 是否真的创建了设备对象和符号链接.

用法: python -I tools/nt_enum.py [关键字]
"""
import ctypes
import sys
from ctypes import wintypes

ntdll = ctypes.WinDLL("ntdll", use_last_error=True)

OBJ_CASE_INSENSITIVE = 0x00000040
DIRECTORY_QUERY = 0x0001


class UNICODE_STRING(ctypes.Structure):
    _fields_ = [
        ("Length", wintypes.USHORT),
        ("MaximumLength", wintypes.USHORT),
        ("Buffer", ctypes.c_void_p),
    ]


class OBJECT_ATTRIBUTES(ctypes.Structure):
    _fields_ = [
        ("Length", wintypes.ULONG),
        ("RootDirectory", wintypes.HANDLE),
        ("ObjectName", ctypes.POINTER(UNICODE_STRING)),
        ("Attributes", wintypes.ULONG),
        ("SecurityDescriptor", ctypes.c_void_p),
        ("SecurityQualityOfService", ctypes.c_void_p),
    ]


class DIRECTORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("ObjectName", UNICODE_STRING),
        ("ObjectTypeName", UNICODE_STRING),
    ]


ntdll.NtOpenDirectoryObject.restype = ctypes.c_long
ntdll.NtOpenDirectoryObject.argtypes = [
    ctypes.POINTER(wintypes.HANDLE), wintypes.ULONG,
    ctypes.POINTER(OBJECT_ATTRIBUTES),
]
ntdll.NtQueryDirectoryObject.restype = ctypes.c_long
ntdll.NtQueryDirectoryObject.argtypes = [
    wintypes.HANDLE, ctypes.c_void_p, wintypes.ULONG, wintypes.BOOLEAN,
    wintypes.BOOLEAN, ctypes.POINTER(wintypes.ULONG), ctypes.POINTER(wintypes.ULONG),
]
ntdll.RtlInitUnicodeString.restype = None
ntdll.RtlInitUnicodeString.argtypes = [ctypes.POINTER(UNICODE_STRING), wintypes.LPCWSTR]


def open_dir(path):
    """打开内核对象目录, 返回句柄或 None.

    注意: UNICODE_STRING.Buffer 必须指向我们自己持有的缓冲区.
    若直接传给 RtlInitUnicodeString 一个 Python str, ctypes 生成的临时
    宽字符缓冲区可能在调用后被回收, 导致 STATUS_OBJECT_PATH_SYNTAX_BAD.
    """
    buf = ctypes.create_unicode_buffer(path)
    us = UNICODE_STRING()
    us.Length = len(path) * 2
    us.MaximumLength = (len(path) + 1) * 2
    us.Buffer = ctypes.cast(buf, ctypes.c_void_p)
    oa = OBJECT_ATTRIBUTES()
    oa.Length = ctypes.sizeof(OBJECT_ATTRIBUTES)
    oa.RootDirectory = None
    oa.ObjectName = ctypes.pointer(us)
    oa.Attributes = OBJ_CASE_INSENSITIVE
    h = wintypes.HANDLE()
    st = ntdll.NtOpenDirectoryObject(ctypes.byref(h), DIRECTORY_QUERY, ctypes.byref(oa))
    if st != 0:
        print(f"  打开 {path} 失败: NTSTATUS=0x{st & 0xffffffff:08X}")
        return None
    return h


def enum_dir(h, limit=600):
    """枚举目录项, 返回名称列表."""
    buf = ctypes.create_string_buffer(0x10000)
    ctx = wintypes.ULONG(0)
    ret = wintypes.ULONG(0)
    names = []
    while True:
        st = ntdll.NtQueryDirectoryObject(
            h, buf, ctypes.sizeof(buf), False, False,
            ctypes.byref(ctx), ctypes.byref(ret),
        )
        if st != 0:
            break
        off = 0
        while off + ctypes.sizeof(DIRECTORY_BASIC_INFORMATION) <= ctypes.sizeof(buf):
            dbi = DIRECTORY_BASIC_INFORMATION.from_buffer(buf, off)
            if not dbi.ObjectName.Buffer:
                break
            raw = ctypes.wstring_at(dbi.ObjectName.Buffer, dbi.ObjectName.Length // 2)
            otype = ""
            if dbi.ObjectTypeName.Buffer:
                otype = ctypes.wstring_at(
                    dbi.ObjectTypeName.Buffer, dbi.ObjectTypeName.Length // 2
                )
            names.append((raw.rstrip("\x00"), otype))
            off += ctypes.sizeof(DIRECTORY_BASIC_INFORMATION)
            if len(names) >= limit:
                return names
    return names


def main():
    args = sys.argv[1:]
    # 用法: nt_enum.py [关键字] [目录1] [目录2] ...
    # 若第一个参数以 \ 开头, 则视为目录列表, 关键字默认 ACPI
    if args and args[0].startswith("\\"):
        keyword = "ACPI"
        dirs = args
    else:
        keyword = args[0] if args else "ACPI"
        dirs = args[1:] or [r"\Device", r"\DosDevices", r"\GLOBAL??"]

    print(f"=== 枚举内核对象命名空间 (关键字: {keyword}) ===\n")

    total_hits = 0
    for path in dirs:
        print(f"--- {path} ---")
        h = open_dir(path)
        if not h:
            print()
            continue
        items = enum_dir(h)
        hits = [(n, t) for n, t in items if keyword.lower() in n.lower()]
        print(f"  总对象数: {len(items)}")
        if hits:
            total_hits += len(hits)
            for n, t in hits:
                print(f"    >> {n}   [{t or 'Directory'}]")
        else:
            print(f"  (没有匹配 {keyword} 的对象)")
            if items:
                names = [n for n, _ in items]
                print(f"  前 20 项: {', '.join(names[:20])}")
        ctypes.windll.kernel32.CloseHandle(h)
        print()

    print(f"合计匹配: {total_hits}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
