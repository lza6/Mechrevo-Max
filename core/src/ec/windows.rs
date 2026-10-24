//! Windows 真机传输层 —— 通过 IOCTL 与 `\\.\ACPIDriver` 通信.
//!
//! # 关键约束 (来自对官方驱动与社区逆向的分析)
//!
//! - 设备路径是 `\\.\ACPIDriver`, 内核对象名 `\DosDevices\ACPIDriver`
//! - 读: IOCTL `0x9C40A488`, 输入 4 字节小端地址, 输出 DWORD 取低字节
//! - 写: IOCTL `0x9C40A48C`, 输入地址 DWORD + 值 DWORD (共 8 字节)
//! - **`DeviceIoControl` 成功 ≠ 硬件执行成功** —— 官方驱动分发末尾有路径直接把状态改成成功,
//!   因此本层只保证"调用没报错", 真正的生效判定由上层读回校验完成.
//! - 每次读写都新开/关闭句柄 (与官方 C# 实现一致). 官方传入 4 MiB 缓冲区,
//!   我们不需要复刻这个尺寸.

use super::{EcTransport, IOCTL_EC_READ, IOCTL_EC_WRITE};
use crate::error::{Error, Result};

#[cfg(windows)]
mod imp {
    use super::{EcTransport, Error, IOCTL_EC_READ, IOCTL_EC_WRITE, Result};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    /// 真机传输层.
    ///
    /// 限速与互斥由**进程级**的 [`crate::ec::throttle::global`] 闸门保证:
    /// 每次 IOCTL 的**整个过程**都在闸门内执行, 因此
    /// ① 两次访问起点间隔 ≥ 最小间隔, ② 两次访问不会重叠, ③ 换个 transport
    /// 实例也无法绕过 (状态是进程共享的). 见 `docs/SPEC.md` §5.1.
    #[derive(Debug)]
    pub struct WindowsAcpiTransport {
        path: String,
        wide_path: Vec<u16>,
    }

    impl WindowsAcpiTransport {
        /// 用默认设备路径构造.
        ///
        /// # Errors
        /// 路径转换失败时返回错误.
        pub fn new() -> Result<Self> {
            Self::with_path(super::super::DEVICE_PATH)
        }

        /// 用指定设备路径构造.
        ///
        /// # Errors
        /// 路径为空时返回错误.
        pub fn with_path(path: &str) -> Result<Self> {
            if path.is_empty() {
                return Err(Error::Config("设备路径不能为空".into()));
            }
            let mut wide_path: Vec<u16> = OsStr::new(path).encode_wide().collect();
            wide_path.push(0);
            Ok(Self {
                path: path.to_owned(),
                wide_path,
            })
        }

        /// 设备路径.
        #[must_use]
        pub fn path(&self) -> &str {
            &self.path
        }

        /// 当前生效的最小访问间隔（来自进程级闸门）.
        #[must_use]
        pub fn min_interval(&self) -> std::time::Duration {
            crate::ec::throttle::global().min_interval()
        }

        /// 探测设备是否可用 (打开一次句柄).
        ///
        /// # Errors
        /// 设备不可用时返回 [`Error::DeviceNotReady`].
        pub fn probe(&self) -> Result<()> {
            let handle = self.open()?;
            // SAFETY: handle 由 CreateFileW 返回且已校验非 INVALID_HANDLE_VALUE,
            // 在此处立即关闭, 不存在别名或悬垂引用.
            unsafe { CloseHandle(handle) };
            Ok(())
        }

        /// 打开设备句柄.
        fn open(&self) -> Result<HANDLE> {
            // SAFETY: wide_path 以 NUL 结尾且生命周期覆盖本次调用;
            // 其余参数为按 Win32 约定传入的字面量.
            let handle = unsafe {
                CreateFileW(
                    self.wide_path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };

            if handle == INVALID_HANDLE_VALUE {
                let err = std::io::Error::last_os_error();
                // ERROR_FILE_NOT_FOUND (2) / ERROR_PATH_NOT_FOUND (3) 都表示设备对象不存在
                if matches!(err.raw_os_error(), Some(2 | 3)) {
                    return Err(Error::DeviceNotReady {
                        path: self.path.clone(),
                    });
                }
                return Err(Error::DeviceOpen {
                    path: self.path.clone(),
                    source: err,
                });
            }
            Ok(handle)
        }

        /// 发起一次 IOCTL.
        ///
        /// 整个访问(打开句柄 + DeviceIoControl + 关闭句柄)都在限速闸门内执行,
        /// 因此单次访问耗时超过最小间隔时, 也不会出现两个访问重叠.
        fn ioctl(&self, code: u32, input: &[u8], out_len: usize) -> Result<Vec<u8>> {
            crate::ec::throttle::global().run(|| self.ioctl_unguarded(code, input, out_len))
        }

        /// 真正发起 IOCTL —— **只能**由 [`Self::ioctl`] 在闸门内调用.
        fn ioctl_unguarded(&self, code: u32, input: &[u8], out_len: usize) -> Result<Vec<u8>> {
            let handle = self.open()?;
            let mut output = vec![0u8; out_len];
            let mut returned: u32 = 0;

            // SAFETY: handle 有效; input 切片与其长度一致; output 缓冲区足够容纳 out_len 字节;
            // returned 是指向栈上 u32 的有效指针. 调用返回后立即关闭句柄.
            let ok = unsafe {
                DeviceIoControl(
                    handle,
                    code,
                    input.as_ptr().cast(),
                    u32::try_from(input.len()).unwrap_or(u32::MAX),
                    output.as_mut_ptr().cast(),
                    u32::try_from(out_len).unwrap_or(u32::MAX),
                    &raw mut returned,
                    std::ptr::null_mut(),
                )
            };

            let err = if ok == 0 {
                Some(std::io::Error::last_os_error())
            } else {
                None
            };
            // SAFETY: 与上面的 CreateFileW 配对, 句柄不再使用.
            unsafe { CloseHandle(handle) };

            if let Some(source) = err {
                return Err(Error::Ioctl { code, source });
            }
            output.truncate(returned as usize);
            Ok(output)
        }
    }

    impl EcTransport for WindowsAcpiTransport {
        fn read_ec(&self, addr: u16) -> Result<u8> {
            // 官方实现把 u16 地址扩展成 DWORD
            let input = u32::from(addr).to_le_bytes();
            let out = self.ioctl(IOCTL_EC_READ, &input, 4)?;
            if out.len() < 4 {
                return Err(Error::Ioctl {
                    code: IOCTL_EC_READ,
                    source: std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!("期望 4 字节响应, 实际 {}", out.len()),
                    ),
                });
            }
            let raw = u32::from_le_bytes([out[0], out[1], out[2], out[3]]);
            Ok((raw & 0xFF) as u8)
        }

        fn write_ec(&self, addr: u16, value: u8) -> Result<()> {
            let mut input = [0u8; 8];
            input[..4].copy_from_slice(&u32::from(addr).to_le_bytes());
            input[4..].copy_from_slice(&u32::from(value).to_le_bytes());
            self.ioctl(IOCTL_EC_WRITE, &input, 4)?;
            Ok(())
        }
    }
}

#[cfg(windows)]
pub use imp::WindowsAcpiTransport;

#[cfg(not(windows))]
/// 非 Windows 平台的占位实现 —— 明确报错而不是静默失败.
#[derive(Debug, Clone)]
pub struct WindowsAcpiTransport;

#[cfg(not(windows))]
impl WindowsAcpiTransport {
    /// 非 Windows 平台无法访问 EC.
    ///
    /// # Errors
    /// 永远返回错误.
    pub fn new() -> Result<Self> {
        Err(Error::NotSupported("EC 访问仅支持 Windows"))
    }
}

#[cfg(not(windows))]
impl EcTransport for WindowsAcpiTransport {
    fn read_ec(&self, _addr: u16) -> Result<u8> {
        Err(Error::NotSupported("EC 访问仅支持 Windows"))
    }

    fn write_ec(&self, _addr: u16, _value: u8) -> Result<()> {
        Err(Error::NotSupported("EC 访问仅支持 Windows"))
    }
}
