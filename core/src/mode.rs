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
/// 撤销时只还原两个档位字段，再写一次提交触发即可。
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
    #[must_use]
    pub const fn from_field1(value: u8) -> Option<Self> {
        match value {
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
