//! EC 寄存器地址表.
//!
//! # 重要：地址必须区分「本机已验证」与「来自社区未验证」
//!
//! EC 地址在不同机型/BIOS 上**并不通用**。本机实测发现：
//! - `0x0730`–`0x0736`（默认功耗）**完全适用**，读出的值与官方注册表一致
//! - `0x0783`–`0x0786`（社区称"当前生效 PL"）**本机读出全为 0，不适用**
//!
//! 因此本模块用 [`AddrStatus`] 显式标注每个地址的可信度，
//! 控制层在操作未验证地址时必须让调用方知情。

/// 地址可信度.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AddrStatus {
    /// 在本机实测验证过（通常是读出的值与官方配置交叉吻合）.
    Verified,
    /// 来自社区逆向成果，**未在本机验证**. 使用前应先读取确认.
    CommunityUnverified,
    /// 本机实测**不适用**（如读出恒为 0）.
    NotApplicable,
}

/// 一个 EC 寄存器的定义.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Reg {
    /// EC 地址.
    pub addr: u16,
    /// 人类可读名称.
    pub name: &'static str,
    /// 可信度.
    pub status: AddrStatus,
    /// 备注（来源、单位、位域说明等）.
    pub note: &'static str,
}

/// CPU 温度 (°C).
///
/// 判定方式（不是"看着像温度"）：与 ACPI 热区
/// (`\Thermal Zone Information(*)\Temperature` 的 `\_sb.ectz`) 做配对比对 ——
/// 9 组配对样本上相关系数 r = +0.857，绝对偏差约 +2 °C；
/// 且在「单线程升温 40 s → 冷却」实验中同步上升 +5.5 °C、随后回落。
pub const CPU_TEMP: Reg = Reg {
    addr: 0x043E,
    name: "CPU Temperature",
    status: AddrStatus::Verified,
    note: "单位 °C；vs ACPI 热区 r=0.857，偏差≈+2；升温/冷却实验同步变化",
};

/// GPU 温度 (°C).
///
/// 判定方式：与 `nvidia-smi --query-gpu=temperature.gpu` 做 80 组并发比对
/// （NVAPI 不经过 EC，不会抢通道），相关系数 r = +0.989，绝对偏差约 +1 °C。
pub const GPU_TEMP: Reg = Reg {
    addr: 0x044F,
    name: "GPU Temperature",
    status: AddrStatus::Verified,
    note: "单位 °C；vs nvidia-smi r=0.989，偏差≈+1；80 组并发样本",
};

/// 项目标识.
pub const PROJECT_ID: Reg = Reg {
    addr: 0x0740,
    name: "ProjectID",
    status: AddrStatus::Verified,
    note: "本机读出 16, 与官方注册表 ProjectID 一致",
};

/// 电源来源.
pub const POWER_SOURCE: Reg = Reg {
    addr: 0x0490,
    name: "ecPowSource",
    status: AddrStatus::Verified,
    note: "本机读出 7",
};

/// CPU 风扇转速高字节.
pub const FAN1_RPM_HIGH: Reg = Reg {
    addr: 0x0464,
    name: "CPU Fan RPM High",
    status: AddrStatus::Verified,
    note: "与大端低字节组合, 实测 3970 RPM",
};

/// CPU 风扇转速低字节.
pub const FAN1_RPM_LOW: Reg = Reg {
    addr: 0x0465,
    name: "CPU Fan RPM Low",
    status: AddrStatus::Verified,
    note: "组合方式: high * 256 + low",
};

/// GPU 风扇转速高字节.
pub const FAN2_RPM_HIGH: Reg = Reg {
    addr: 0x046C,
    name: "GPU Fan RPM High",
    status: AddrStatus::Verified,
    note: "注意与低字节的地址顺序相反",
};

/// GPU 风扇转速低字节.
pub const FAN2_RPM_LOW: Reg = Reg {
    addr: 0x046B,
    name: "GPU Fan RPM Low",
    status: AddrStatus::Verified,
    note: "地址小于高字节",
};

/// 运行档位字段 1.
///
/// bit6 是**强冷位**，与档位共用该字节：真机实测读到过 `0x40`（强冷开、档位=均衡），
/// 因此解析档位前必须先把 bit6 屏蔽掉（见 [`MODE_MASK`]）。
pub const MODE_FIELD_1: Reg = Reg {
    addr: 0x0751,
    name: "Mode Field 1",
    status: AddrStatus::Verified,
    note: "0x00=均衡 0xA0=办公 0x10=狂暴; bit6 为强冷位，解析前需 & MODE_MASK",
};

/// 解析档位时要屏蔽掉的位（bit6 = 强冷）.
pub const MODE_MASK: u8 = 0xBF;

/// 运行档位字段 2.
pub const MODE_FIELD_2: Reg = Reg {
    addr: 0x07AB,
    name: "Mode Field 2",
    status: AddrStatus::Verified,
    note: "均衡/办公 0x00, 狂暴 0x10。本机实测",
};

/// 档位提交寄存器 —— 写完固件自动清零.
pub const MODE_COMMIT: Reg = Reg {
    addr: 0x0767,
    name: "Mode Commit",
    status: AddrStatus::Verified,
    note: "写入后自动清零, 因此不可做读回校验",
};

/// 强冷开关所在寄存器的位号.
pub const FAN_BOOST_REG: Reg = Reg {
    addr: 0x0751,
    name: "Fan Boost Bit",
    status: AddrStatus::Verified,
    note: "与档位共用寄存器, bit6 = 强冷",
};

/// 本机支持的运行模式.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// 办公模式.
    Office,
    /// 游戏模式.
    Gaming,
}

impl Mode {
    /// 该模式的默认功耗三元组寄存器 (PL1, PL2, PL4).
    #[must_use]
    pub const fn default_power_regs(self) -> (Reg, Reg, Reg) {
        match self {
            Self::Gaming => (
                Reg {
                    addr: 0x0730,
                    name: "Gaming Default PL1",
                    status: AddrStatus::Verified,
                    note: "本机读出 75, 与官方注册表一致",
                },
                Reg {
                    addr: 0x0731,
                    name: "Gaming Default PL2",
                    status: AddrStatus::Verified,
                    note: "本机读出 75",
                },
                Reg {
                    addr: 0x0732,
                    name: "Gaming Default PL4",
                    status: AddrStatus::Verified,
                    note: "本机读出 200",
                },
            ),
            Self::Office => (
                Reg {
                    addr: 0x0734,
                    name: "Office Default PL1",
                    status: AddrStatus::Verified,
                    note: "本机读出 45",
                },
                Reg {
                    addr: 0x0735,
                    name: "Office Default PL2",
                    status: AddrStatus::Verified,
                    note: "本机读出 45",
                },
                Reg {
                    addr: 0x0736,
                    name: "Office Default PL4",
                    status: AddrStatus::Verified,
                    note: "本机读出 200",
                },
            ),
        }
    }

    /// 中文显示名.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Office => "办公模式",
            Self::Gaming => "游戏模式",
        }
    }
}

/// 固件侧基线 PL1（只读）.
///
/// **2026-10-24 更正**：早前本模块把它标为 `NotApplicable`（当时读出 0），
/// 但真机复测读出 75，且做了两组决定性实验：
/// ① 把默认功耗 `0x730` 从 75 改成 50 并写提交寄存器 `0x767` —— `0x783` 不变；
/// ② 依次切到办公/狂暴档再切回 —— `0x783` 仍不变。
/// 因此它**不是**"当前生效值"，而是**固件/BIOS 给的基线**，且不随应用侧写入变化。
pub const FW_PL1: Reg = Reg {
    addr: 0x0783,
    name: "Firmware Baseline PL1",
    status: AddrStatus::Verified,
    note: "实测恒 75；改默认 PL 与切档都不影响它 → 固件基线，非生效值",
};

/// 固件侧基线 PL2（只读，实测 130）.
pub const FW_PL2: Reg = Reg {
    addr: 0x0784,
    name: "Firmware Baseline PL2",
    status: AddrStatus::Verified,
    note: "实测 130；与默认寄存器 0x731=75 不同，说明固件有自己的上限",
};

/// 固件侧基线 PL4（只读，实测 200）.
pub const FW_PL4: Reg = Reg {
    addr: 0x0785,
    name: "Firmware Baseline PL4",
    status: AddrStatus::Verified,
    note: "实测 200",
};

/// `0x0786`（实测 133）—— 含义未确认，不猜测用途.
pub const FW_EXTRA: Reg = Reg {
    addr: 0x0786,
    name: "Firmware Baseline Extra",
    status: AddrStatus::CommunityUnverified,
    note: "实测 133；社区曾称 TCC Offset，但本机未验证，不用于任何写入",
};

/// 本机实测**不适用**的社区地址 —— 保留在表里作为反面证据, 避免误用.
///
/// 注：`0x0783`–`0x0786` 已从本列表移出（2026-10-24 复测有非零值，见 [`FW_PL1`] 等）。
pub const NOT_APPLICABLE: &[Reg] = &[Reg {
    addr: 0x0D4B,
    name: "GPU 核心频率 (社区)",
    status: AddrStatus::NotApplicable,
    note: "本机读出 255",
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaming_and_office_regs_do_not_overlap() {
        let (g1, g2, g3) = Mode::Gaming.default_power_regs();
        let (o1, o2, o3) = Mode::Office.default_power_regs();
        let gaming = [g1.addr, g2.addr, g3.addr];
        let office = [o1.addr, o2.addr, o3.addr];
        for a in gaming {
            assert!(!office.contains(&a), "Gaming 与 Office 地址不应重叠");
        }
    }

    #[test]
    fn power_regs_are_verified() {
        for mode in [Mode::Gaming, Mode::Office] {
            let regs = mode.default_power_regs();
            for r in [regs.0, regs.1, regs.2] {
                assert_eq!(r.status, AddrStatus::Verified, "{} 应为已验证地址", r.name);
            }
        }
    }

    #[test]
    fn not_applicable_entries_are_marked() {
        assert!(!NOT_APPLICABLE.is_empty());
        for r in NOT_APPLICABLE {
            assert_eq!(r.status, AddrStatus::NotApplicable);
        }
    }
}
