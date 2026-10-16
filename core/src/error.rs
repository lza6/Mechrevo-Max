//! 错误类型定义.
//!
//! 所有 EC 访问失败都必须给出明确的失败原因 —— 绝不把失败静默成 0.
//! 这是从官方驱动学到的教训: `DeviceIoControl` 返回成功不代表 ACPI 真正执行成功,
//! 官方驱动分发末尾甚至有路径直接把状态改成成功.
//! 因此本库坚持「读回校验」, 无法读回时一律报错.

use thiserror::Error;

/// 核心库统一错误类型.
#[derive(Debug, Error)]
pub enum Error {
    /// 无法打开 EC 设备对象.
    #[error("无法打开 EC 设备 {path}: {source}")]
    DeviceOpen {
        /// 尝试打开的路径.
        path: String,
        /// 底层 IO 错误.
        #[source]
        source: std::io::Error,
    },

    /// 设备未就绪 —— 通常是驱动没绑上 PnP 设备.
    #[error(
        "EC 设备未就绪 ({path}). 请依次执行:\n  \
         1) sc.exe start UWACPIDriver\n  \
         2) pnputil.exe /scan-devices\n\
         详细说明见 docs/protocol/ec-access.md"
    )]
    DeviceNotReady {
        /// 尝试打开的路径.
        path: String,
    },

    /// IOCTL 调用失败.
    #[error("IOCTL 0x{code:08X} 失败: {source}")]
    Ioctl {
        /// 失败的 IOCTL 码.
        code: u32,
        /// 底层 IO 错误.
        #[source]
        source: std::io::Error,
    },

    /// 读回校验失败 —— 写入后读到的值与期望不符.
    #[error("写入校验失败 @ 0x{addr:04X}: 期望 0x{expected:02X}, 实际读到 0x{actual:02X}")]
    VerifyMismatch {
        /// EC 地址.
        addr: u16,
        /// 期望值.
        expected: u8,
        /// 实际读回值.
        actual: u8,
    },

    /// 值超出允许范围.
    #[error("值越界 {field}: {value} 不在 [{min}, {max}] 内")]
    OutOfRange {
        /// 字段名.
        field: &'static str,
        /// 传入值.
        value: i32,
        /// 最小值.
        min: i32,
        /// 最大值.
        max: i32,
    },

    /// 当前硬件不支持该功能.
    #[error("当前机型不支持该功能: {0}")]
    NotSupported(&'static str),

    /// 注册表/配置解析错误.
    #[error("配置解析失败: {0}")]
    Config(String),
}

/// 便捷 Result 别名.
pub type Result<T> = std::result::Result<T, Error>;
