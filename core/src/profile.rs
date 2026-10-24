//! 自定义配置（Profile）—— 把「功耗墙三项 + 强冷」存成一组，一键应用.
//!
//! 纯数据与校验，**不做文件 IO**：持久化由 app 层负责（路径在 `%LOCALAPPDATA%`），
//! 这样本模块可以在没有文件系统的环境里被单测覆盖。
//!
//! # 为什么要自定义配置
//!
//! 机器自带的档位只有「均衡 / 办公 / 狂暴」三档，且**档位由固件决定**，
//! 应用只在用户点击时写入。用户实际需要的往往是「办公但 PL1 提到 60」「打游戏但限到 45 更静」
//! 这类组合 —— 这就是 Profile 的用途。
//!
//! # 安全
//!
//! 校验一律在构造时完成（范围来自 [`crate::control`] 的常量），
//! 应用配置时仍然逐项走 `snapshot → write_verified → 失败回滚`，不会因为"存过盘"就跳过校验。

use crate::control::{PL1_RANGE, PL2_RANGE, PL4_RANGE};
use crate::error::{Error, Result};

/// 配置名最大长度（字符数）.
pub const MAX_NAME_LEN: usize = 32;

/// 一条自定义配置.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    /// 显示名（用户可读）.
    pub name: String,
    /// 长时功耗上限 (W).
    pub pl1: u8,
    /// 短时功耗上限 (W).
    pub pl2: u8,
    /// 峰值功耗上限 (W).
    pub pl4: u8,
    /// 是否同时开启强冷.
    pub fan_boost: bool,
}

impl Profile {
    /// 构造并校验.
    ///
    /// # Errors
    /// 名称空白/过长、或任一功耗值越界时返回错误.
    pub fn new(
        name: impl Into<String>,
        pl1: u8,
        pl2: u8,
        pl4: u8,
        fan_boost: bool,
    ) -> Result<Self> {
        let profile = Self {
            name: name.into(),
            pl1,
            pl2,
            pl4,
            fan_boost,
        };
        profile.validate()?;
        Ok(profile)
    }

    /// 校验（构造后仍可能被反序列化篡改，因此公开）.
    ///
    /// # Errors
    /// 同 [`Profile::new`].
    pub fn validate(&self) -> Result<()> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err(Error::Config("配置名不能为空".into()));
        }
        if trimmed.chars().count() > MAX_NAME_LEN {
            return Err(Error::Config(format!(
                "配置名过长（最多 {MAX_NAME_LEN} 个字符）"
            )));
        }
        check("PL1", self.pl1, PL1_RANGE)?;
        check("PL2", self.pl2, PL2_RANGE)?;
        check("PL4", self.pl4, PL4_RANGE)?;
        Ok(())
    }

    /// 一句话摘要（界面/日志用）.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "PL1={} PL2={} PL4={}{}",
            self.pl1,
            self.pl2,
            self.pl4,
            if self.fan_boost { " · 强冷开" } else { "" }
        )
    }
}

fn check(field: &'static str, value: u8, (min, max): (u8, u8)) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_profile() {
        let p = Profile::new("安静办公", 45, 45, 200, true).unwrap();
        assert_eq!(p.summary(), "PL1=45 PL2=45 PL4=200 · 强冷开");
    }

    #[test]
    fn rejects_blank_or_too_long_name() {
        assert!(Profile::new("   ", 45, 45, 200, false).is_err());
        let long = "长".repeat(MAX_NAME_LEN + 1);
        assert!(Profile::new(long, 45, 45, 200, false).is_err());
    }

    #[test]
    fn rejects_out_of_range_power() {
        assert!(Profile::new("x", 0, 45, 200, false).is_err(), "PL1 下限 5");
        assert!(
            Profile::new("x", 201, 45, 200, false).is_err(),
            "PL1 上限 200"
        );
        assert!(
            Profile::new("x", 45, 45, 255, false).is_ok(),
            "PL4 允许 255"
        );
    }

    #[test]
    fn trim_is_only_for_validation_not_stored() {
        // 保留用户原始输入（界面自己 trim），但校验用 trim 后的值
        let p = Profile::new("  办公  ", 45, 45, 200, false).unwrap();
        assert_eq!(p.name, "  办公  ");
        p.validate().unwrap();
    }
}
