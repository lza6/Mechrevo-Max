//! 运行档位（性能模式）控制.
//!
//! # 协议来源与验证
//!
//! 档位由两个寄存器组合表示，改完写一次提交寄存器触发：
//!
//! | 档位 | `0x751` | `0x7AB` | 默认 PL 来源 |
//! |---|---|---|---|
//! | 均衡 | `0x00` | `0x00` | `0x730`–`0x732` |
//! | 办公 | `0xA0` | `0x00` | `0x734`–`0x736` |
//! | 狂暴 | `0x10` | `0x10` | 游戏默认值 |
//!
//! **本机实测验证（2026-10-16）**：三个档位均可写入且读回一致，
//! 并观察到真实行为变化（狂暴档 GPU 风扇由 75 RPM 升至 130 RPM），
//! 实验结束后寄存器完全恢复原值。
//!
//! # 注意
//!
//! 提交寄存器 `0x767` 写入后**会自动清零**，因此不能对它做读回校验。

use crate::ec::{Ec, EcTransport};
use crate::error::Result;
use crate::namespace as ns;

/// 档位控制涉及的寄存器集合（用于快照/回滚）.
///
/// **刻意不含提交寄存器 `0x767`**：它写入后由固件自动清零，没有"原始值"
/// 可言，把它纳入快照会让撤销流程去写一个"命令寄存器"，语义未定义。
///
/// 注意：撤销（[`ModeChange::undo`]）只回写这两个档位字段，**不会**再写一次
/// 提交寄存器。因此"撤销后固件是否立即重新应用"尚未在真机确认
/// （见 `docs/protocol/ec-baseline.md` §6 的未验证项）；如果需要保证立即生效，
/// 应在真机上单独验证后再决定是否补一次提交。
const MODE_REGS: [u16; 2] = [ns::MODE_FIELD_1.addr, ns::MODE_FIELD_2.addr];

/// 运行档位.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OperatingMode {
    /// 均衡 —— 日常使用，按游戏默认功耗曲线.
    Balance,
    /// 办公 —— 更安静、功耗更保守.
    Office,
    /// 狂暴 —— 最高性能，风扇更激进.
    Turbo,
}

impl OperatingMode {
    /// 该档位对应的 (档位字段1, 档位字段2) 值.
    #[must_use]
    pub const fn register_values(self) -> (u8, u8) {
        match self {
            Self::Balance => (0x00, 0x00),
            Self::Office => (0xA0, 0x00),
            Self::Turbo => (0x10, 0x10),
        }
    }

    /// 中文显示名.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Balance => "均衡",
            Self::Office => "办公",
            Self::Turbo => "狂暴",
        }
    }

    /// 稳定的机器可读标识（供前后端通信使用，不随文案变化）.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Balance => "balance",
            Self::Office => "office",
            Self::Turbo => "turbo",
        }
    }

    /// 由标识解析档位.
    #[must_use]
    pub const fn from_key(key: &str) -> Option<Self> {
        match key.as_bytes() {
            b"balance" => Some(Self::Balance),
            b"office" => Some(Self::Office),
            b"turbo" => Some(Self::Turbo),
            _ => None,
        }
    }

    /// 由档位字段1 的值反推档位.
    ///
    /// `0x0751` 的 bit6 是**强冷位**，与档位编码共用同一字节：真机实测读到过
    /// `0x40`（强冷开 + 均衡档）。因此这里先 `& MODE_MASK` 屏蔽 bit6 再匹配，
    /// 否则「强冷开着」就会被误判成"未知档位"。
    #[must_use]
    pub const fn from_field1(value: u8) -> Option<Self> {
        match value & ns::MODE_MASK {
            0x00 => Some(Self::Balance),
            0xA0 => Some(Self::Office),
            0x10 => Some(Self::Turbo),
            _ => None,
        }
    }

    /// 全部档位.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Balance, Self::Office, Self::Turbo]
    }
}

/// 提交寄存器写入值（写入后固件自动清零）.
const COMMIT_VALUE: u8 = 0x01;

/// 档位控制器.
pub struct ModeController<'a, T: EcTransport> {
    ec: &'a Ec<T>,
}

impl<'a, T: EcTransport> ModeController<'a, T> {
    /// 绑定一个 EC 访问器.
    pub fn new(ec: &'a Ec<T>) -> Self {
        Self { ec }
    }

    /// 读取当前档位.
    ///
    /// 返回 `None` 表示寄存器里是未知编码 —— 不猜测，交给调用方判断.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn current(&self) -> Result<Option<OperatingMode>> {
        let field1 = self.ec.read(ns::MODE_FIELD_1.addr)?;
        Ok(OperatingMode::from_field1(field1))
    }

    /// 读取档位字段原始值（诊断用）.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn raw_fields(&self) -> Result<(u8, u8)> {
        Ok((
            self.ec.read(ns::MODE_FIELD_1.addr)?,
            self.ec.read(ns::MODE_FIELD_2.addr)?,
        ))
    }

    /// 切换到指定档位.
    ///
    /// 流程：存回滚点 → 写两个档位字段并校验 → 写提交寄存器 → 返回改动记录.
    /// 任一步失败会自动回滚.
    ///
    /// # Errors
    /// 任一写入或校验失败时返回错误；已写入的字段会被自动还原.
    pub fn switch(&self, mode: OperatingMode) -> Result<ModeChange> {
        let (v1, v2) = mode.register_values();
        let rollback = self
            .ec
            .snapshot(&MODE_REGS, format!("切换到{}档", mode.display_name()))?;

        let steps = [(ns::MODE_FIELD_1.addr, v1), (ns::MODE_FIELD_2.addr, v2)];

        for (addr, value) in steps {
            if let Err(e) = self.ec.write_verified(addr, value) {
                let _ = self.ec.restore(&rollback);
                return Err(e);
            }
        }

        // 提交寄存器写完即自清零, 所以只写不校验
        if let Err(e) = self.ec.write_unverified(ns::MODE_COMMIT.addr, COMMIT_VALUE) {
            let _ = self.ec.restore(&rollback);
            return Err(e);
        }

        Ok(ModeChange { mode, rollback })
    }
}

/// 一次档位切换的记录.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModeChange {
    /// 切换到的档位.
    pub mode: OperatingMode,
    /// 切换前的寄存器快照.
    pub rollback: crate::ec::RollbackPoint,
}

impl ModeChange {
    /// 撤销这次切换.
    ///
    /// # Errors
    /// 写回失败时返回错误.
    pub fn undo<T: EcTransport>(&self, ec: &Ec<T>) -> Result<usize> {
        ec.restore(&self.rollback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ec::MockTransport;

    fn ec_in(mode: OperatingMode) -> Ec<MockTransport> {
        let t = MockTransport::new();
        let (v1, v2) = mode.register_values();
        t.seed(ns::MODE_FIELD_1.addr, v1);
        t.seed(ns::MODE_FIELD_2.addr, v2);
        Ec::new(t)
    }

    #[test]
    fn identifies_current_mode() {
        for mode in OperatingMode::all() {
            let ec = ec_in(mode);
            assert_eq!(ModeController::new(&ec).current().unwrap(), Some(mode));
        }
    }

    #[test]
    fn fan_boost_bit_does_not_break_mode_detection() {
        // 真机实测读到过 0x40（强冷位 bit6 + 均衡档），必须识别为均衡而不是"未知"
        assert_eq!(
            OperatingMode::from_field1(0x40),
            Some(OperatingMode::Balance)
        );
        assert_eq!(OperatingMode::from_field1(0x50), Some(OperatingMode::Turbo)); // 强冷 + 狂暴
        assert_eq!(
            OperatingMode::from_field1(0xE0),
            Some(OperatingMode::Office)
        ); // 强冷 + 办公
        assert_eq!(
            OperatingMode::from_field1(0x77),
            None,
            "真正未知的编码仍应返回 None"
        );
    }

    #[test]
    fn unknown_encoding_returns_none_not_a_guess() {
        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0x77);
        let ec = Ec::new(t);
        assert_eq!(ModeController::new(&ec).current().unwrap(), None);
        assert_eq!(OperatingMode::from_field1(0x77), None);
    }

    #[test]
    fn switches_between_all_modes() {
        let ec = ec_in(OperatingMode::Balance);
        let ctl = ModeController::new(&ec);

        for mode in OperatingMode::all() {
            let change = ctl.switch(mode).unwrap();
            assert_eq!(ctl.current().unwrap(), Some(mode));
            change.undo(&ec).unwrap();
            assert_eq!(ctl.current().unwrap(), Some(OperatingMode::Balance));
        }
    }

    #[test]
    fn commit_register_is_written_but_not_verified() {
        // 提交寄存器自清零 —— 若做读回校验会误报失败
        let t = MockTransport::new().with_stuck(ns::MODE_COMMIT.addr);
        let ec = Ec::new(t);
        let ctl = ModeController::new(&ec);
        assert!(
            ctl.switch(OperatingMode::Turbo).is_ok(),
            "提交寄存器自清零不应导致切换失败"
        );
    }

    #[test]
    fn rolls_back_when_a_mode_field_is_stuck() {
        let t = MockTransport::new().with_stuck(ns::MODE_FIELD_2.addr);
        t.seed(ns::MODE_FIELD_1.addr, 0x00);
        t.seed(ns::MODE_FIELD_2.addr, 0x00);
        let ec = Ec::new(t);
        let ctl = ModeController::new(&ec);

        let err = ctl.switch(OperatingMode::Turbo).unwrap_err();
        assert!(matches!(err, crate::Error::VerifyMismatch { .. }));
        assert_eq!(
            ctl.current().unwrap(),
            Some(OperatingMode::Balance),
            "失败后档位字段必须回到原值"
        );
    }

    /// 模拟真机上观察到的「偶发丢一次写」：前 n 次写被忽略，之后正常。
    struct DropFirstWrites {
        inner: MockTransport,
        drops_left: std::sync::atomic::AtomicUsize,
    }

    impl DropFirstWrites {
        fn new(drops: usize) -> Self {
            Self {
                inner: MockTransport::new(),
                drops_left: std::sync::atomic::AtomicUsize::new(drops),
            }
        }
    }

    impl EcTransport for DropFirstWrites {
        fn read_ec(&self, addr: u16) -> crate::Result<u8> {
            self.inner.read_ec(addr)
        }
        fn write_ec(&self, addr: u16, value: u8) -> crate::Result<()> {
            use std::sync::atomic::Ordering;
            if self.drops_left.load(Ordering::SeqCst) > 0 {
                self.drops_left.fetch_sub(1, Ordering::SeqCst);
                return Ok(()); // 静默丢弃，模拟固件行为
            }
            self.inner.write_ec(addr, value)
        }
    }

    #[test]
    fn fan_boost_retries_when_firmware_drops_a_write() {
        let ec = Ec::new(DropFirstWrites::new(1));
        let ctl = ModeController::new(&ec);
        // 第一次写被丢弃 → 重试后应成功
        ctl.set_fan_boost(true).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0x40);
    }

    #[test]
    fn fan_boost_gives_up_after_bounded_retries() {
        // 连续丢弃 3 次 = 超过重试上限 → 必须报错，且回滚到原值
        let ec = Ec::new(DropFirstWrites::new(3));
        let ctl = ModeController::new(&ec);
        let err = ctl.set_fan_boost(true).unwrap_err();
        assert!(
            matches!(err, crate::Error::VerifyMismatch { addr, .. } if addr == ns::MODE_FIELD_1.addr)
        );
        assert_eq!(
            ec.read(ns::MODE_FIELD_1.addr).unwrap(),
            0x00,
            "失败后应保持原值"
        );
    }

    #[test]
    fn reads_fan_boost_bit() {
        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0x40); // 强冷开 + 均衡
        let ec = Ec::new(t);
        assert!(ModeController::new(&ec).fan_boost().unwrap());

        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0x00);
        let ec = Ec::new(t);
        assert!(!ModeController::new(&ec).fan_boost().unwrap());
    }

    #[test]
    fn set_fan_boost_only_touches_bit6() {
        // 均衡(0x00) → 强冷开(0x40) → 关回(0x00)
        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0x00);
        let ec = Ec::new(t);
        let ctl = ModeController::new(&ec);

        ctl.set_fan_boost(true).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0x40);
        ctl.set_fan_boost(false).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0x00);
    }

    #[test]
    fn set_fan_boost_preserves_mode_bits() {
        // 办公(0xA0) + 强冷 = 0xE0；关掉后必须回到 0xA0，不能变成 0x00
        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0xA0);
        let ec = Ec::new(t);
        let ctl = ModeController::new(&ec);

        ctl.set_fan_boost(true).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0xE0);
        assert_eq!(ctl.current().unwrap(), Some(OperatingMode::Office));

        ctl.set_fan_boost(false).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0xA0);
    }

    #[test]
    fn fan_boost_rolls_back_when_register_is_stuck() {
        let t = MockTransport::new().with_stuck(ns::MODE_FIELD_1.addr);
        t.seed(ns::MODE_FIELD_1.addr, 0x00);
        let ec = Ec::new(t);
        let err = ModeController::new(&ec).set_fan_boost(true).unwrap_err();
        assert!(matches!(err, crate::Error::VerifyMismatch { .. }));
        assert_eq!(
            ec.read(ns::MODE_FIELD_1.addr).unwrap(),
            0x00,
            "失败后必须回到原值"
        );
    }

    #[test]
    fn fan_boost_change_can_be_undone() {
        let t = MockTransport::new();
        t.seed(ns::MODE_FIELD_1.addr, 0x00);
        let ec = Ec::new(t);
        let ctl = ModeController::new(&ec);
        let change = ctl.set_fan_boost(true).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0x40);
        change.undo(&ec).unwrap();
        assert_eq!(ec.read(ns::MODE_FIELD_1.addr).unwrap(), 0x00);
    }

    #[test]
    fn rollback_point_excludes_commit_register() {
        // 提交寄存器写入后自清零, 没有"原始值"可还原; 纳入快照会让撤销去写命令寄存器.
        let ec = ec_in(OperatingMode::Balance);
        let ctl = ModeController::new(&ec);
        let change = ctl.switch(OperatingMode::Turbo).unwrap();

        assert_eq!(change.rollback.len(), 2, "回滚点只应包含两个档位字段");
        assert!(
            change.rollback.get(ns::MODE_COMMIT.addr).is_none(),
            "提交寄存器不应进入回滚点"
        );
        assert!(change.rollback.get(ns::MODE_FIELD_1.addr).is_some());
        assert!(change.rollback.get(ns::MODE_FIELD_2.addr).is_some());
    }

    #[test]
    fn mode_values_match_protocol() {
        assert_eq!(OperatingMode::Balance.register_values(), (0x00, 0x00));
        assert_eq!(OperatingMode::Office.register_values(), (0xA0, 0x00));
        assert_eq!(OperatingMode::Turbo.register_values(), (0x10, 0x10));
    }

    #[test]
    fn key_roundtrips_for_all_modes() {
        for mode in OperatingMode::all() {
            assert_eq!(OperatingMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(OperatingMode::from_key("unknown"), None);
        assert_eq!(OperatingMode::from_key(""), None);
    }
}

/// 强冷（Fan Boost）在 `0x0751` 上的位号.
pub const FAN_BOOST_BIT: u8 = 6;

/// 写入该寄存器允许的重试次数.
///
/// 真机实测（2026-10-24）：强冷位**偶发被 EC 丢弃一次** —— 4 次往返尝试中出现 1 次，
/// 且是"前向写入成功、紧接着的回滚写入读回仍是旧值"，之后重试立刻成功。
/// 目标是固定值的幂等写（写 0x40 两次与一次等价），因此这里做**有界重试**：
/// 最多 [`WRITE_ATTEMPTS`] 次「写 + 读回」，任一成功即返回；全部失败才报错并回滚。
const WRITE_ATTEMPTS: usize = 3;

/// 重试之间的间隔.
const WRITE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

/// 写一个固定目标值并读回校验，允许少量重试（见 [`WRITE_ATTEMPTS`] 的实测依据）.
fn write_settled<T: EcTransport>(ec: &Ec<T>, addr: u16, value: u8) -> Result<()> {
    let mut last_actual = 0u8;
    for attempt in 0..WRITE_ATTEMPTS {
        ec.write_unverified(addr, value)?;
        last_actual = ec.read(addr)?;
        if last_actual == value {
            return Ok(());
        }
        log::warn!(
            "0x{addr:04X} 写入 0x{value:02X} 未生效（读回 0x{last_actual:02X}），第 {}/{} 次",
            attempt + 1,
            WRITE_ATTEMPTS
        );
        std::thread::sleep(WRITE_RETRY_DELAY);
    }
    Err(crate::Error::VerifyMismatch {
        addr,
        expected: value,
        actual: last_actual,
    })
}

/// 一次强冷开关改动的记录.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FanBoostChange {
    /// 改成了开还是关.
    pub enabled: bool,
    /// 改动前的寄存器快照.
    pub rollback: crate::ec::RollbackPoint,
}

impl FanBoostChange {
    /// 撤销这次强冷开关.
    ///
    /// # Errors
    /// 写回失败时返回错误.
    pub fn undo<T: EcTransport>(&self, ec: &Ec<T>) -> Result<usize> {
        ec.restore(&self.rollback)
    }
}

impl<T: EcTransport> ModeController<'_, T> {
    /// 读取当前是否开启强冷（`0x0751` 的 bit6）.
    ///
    /// # Errors
    /// 读失败时返回错误.
    pub fn fan_boost(&self) -> Result<bool> {
        let value = self.ec.read(ns::MODE_FIELD_1.addr)?;
        Ok(value & (1 << FAN_BOOST_BIT) != 0)
    }

    /// 开关强冷.
    ///
    /// 与档位共用 `0x0751`，因此必须**读改写**：只动 bit6，其它位原样保留
    /// （否则会把档位字段一起清掉）。写入后读回校验，失败自动回滚。
    ///
    /// # Errors
    /// 写入或校验失败时返回错误；已改动会被自动还原.
    pub fn set_fan_boost(&self, enabled: bool) -> Result<FanBoostChange> {
        let addr = ns::MODE_FIELD_1.addr;
        let mask = 1u8 << FAN_BOOST_BIT;
        let rollback = self.ec.snapshot(
            &[addr],
            if enabled {
                "开启强冷"
            } else {
                "关闭强冷"
            },
        )?;

        let current = self.ec.read(addr)?;
        let next = if enabled {
            current | mask
        } else {
            current & !mask
        };

        if let Err(e) = write_settled(self.ec, addr, next) {
            let _ = self.ec.restore(&rollback);
            return Err(e);
        }

        Ok(FanBoostChange { enabled, rollback })
    }
}
