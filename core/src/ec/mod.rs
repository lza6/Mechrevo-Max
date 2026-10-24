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

mod throttle;
mod transport;
mod windows;

pub use throttle::{AccessThrottle, DEFAULT_MIN_INTERVAL, HARD_MIN_INTERVAL};
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
        let end = start.checked_add(len).ok_or(Error::OutOfRange {
            field: "read_block end",
            value: i32::from(start) + i32::from(len),
            min: 0,
            max: i32::from(EC_MAX_ADDR) + 1,
        })?;
        if end > EC_MAX_ADDR.saturating_add(1) {
            return Err(Error::OutOfRange {
                field: "read_block end",
                value: i32::from(end),
                min: 0,
                max: i32::from(EC_MAX_ADDR) + 1,
            });
        }
        (start..end).map(|addr| self.read(addr)).collect()
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

    /// 为回滚点创建一个 RAII 守卫.
    ///
    /// 守卫离开作用域时 **无条件** 还原 —— 包括断言 panic 展开的路径.
    /// Rust 没有 `finally`, 少了这一步, "写入 → 断言失败" 就会把硬件
    /// 停在改动后的状态.
    ///
    /// 主动回滚请调用 [`RestoreGuard::restore_now`], 之后守卫自动解除;
    /// 若已用别的方式回滚完, 调用 [`RestoreGuard::disarm`] 避免重复写入.
    #[must_use]
    pub fn guard(&self, point: RollbackPoint) -> RestoreGuard<'_, T> {
        RestoreGuard {
            ec: self,
            point: Some(point),
        }
    }
}

/// 回滚守卫 —— 离开作用域时自动 `restore`.
///
/// 主要服务于"会写真实硬件"的路径 (尤其 `core/tests/hardware.rs`):
/// 断言失败时进程会 panic 展开, 普通语句被跳过, 只有 `Drop` 仍会执行.
///
/// `Drop` 里的失败无法向上传播, 因此只用 `eprintln!` 告知; 需要拿到错误的
/// 调用方应改用 [`RestoreGuard::restore_now`].
pub struct RestoreGuard<'a, T: EcTransport> {
    ec: &'a Ec<T>,
    point: Option<RollbackPoint>,
}

impl<T: EcTransport> RestoreGuard<'_, T> {
    /// 解除守卫 —— 回滚已经完成, 不需要再还原一次.
    pub fn disarm(&mut self) {
        self.point = None;
    }

    /// 守卫当前持有的回滚点 (解除后为 `None`).
    #[must_use]
    pub fn point(&self) -> Option<&RollbackPoint> {
        self.point.as_ref()
    }

    /// 立即还原并解除守卫.
    ///
    /// # Errors
    /// 写回失败时返回错误; 无论成败守卫都会解除, 不会在 `Drop` 时重试.
    pub fn restore_now(&mut self) -> Result<usize> {
        match self.point.take() {
            Some(point) => self.ec.restore(&point),
            None => Ok(0),
        }
    }
}

impl<T: EcTransport> Drop for RestoreGuard<'_, T> {
    fn drop(&mut self) {
        if let Some(point) = self.point.take() {
            match self.ec.restore(&point) {
                Ok(n) => log::info!("[ec-guard] 已自动回滚 {n} 个寄存器"),
                // 这是全项目最关键的一条告警：硬件可能停在中间状态。
                // release 下没有 stderr，必须经 log 落到文件（见 app 的 logging 模块）。
                Err(e) => {
                    eprintln!("[ec-guard] ⚠️ 自动回滚失败: {e}");
                    log::error!("[ec-guard] ⚠️ 自动回滚失败, 硬件可能停在中间状态: {e}");
                }
            }
        }
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
    fn read_block_rejects_out_of_range_without_wrapping() {
        let ec = Ec::new(MockTransport::new());
        let err = ec.read_block(EC_MAX_ADDR, 2).unwrap_err();
        assert!(matches!(err, Error::OutOfRange { .. }));
        let err = ec.read_block(u16::MAX, 2).unwrap_err();
        assert!(matches!(err, Error::OutOfRange { .. }));
    }

    #[test]
    fn read_block_accepts_exact_upper_boundary() {
        let t = MockTransport::new();
        t.seed(EC_MAX_ADDR - 1, 0xAA);
        t.seed(EC_MAX_ADDR, 0xBB);
        let ec = Ec::new(t);
        assert_eq!(ec.read_block(EC_MAX_ADDR - 1, 2).unwrap(), [0xAA, 0xBB]);
    }

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

    #[test]
    fn guard_restores_when_dropped() {
        let t = MockTransport::new();
        t.seed(0x0730, 75);
        let ec = Ec::new(t);

        let point = ec.snapshot(&[0x0730], "guard 测试").unwrap();
        {
            let _guard = ec.guard(point);
            ec.write_verified(0x0730, 45).unwrap();
            assert_eq!(ec.read(0x0730).unwrap(), 45);
        }
        assert_eq!(ec.read(0x0730).unwrap(), 75, "守卫离开作用域后必须自动还原");
    }

    #[test]
    fn guard_does_not_restore_after_disarm() {
        let t = MockTransport::new();
        t.seed(0x0730, 75);
        let ec = Ec::new(t);

        let point = ec.snapshot(&[0x0730], "disarm 测试").unwrap();
        {
            let mut guard = ec.guard(point);
            guard.disarm();
            ec.write_verified(0x0730, 60).unwrap();
        }
        assert_eq!(ec.read(0x0730).unwrap(), 60, "解除守卫后不应再还原");
    }

    #[test]
    fn guard_restore_now_returns_count_and_disarms() {
        let t = MockTransport::new();
        t.seed(0x0730, 75);
        t.seed(0x0731, 80);
        let ec = Ec::new(t);

        let point = ec.snapshot(&[0x0730, 0x0731], "restore_now").unwrap();
        let mut guard = ec.guard(point);
        ec.write_verified(0x0730, 10).unwrap();
        ec.write_verified(0x0731, 11).unwrap();

        let restored = guard.restore_now().unwrap();
        assert_eq!(restored, 2);
        assert!(guard.point().is_none(), "restore_now 后守卫应已解除");
        assert_eq!(ec.read(0x0730).unwrap(), 75);
        assert_eq!(ec.read(0x0731).unwrap(), 80);
    }

    #[test]
    fn guard_restores_after_panic() {
        // 这是 P0-01 的核心场景: 断言失败 → panic 展开 → 硬件必须回到原值
        let t = MockTransport::new();
        t.seed(0x0730, 75);
        let ec = Ec::new(t);

        let point = ec.snapshot(&[0x0730], "panic 测试").unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = ec.guard(point);
            ec.write_verified(0x0730, 200).unwrap();
            assert_eq!(ec.read(0x0730).unwrap(), 45, "故意失败以触发 panic");
        }));

        assert!(outcome.is_err(), "内部断言应已 panic");
        assert_eq!(
            ec.read(0x0730).unwrap(),
            75,
            "panic 路径下守卫也必须完成回滚"
        );
    }
}
