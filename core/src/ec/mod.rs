//! EC (Embedded Controller) 访问层.
//!
//! 通过 Windows 内核驱动 `\\.\ACPIDriver` 以 IOCTL 方式读写 EC 寄存器.
//!
//! # 设计要点
//!
//! - [`EcTransport`] 是可替换的传输抽象 —— 真机走 [`WindowsAcpiTransport`],
//!   测试走 [`MockTransport`], 这样协议逻辑可以离线单测.
//! - 所有写入默认执行「读回校验」, 校验失败即报错, 绝不静默通过.
//! - 提供 [`Ec::snapshot`] / [`Ec::restore`] 实现"写入前先存回滚点"的纪律.

mod transport;
mod windows;

pub use transport::{EcTransport, MockTransport};
pub use windows::WindowsAcpiTransport;

use crate::error::{Error, Result};

/// ACPI 驱动设备路径.
pub const DEVICE_PATH: &str = r"\\.\ACPIDriver";

/// 读 EC 的 IOCTL 码 (`IOCTL_GPD_ACPI_ECREAD`).
pub const IOCTL_EC_READ: u32 = 0x9C40_A488;

/// 写 EC 的 IOCTL 码 (`IOCTL_GPD_ACPI_ECWRITE`).
pub const IOCTL_EC_WRITE: u32 = 0x9C40_A48C;

/// EC 地址空间上界 (本机实测 0x0000-0x0FFF 全部可读).
pub const EC_MAX_ADDR: u16 = 0x0FFF;

/// 写入前的回滚点 —— 记录一批地址的原始值.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RollbackPoint {
    /// 采集时间戳 (Unix 秒).
    pub timestamp: i64,
    /// 描述这次改动做了什么.
    pub description: String,
    /// 地址 → 原始值.
    pub values: Vec<(u16, u8)>,
}

impl RollbackPoint {
    /// 查找某地址记录的原始值.
    #[must_use]
    pub fn get(&self, addr: u16) -> Option<u8> {
        self.values
            .iter()
            .find(|(a, _)| *a == addr)
            .map(|(_, v)| *v)
    }

    /// 记录了多少个地址.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// EC 访问器.
///
/// 所有读写都经过传输层抽象, 因此可以在没有真机的情况下用 [`MockTransport`] 单测.
pub struct Ec<T: EcTransport> {
    transport: T,
}

impl<T: EcTransport> Ec<T> {
    /// 用给定传输层构造.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// 读 1 字节.
    ///
    /// # Errors
    /// 传输层失败时返回 [`Error::Ioctl`].
    pub fn read(&self, addr: u16) -> Result<u8> {
        self.transport.read_ec(addr)
    }

    /// 读连续多个字节.
    ///
    /// # Errors
    /// 任一地址读取失败即返回错误.
    pub fn read_block(&self, start: u16, len: u16) -> Result<Vec<u8>> {
        (0..len)
            .map(|i| {
                let addr = start.wrapping_add(i);
                self.read(addr)
            })
            .collect()
    }

    /// 写 1 字节, 并读回校验.
    ///
    /// 校验失败返回 [`Error::VerifyMismatch`].
    ///
    /// # Errors
    /// 传输层失败或读回值不符时返回错误.
    pub fn write_verified(&self, addr: u16, value: u8) -> Result<()> {
        self.transport.write_ec(addr, value)?;
        let actual = self.transport.read_ec(addr)?;
        if actual != value {
            return Err(Error::VerifyMismatch {
                addr,
                expected: value,
                actual,
            });
        }
        Ok(())
    }

    /// 写 1 字节, 不校验 (仅用于确认寄存器本身不可读的场景).
    ///
    /// # Errors
    /// 传输层失败时返回错误.
    pub fn write_unverified(&self, addr: u16, value: u8) -> Result<()> {
        self.transport.write_ec(addr, value)
    }

    /// 读改写单个 bit (实现"只动一位"的最小改动).
    ///
    /// # Errors
    /// 读写失败时返回错误.
    pub fn write_bit(&self, addr: u16, bit: u8, set: bool) -> Result<u8> {
        if bit > 7 {
            return Err(Error::OutOfRange {
                field: "bit",
                value: i32::from(bit),
                min: 0,
                max: 7,
            });
        }
        let current = self.read(addr)?;
        let mask = 1u8 << bit;
        let next = if set { current | mask } else { current & !mask };
        self.write_verified(addr, next)?;
        Ok(next)
    }

    /// 采集回滚点 —— 写入任何寄存器之前必须先调用.
    ///
    /// # Errors
    /// 任一地址读取失败即返回错误 (宁可失败也不要存一份不完整的回滚点).
    pub fn snapshot(&self, addrs: &[u16], description: impl Into<String>) -> Result<RollbackPoint> {
        let mut values = Vec::with_capacity(addrs.len());
        for &addr in addrs {
            values.push((addr, self.read(addr)?));
        }
        Ok(RollbackPoint {
            timestamp: now_unix(),
            description: description.into(),
            values,
        })
    }

    /// 从回滚点恢复.
    ///
    /// 逐个写回并校验; 返回成功恢复的地址数.
    ///
    /// # Errors
    /// 写回失败时返回错误 (此时部分地址可能已恢复).
    pub fn restore(&self, point: &RollbackPoint) -> Result<usize> {
        let mut ok = 0;
        for &(addr, value) in &point.values {
            self.write_verified(addr, value)?;
            ok += 1;
        }
        Ok(ok)
    }
}

/// 当前 Unix 时间戳 (秒).
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let t = MockTransport::new();
        let ec = Ec::new(t);
        ec.write_verified(0x0740, 0x10).unwrap();
        assert_eq!(ec.read(0x0740).unwrap(), 0x10);
    }

    #[test]
    fn detects_write_verify_mismatch() {
        // 模拟"写了但没生效"的寄存器 —— 官方驱动正有这种坑
        let t = MockTransport::new().with_stuck(0x0740);
        let ec = Ec::new(t);
        let err = ec.write_verified(0x0740, 0x10).unwrap_err();
        match err {
            Error::VerifyMismatch {
                addr,
                expected,
                actual,
            } => {
                assert_eq!(addr, 0x0740);
                assert_eq!(expected, 0x10);
                assert_eq!(actual, 0x00);
            }
            other => panic!("期望 VerifyMismatch, 实际 {other:?}"),
        }
    }

    #[test]
    fn write_bit_only_touches_one_bit() {
        let t = MockTransport::new();
        t.seed(0x0768, 0b1010_0001);
        let ec = Ec::new(t);

        ec.write_bit(0x0768, 1, true).unwrap();
        assert_eq!(ec.read(0x0768).unwrap(), 0b1010_0011);

        ec.write_bit(0x0768, 0, false).unwrap();
        assert_eq!(ec.read(0x0768).unwrap(), 0b1010_0010);
    }

    #[test]
    fn rejects_out_of_range_bit() {
        let ec = Ec::new(MockTransport::new());
        let err = ec.write_bit(0x0768, 9, true).unwrap_err();
        assert!(matches!(err, Error::OutOfRange { field: "bit", .. }));
    }

    #[test]
    fn snapshot_then_restore_roundtrips() {
        let t = MockTransport::new();
        t.seed(0x0730, 75);
        t.seed(0x0731, 75);
        let ec = Ec::new(t);

        let point = ec.snapshot(&[0x0730, 0x0731], "测试回滚").unwrap();
        assert_eq!(point.len(), 2);
        assert_eq!(point.get(0x0730), Some(75));

        ec.write_verified(0x0730, 45).unwrap();
        assert_eq!(ec.read(0x0730).unwrap(), 45);

        let restored = ec.restore(&point).unwrap();
        assert_eq!(restored, 2);
        assert_eq!(ec.read(0x0730).unwrap(), 75);
        assert_eq!(ec.read(0x0731).unwrap(), 75);
    }

    #[test]
    fn read_block_reads_consecutive() {
        let t = MockTransport::new();
        t.seed(0x0700, 1);
        t.seed(0x0701, 2);
        t.seed(0x0702, 3);
        let ec = Ec::new(t);
        assert_eq!(ec.read_block(0x0700, 3).unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn snapshot_fails_if_any_read_fails() {
        let t = MockTransport::new().with_failing_read(0x0731);
        let ec = Ec::new(t);
        assert!(ec.snapshot(&[0x0730, 0x0731], "应当失败").is_err());
    }
}
