//! 功耗墙控制 (PL1 / PL2 / PL4).
//!
//! # 操作对象
//!
//! 本模块操作的是各模式的**默认功耗寄存器**（`0x0730`–`0x0736`），
//! 即"这个模式应该用多少瓦"。
//!
//! # 安全纪律
//!
//! - 所有写入前**必须**先 `snapshot`，失败即中止（宁可不改也不留半成品）
//! - 逐字段写入，每写一个立即读回校验
//! - 任一步失败自动**回滚已写入的字段**
//! - 写入完成后返回 [`ChangeRecord`]，调用方可以据此再回滚

use crate::ec::{Ec, EcTransport, RollbackPoint};
use crate::error::{Error, Result};
use crate::namespace::Mode;

/// PL1 允许范围 (W).
pub const PL1_RANGE: (u8, u8) = (5, 200);
/// PL2 允许范围 (W).
pub const PL2_RANGE: (u8, u8) = (5, 200);
/// PL4 允许范围 (W).
pub const PL4_RANGE: (u8, u8) = (0, 255);

/// 一组功耗墙数值.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PowerLimits {
    /// 长时功耗上限 (W).
    pub pl1: u8,
    /// 短时功耗上限 (W).
    pub pl2: u8,
    /// 峰值功耗上限 (W).
    pub pl4: u8,
}

impl PowerLimits {
    /// 构造并校验.
    ///
    /// # Errors
    /// 任一字段越界时返回 [`Error::OutOfRange`].
    pub fn new(pl1: u8, pl2: u8, pl4: u8) -> Result<Self> {
        check_range("PL1", pl1, PL1_RANGE)?;
        check_range("PL2", pl2, PL2_RANGE)?;
        check_range("PL4", pl4, PL4_RANGE)?;
        Ok(Self { pl1, pl2, pl4 })
    }
}

fn check_range(field: &'static str, value: u8, (min, max): (u8, u8)) -> Result<()> {
    if value < min || value > max {
        return Err(Error::OutOfRange {
            field,
            value: i32::from(value),
            min: i32::from(min),
            max: i32::from(max),
        });
    }
    Ok(())
}

/// 一次改动的完整记录，可用于事后回滚.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChangeRecord {
    /// 改动描述.
    pub description: String,
    /// 改动前的回滚点.
    pub rollback: RollbackPoint,
    /// 改动后的实际值（地址 → 值）.
    pub applied: Vec<(u16, u8)>,
}

impl ChangeRecord {
    /// 用记录的回滚点撤销这次改动.
    ///
    /// # Errors
    /// 写回失败时返回错误.
    pub fn undo<T: EcTransport>(&self, ec: &Ec<T>) -> Result<usize> {
        ec.restore(&self.rollback)
    }
}

/// 功耗墙控制器.
pub struct PowerController<'a, T: EcTransport> {
    ec: &'a Ec<T>,
}

impl<'a, T: EcTransport> PowerController<'a, T> {
    /// 绑定一个 EC 访问器.
    pub fn new(ec: &'a Ec<T>) -> Self {
        Self { ec }
    }

    /// 读取某模式当前的默认功耗设置.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn read_defaults(&self, mode: Mode) -> Result<PowerLimits> {
        let (r1, r2, r4) = mode.default_power_regs();
        Ok(PowerLimits {
            pl1: self.ec.read(r1.addr)?,
            pl2: self.ec.read(r2.addr)?,
            pl4: self.ec.read(r4.addr)?,
        })
    }

    /// 写入某模式的默认功耗设置.
    ///
    /// 流程：校验 → 存回滚点 → 逐项写入并读回校验 → 任一步失败自动回滚.
    ///
    /// # Errors
    /// 校验失败、写入失败或读回不匹配时返回错误；此时已写入的字段会被自动还原.
    pub fn apply_defaults(&self, mode: Mode, limits: &PowerLimits) -> Result<ChangeRecord> {
        check_range("PL1", limits.pl1, PL1_RANGE)?;
        check_range("PL2", limits.pl2, PL2_RANGE)?;
        check_range("PL4", limits.pl4, PL4_RANGE)?;

        let (r1, r2, r4) = mode.default_power_regs();
        let plan = [(r1, limits.pl1), (r2, limits.pl2), (r4, limits.pl4)];
        let addrs: Vec<u16> = plan.iter().map(|(r, _)| r.addr).collect();

        // 1) 先存回滚点 —— 存不下就完全不改
        let description = format!(
            "设置 {} 默认功耗 PL1={} PL2={} PL4={}",
            mode.display_name(),
            limits.pl1,
            limits.pl2,
            limits.pl4
        );
        let rollback = self.ec.snapshot(&addrs, description.clone())?;

        // 2) 逐项写入
        let mut applied = Vec::with_capacity(plan.len());
        for (reg, value) in plan {
            if let Err(e) = self.ec.write_verified(reg.addr, value) {
                // 3) 失败即回滚已改动的部分
                let _ = self.ec.restore(&rollback);
                return Err(e);
            }
            applied.push((reg.addr, value));
        }

        Ok(ChangeRecord {
            description,
            rollback,
            applied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ec::MockTransport;
    use crate::namespace::Mode;

    fn seeded() -> Ec<MockTransport> {
        let t = MockTransport::new();
        // 官方出厂值: Gaming 75/75/200, Office 45/45/200
        t.seed(0x0730, 75);
        t.seed(0x0731, 75);
        t.seed(0x0732, 200);
        t.seed(0x0734, 45);
        t.seed(0x0735, 45);
        t.seed(0x0736, 200);
        Ec::new(t)
    }

    #[test]
    fn reads_official_gaming_defaults() {
        let ec = seeded();
        let ctl = PowerController::new(&ec);
        let limits = ctl.read_defaults(Mode::Gaming).unwrap();
        assert_eq!(
            limits,
            PowerLimits {
                pl1: 75,
                pl2: 75,
                pl4: 200
            }
        );
    }

    #[test]
    fn reads_official_office_defaults() {
        let ec = seeded();
        let ctl = PowerController::new(&ec);
        let limits = ctl.read_defaults(Mode::Office).unwrap();
        assert_eq!(
            limits,
            PowerLimits {
                pl1: 45,
                pl2: 45,
                pl4: 200
            }
        );
    }

    #[test]
    fn rejects_out_of_range_before_touching_hardware() {
        let ec = seeded();
        let ctl = PowerController::new(&ec);
        // PL1 下限 5
        let err = ctl
            .apply_defaults(
                Mode::Gaming,
                &PowerLimits {
                    pl1: 1,
                    pl2: 75,
                    pl4: 200,
                },
            )
            .unwrap_err();
        assert!(matches!(err, Error::OutOfRange { field: "PL1", .. }));
        // 硬件未被改动
        assert_eq!(ec.read(0x0730).unwrap(), 75);
    }

    #[test]
    fn applies_and_records_rollback() {
        let ec = seeded();
        let ctl = PowerController::new(&ec);
        let rec = ctl
            .apply_defaults(
                Mode::Gaming,
                &PowerLimits {
                    pl1: 60,
                    pl2: 80,
                    pl4: 190,
                },
            )
            .unwrap();

        assert_eq!(ec.read(0x0730).unwrap(), 60);
        assert_eq!(ec.read(0x0731).unwrap(), 80);
        assert_eq!(ec.read(0x0732).unwrap(), 190);
        assert_eq!(rec.rollback.get(0x0730), Some(75));

        // 撤销后应恢复出厂值
        rec.undo(&ec).unwrap();
        assert_eq!(ec.read(0x0730).unwrap(), 75);
        assert_eq!(ec.read(0x0731).unwrap(), 75);
        assert_eq!(ec.read(0x0732).unwrap(), 200);
    }

    #[test]
    fn auto_rolls_back_when_a_register_is_stuck() {
        let t = MockTransport::new().with_stuck(0x0731); // PL2 写不进去
        t.seed(0x0730, 75);
        t.seed(0x0731, 75);
        t.seed(0x0732, 200);
        let ec = Ec::new(t);
        let ctl = PowerController::new(&ec);

        let err = ctl
            .apply_defaults(
                Mode::Gaming,
                &PowerLimits {
                    pl1: 60,
                    pl2: 80,
                    pl4: 190,
                },
            )
            .unwrap_err();
        assert!(matches!(err, Error::VerifyMismatch { addr: 0x0731, .. }));

        // PL1 已被写入, 但因失败必须被自动还原
        assert_eq!(ec.read(0x0730).unwrap(), 75, "失败后 PL1 应被回滚");
        assert_eq!(ec.read(0x0731).unwrap(), 75);
    }

    #[test]
    fn power_limits_constructor_validates() {
        assert!(PowerLimits::new(75, 75, 200).is_ok());
        assert!(PowerLimits::new(0, 75, 200).is_err(), "PL1 下限 5");
        assert!(PowerLimits::new(201, 75, 200).is_err(), "PL1 上限 200");
    }
}
