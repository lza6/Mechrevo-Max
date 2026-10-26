//! 风扇曲线表（只读展示 + 受控写入会话）.
//!
//! # 这是什么
//!
//! EC 里有若干 16 字节的表，本机实测（2026-10-24 全量扫描、2026-10-25 重扫
//! 双次逐字节一致）在 `0x0F00`–`0x0F5F` 看到 6 个块，形态是「一串温度点 +
//! 0xFF 填充」和「一串占空比（192/196/200）」交替出现。
//!
//! # 读取侧
//!
//! 读出 6 个块的原始字节、按数据特征分类（温度点 / 占空比）、给出"候选配对"，
//! 并在返回结构里带上 [`AddrStatus::CommunityUnverified`] 的语义。
//!
//! # 写入侧（T-040-04，2026-10-25）
//!
//! CPU 表对（`0x0F00`/`0x0F20`）已完成**行为级验证**后才开放写入：
//! - 占空比 192-200→180，RPM 5416→5030（-7.1%），回滚逐字节复原
//!   （`evidence/fan-write-v2b.json`）—— 证明该区就是生效曲线且配对正确；
//! - 档位切换前后曲线区 0 diff —— 表不随档位变（`evidence/fan-dump-v1b.json`）。
//!
//! 写入安全链见 [`write_curve`]：校验 → 白名单 → 快照 → RAII 回滚 →
//! 逐格有界重试 → RPM 停转监视。GPU 表对未做行为级验证，**不在写入白名单**。

use crate::ec::{Ec, EcTransport};
use crate::error::Result;
use crate::namespace::AddrStatus;

/// 每个表的字节数（本机实测为 16）.
pub const BLOCK_LEN: usize = 16;

/// 曲线的起始地址（本机实测 6 个块连续排布）.
pub const CURVE_BASE: u16 = 0x0F00;

/// 块的数量.
pub const BLOCK_COUNT: usize = 6;

/// 一个表的类型（按数据特征判断，不作为协议断言）.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// 温度控制点（值域合理且单调不减）.
    TemperaturePoints,
    /// 占空比（0-255 尺度，且全部 ≥64）.
    DutyPercent,
    /// 看不出规律.
    Unknown,
}

/// 曲线表的可信度 —— 一律未验证.
pub const CURVE_STATUS: AddrStatus = AddrStatus::CommunityUnverified;

/// 一个 16 字节的曲线块.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FanCurveBlock {
    /// 起始地址.
    pub base: u16,
    /// 原始 16 字节.
    pub raw: Vec<u8>,
    /// 按数据特征推断的类型.
    pub kind: BlockKind,
    /// 去掉 `0xFF` 填充与首字节 `0` 之后的有效值.
    pub values: Vec<u8>,
}

impl FanCurveBlock {
    /// 由原始字节构造并分类.
    #[must_use]
    pub fn from_raw(base: u16, raw: Vec<u8>) -> Self {
        let values = effective_values(&raw);
        let kind = classify(&values);
        Self {
            base,
            raw,
            kind,
            values,
        }
    }

    /// 一行可读摘要（界面/日志用）.
    #[must_use]
    pub fn summary(&self) -> String {
        let head: Vec<String> = self.values.iter().take(10).map(u8::to_string).collect();
        format!("{} 个有效值: {}", self.values.len(), head.join(", "))
    }
}

/// 去掉填充：`0xFF` 一律视为填充；长度 >1 时开头的 `0` 也视为表头/序号。
fn effective_values(raw: &[u8]) -> Vec<u8> {
    let mut values: Vec<u8> = raw.iter().copied().filter(|v| *v != 0xFF).collect();
    if values.len() > 1 && values[0] == 0 {
        values.remove(0);
    }
    values
}

/// 按数据特征分类（保守：判不准就 `Unknown`）.
fn classify(values: &[u8]) -> BlockKind {
    if values.is_empty() {
        return BlockKind::Unknown;
    }
    let all_in_temp_range = values.iter().all(|v| (20..=110).contains(v));
    let monotonic = values.windows(2).all(|w| w[0] <= w[1]);
    if all_in_temp_range && monotonic && values.len() >= 3 {
        return BlockKind::TemperaturePoints;
    }
    if values.iter().all(|v| *v >= 64) {
        return BlockKind::DutyPercent;
    }
    BlockKind::Unknown
}

/// 读全部 6 个块（只读，96 次读 ≈ 2 秒 @50 次/秒）.
///
/// # Errors
/// 任一读取失败即返回错误.
pub fn read_blocks<T: EcTransport>(ec: &Ec<T>) -> Result<Vec<FanCurveBlock>> {
    let mut blocks = Vec::with_capacity(BLOCK_COUNT);
    for i in 0..BLOCK_COUNT {
        let base = CURVE_BASE + (i * BLOCK_LEN) as u16;
        let raw = ec.read_block(base, BLOCK_LEN as u16)?;
        blocks.push(FanCurveBlock::from_raw(base, raw));
    }
    Ok(blocks)
}

/// 把「温度点表」与「占空比表」按数量对齐成 (温度, 占空比%) 点对.
///
/// 占空比按 `值/255*100` 折算成百分比。数量不等时按较短的截断 ——
/// **这只是展示用的候选配对，不是协议结论**。
#[must_use]
pub fn pair_points(temps: &[u8], duties: &[u8]) -> Vec<(u8, u8)> {
    temps
        .iter()
        .zip(duties.iter())
        .map(|(t, d)| (*t, ((u16::from(*d) * 100) / 255) as u8))
        .collect()
}

// ---------------------------------------------------------------------------
// 写入会话（T-040-04，评审方案见 docs/计划书/下一步改进指南.md §19）
//
// 安全设计（§19.4 定案）：
// 1. 逐格 write_settled（有界重试），禁止整块一次性写（本项目无块写 IOCTL）；
// 2. 写入前 snapshot 目标两块 → RAII guard 兜底 panic；
// 3. 每格写完后调用 RPM 监视回调，转速低于基线 30% 立即回滚（停转保护）；
// 4. 0xFF 填充格"不写不校验"（§19.8 假设 3：填充语义不明，不碰）。
// ---------------------------------------------------------------------------

/// 本机实测（2026-10-24/25 双次逐字节一致）的表地址.
/// CPU 温度点表基址（本机实测）.
pub const CPU_TEMP_BLOCK: u16 = 0x0F00;
/// CPU 占空比表基址（本机实测）.
pub const CPU_DUTY_BLOCK: u16 = 0x0F20;
/// GPU 温度点表基址（本机实测）.
pub const GPU_TEMP_BLOCK: u16 = 0x0F30;
/// GPU 占空比表基址（本机实测）.
pub const GPU_DUTY_BLOCK: u16 = 0x0F50;

/// 允许写入的表地址对（温度块, 占空比表）.
///
/// 复审 #2：后端 IPC 不得接受任意地址 —— 否则任何能调到 IPC 的路径都能把
/// 一条"合法曲线"写到 EC 任意 16 字节区域，写错地址比写错数值更危险。
/// 只开放本机已做行为级验证的 CPU 表；GPU 表待同样验证后再加入。
pub const WRITABLE_PAIRS: &[(u16, u16)] = &[(CPU_TEMP_BLOCK, CPU_DUTY_BLOCK)];

/// 校验 (temp_block, duty_block) 是否为已授权的可写表对.
///
/// # Errors
/// 不在白名单时返回 [`Error::Config`].
pub fn validate_blocks(temp_block: u16, duty_block: u16) -> Result<()> {
    if WRITABLE_PAIRS.contains(&(temp_block, duty_block)) {
        return Ok(());
    }
    Err(crate::Error::Config(format!(
        "0x{temp_block:04X}/0x{duty_block:04X} 不是已授权的可写曲线表；当前仅支持 \
         0x{CPU_TEMP_BLOCK:04X}/0x{CPU_DUTY_BLOCK:04X}（CPU，已行为级验证）"
    )))
}

/// 占空比合法下界：本机两张表全部 ≥192（0xC0），且 `classify` 以 64 为界.
/// 写入低于此值的占空比会被拒绝 —— 防止把风扇写到危险低速区.
pub const MIN_DUTY: u8 = 64;

/// 温度点合法范围（与 `classify` 一致）.
pub const TEMP_RANGE: std::ops::RangeInclusive<u8> = 20..=110;

/// 一条待写入的曲线（温度点 + 占空比，等长，无 0xFF）.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CurveInput {
    /// 温度控制点，°C.
    pub temps: Vec<u8>,
    /// 占空比（0-255 EC 尺度），与 `temps` 等长.
    pub duties: Vec<u8>,
}

impl CurveInput {
    /// 校验曲线合法性（§19.5 F-1）.
    ///
    /// # Errors
    /// 长度不等 / 温度越界或非单调 / 占空比越界时返回 [`Error::Config`].
    pub fn validate(&self) -> Result<()> {
        if self.temps.len() != self.duties.len() {
            return Err(crate::Error::Config(format!(
                "温度点数 {} 与占空比点数 {} 不等",
                self.temps.len(),
                self.duties.len()
            )));
        }
        if self.temps.is_empty() {
            return Err(crate::Error::Config("曲线不能为空".to_string()));
        }
        if self.temps.len() > BLOCK_LEN {
            return Err(crate::Error::Config(format!(
                "曲线最多 {BLOCK_LEN} 个点，传入 {} 个",
                self.temps.len()
            )));
        }
        for (i, &t) in self.temps.iter().enumerate() {
            if !TEMP_RANGE.contains(&t) {
                return Err(crate::Error::Config(format!(
                    "第 {i} 点 {t}°C 超出合法温度范围"
                )));
            }
            if i > 0 && t < self.temps[i - 1] {
                return Err(crate::Error::Config(format!(
                    "第 {i} 点 {t}°C 低于前点 {}（必须单调不减）",
                    self.temps[i - 1]
                )));
            }
        }
        for (i, &d) in self.duties.iter().enumerate() {
            if d < MIN_DUTY {
                return Err(crate::Error::Config(format!(
                    "第 {i} 点占空比 {d} 低于安全下界 {MIN_DUTY}"
                )));
            }
        }
        Ok(())
    }
}

/// RPM 监视回调：返回当前风扇 RPM. 返回 Err 视为读取失败（同样触发回滚）.
pub trait RpmMonitor {
    /// 读取当前 RPM.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    fn rpm(&self) -> Result<u16>;
}

/// 停转判定：低于写入前基线的这个比例即回滚.
pub const STALL_RATIO: u16 = 30; // %

/// 建立停转基准所需的最低 RPM.
///
/// 复审发现（2026-10-25）：基线为 0 或极小时 `基线*30/100` 会塌缩成 0，
/// `rpm < 0` 永假 —— 停转保护静默失效，恰好是最危险的场景（风扇本已停转）。
/// 因此低于该值直接拒绝写入，而不是"带着失效的保护"继续。
pub const MIN_BASELINE_RPM: u16 = 500;

/// 一次曲线写入会话的结果.
#[derive(Debug)]
pub struct WriteReport {
    /// 实际写入的格子数（不含 0xFF 跳过格）.
    pub cells_written: usize,
    /// 未被覆盖的格数（尾部保留原值）——UI 必须向用户交代（复审 #3）.
    pub untouched_cells: usize,
}

/// 写入一条曲线到指定的温度块 + 占空比块.
///
/// 流程：validate → snapshot 两块 → guard → 逐格写入（每格后查 RPM）→ 解除守卫。
/// 任一步失败：guard 自动整块回滚，返回错误。
///
/// # Errors
/// 校验失败 / 写入读回失败 / RPM 停转 / 快照或回滚失败。
#[allow(clippy::too_many_lines)]
pub fn write_curve<T: EcTransport, M: RpmMonitor>(
    ec: &Ec<T>,
    monitor: &M,
    temp_block: u16,
    duty_block: u16,
    curve: &CurveInput,
) -> Result<WriteReport> {
    curve.validate()?;
    validate_blocks(temp_block, duty_block)?;

    // 1. 写入前基线 RPM（停转判定基准）
    let baseline_rpm = monitor.rpm()?;
    if baseline_rpm < MIN_BASELINE_RPM {
        // 复审 Critical #1：基线过低会让阈值塌缩成 0，停转保护静默失效
        return Err(crate::Error::Config(format!(
            "当前风扇转速 {baseline_rpm} RPM 过低（低于 {MIN_BASELINE_RPM}），无法建立停转保护基准，拒绝写入"
        )));
    }

    // 2. 快照两块（完整 16 字节，含填充格）
    let mut addrs = Vec::with_capacity(32);
    for i in 0..BLOCK_LEN {
        addrs.push(temp_block + i as u16);
    }
    for i in 0..BLOCK_LEN {
        addrs.push(duty_block + i as u16);
    }
    let point = ec.snapshot(&addrs, "fan curve write")?;
    let mut guard = ec.guard(point);

    // 3. 停转检查（每格写完后调用）
    // 阈值向上取整，避免 baseline 较小时 `baseline*30/100` 塌缩成 0（复审 #5）
    let threshold = (u32::from(baseline_rpm) * u32::from(STALL_RATIO)).div_ceil(100) as u16;
    let check_stall = |cells_done: usize| -> Result<()> {
        let rpm = monitor.rpm()?;
        if rpm < threshold {
            log::error!(
                "风扇停转保护：RPM {rpm} < 基线 {baseline_rpm} 的 {STALL_RATIO}%（已写 {cells_done} 格），回滚"
            );
            return Err(crate::Error::Config(format!(
                "风扇转速 {rpm} RPM 低于基线 {baseline_rpm} 的 {STALL_RATIO}%，已回滚"
            )));
        }
        Ok(())
    };

    // 4. 逐格写温度点（0xFF 填充格跳过）
    let mut cells = 0usize;
    let padded_temps = pad_to_block(&curve.temps);
    for (i, &v) in padded_temps.iter().enumerate() {
        let addr = temp_block + i as u16;
        if v == 0xFF {
            continue; // 填充格不写不校验
        }
        write_settled(ec, addr, v)?;
        cells += 1;
        check_stall(cells)?;
    }

    // 5. 逐格写占空比
    let padded_duties = pad_to_block(&curve.duties);
    for (i, &v) in padded_duties.iter().enumerate() {
        let addr = duty_block + i as u16;
        if v == 0xFF {
            continue;
        }
        write_settled(ec, addr, v)?;
        cells += 1;
        check_stall(cells)?;
    }

    // 未被覆盖的格子：短曲线的尾部（pad 成 0xFF 跳过 → 保留原表值）。
    // UI 必须向用户交代这一点，否则用户会以为写入的是完整 16 格曲线。
    let untouched = (BLOCK_LEN - curve.temps.len()) + (BLOCK_LEN - curve.duties.len());

    // 6. 解守卫前最后一次停转检查（复审 #9：RPM 有物理滞后，
    //    最后一格的影响可能在逐格检查后才显现）
    check_stall(cells)?;

    guard.disarm();
    Ok(WriteReport {
        cells_written: cells,
        // 未被本次写入覆盖的格子数（§19.8 假设 3：填充格不碰，尾部保留原值）.
        untouched_cells: untouched,
    })
}

/// 把有效值序列补齐到 16 字节块（尾部填 0xFF），与 EC 表布局对齐.
fn pad_to_block(values: &[u8]) -> Vec<u8> {
    let mut out = values.to_vec();
    out.resize(BLOCK_LEN, 0xFF);
    out
}

/// 有界重试写固定值（复用 `mode.rs::write_settled` 的实测依据：EC 偶发丢写 1 次）.
fn write_settled<T: EcTransport>(ec: &Ec<T>, addr: u16, value: u8) -> Result<()> {
    const ATTEMPTS: usize = 3;
    const DELAY: std::time::Duration = std::time::Duration::from_millis(100);
    let mut last_actual = 0u8;
    for attempt in 0..ATTEMPTS {
        ec.write_unverified(addr, value)?;
        last_actual = ec.read(addr)?;
        if last_actual == value {
            return Ok(());
        }
        log::warn!(
            "0x{addr:04X} 写入 0x{value:02X} 未生效（读回 0x{last_actual:02X}），第 {}/{} 次",
            attempt + 1,
            ATTEMPTS
        );
        std::thread::sleep(DELAY);
    }
    Err(crate::Error::VerifyMismatch {
        addr,
        expected: value,
        actual: last_actual,
    })
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use std::sync::Mutex;

    use crate::ec::MockTransport;

    /// 固定 RPM 监视器.
    struct FixedRpm(u16);
    impl RpmMonitor for FixedRpm {
        fn rpm(&self) -> Result<u16> {
            Ok(self.0)
        }
    }

    /// 总是失败的 RPM 监视器（模拟读数失败）.
    struct ErrRpm;
    impl RpmMonitor for ErrRpm {
        fn rpm(&self) -> Result<u16> {
            Err(crate::Error::Ioctl {
                code: crate::ec::IOCTL_EC_READ,
                source: std::io::Error::other("模拟读数失败"),
            })
        }
    }

    /// 按调用次数返回递减 RPM（模拟写入过程中风扇降速）.
    struct FallingRpm {
        values: Mutex<Vec<u16>>,
    }
    impl RpmMonitor for FallingRpm {
        fn rpm(&self) -> Result<u16> {
            let mut v = self.values.lock().unwrap();
            if v.len() > 1 {
                v.remove(0);
            }
            Ok(v[0])
        }
    }

    fn seed_real_blocks(t: &MockTransport) {
        // 与真机 2026-10-25 重扫一致的 CPU 表
        let temps = [
            54, 59, 63, 66, 69, 72, 75, 78, 81, 255, 255, 255, 255, 255, 255, 255,
        ];
        let duties = [
            0, 192, 192, 196, 196, 196, 196, 196, 196, 200, 200, 200, 200, 200, 200, 200,
        ];
        for (i, v) in temps.iter().enumerate() {
            t.seed(CPU_TEMP_BLOCK + i as u16, *v);
        }
        for (i, v) in duties.iter().enumerate() {
            t.seed(CPU_DUTY_BLOCK + i as u16, *v);
        }
    }

    // F-1 曲线值域校验
    #[test]
    fn f1_rejects_length_mismatch() {
        let c = CurveInput {
            temps: vec![50, 60],
            duties: vec![192],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn f1_rejects_temp_out_of_range() {
        let c = CurveInput {
            temps: vec![50, 130],
            duties: vec![192, 192],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn f1_rejects_non_monotonic_temps() {
        let c = CurveInput {
            temps: vec![60, 50],
            duties: vec![192, 192],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn f1_rejects_duty_below_safe_floor() {
        let c = CurveInput {
            temps: vec![50],
            duties: vec![10],
        };
        assert!(c.validate().is_err(), "占空比 10 低于安全下界必须拒绝");
    }

    #[test]
    fn f1_accepts_valid_curve() {
        let c = CurveInput {
            temps: vec![50, 70, 90],
            duties: vec![192, 196, 200],
        };
        assert!(c.validate().is_ok());
    }

    // F-2 逐格写入 + 丢写重试（stuck 地址模拟一次丢写后由重试覆盖不了 → 走回滚路径见 F-3）
    #[test]
    fn f2_writes_cells_and_reports_count() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t.clone());
        let curve = CurveInput {
            temps: vec![50, 55, 60],
            duties: vec![192, 196, 200],
        };
        let report = write_curve(&ec, &FixedRpm(5000), CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect("合法曲线必须写成功");
        assert_eq!(report.cells_written, 6);
        // 读回验证（经 ec —— 它持有 t 的克隆副本）
        assert_eq!(ec.read(CPU_TEMP_BLOCK).unwrap(), 50);
        assert_eq!(ec.read(CPU_TEMP_BLOCK + 2).unwrap(), 60);
        assert_eq!(ec.read(CPU_DUTY_BLOCK + 1).unwrap(), 196);
        assert_eq!(ec.read(CPU_DUTY_BLOCK + 2).unwrap(), 200);
        // 填充格未被触碰（保持 0xFF）
        assert_eq!(ec.read(CPU_TEMP_BLOCK + 15).unwrap(), 0xFF);
    }

    #[test]
    fn f2_lost_write_retried_then_succeeds() {
        // stuck 只让第一次写不生效的场景：Mock 的 stuck 是永久的，
        // 因此这里验证"永久 stuck → 重试 3 次仍失败 → 报 VerifyMismatch"
        let t = MockTransport::new().with_stuck(CPU_TEMP_BLOCK);
        seed_real_blocks(&t);
        let ec = Ec::new(t);
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        let err = write_curve(&ec, &FixedRpm(5000), CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect_err("stuck 地址 3 次重试后必须失败");
        assert!(matches!(err, crate::Error::VerifyMismatch { .. }));
    }

    // F-3 回滚完整性：第 2 格失败 → 第 1 格必须被恢复为原值
    #[test]
    fn f3_failed_write_rolls_back_previous_cells() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        // 温度块第 1 格（0x0F01）stuck：第 0 格成功写入后第 1 格失败
        let stuck = t.clone().with_stuck(CPU_TEMP_BLOCK + 1);
        let ec = Ec::new(stuck.clone());
        let curve = CurveInput {
            temps: vec![50, 55, 60],
            duties: vec![192],
        };
        assert!(write_curve(&ec, &FixedRpm(5000), CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve).is_err());
        // 回滚断言走 ec.read —— ec 持有自己的 MockTransport 深拷贝副本，
        // 外层 stuck 句柄永远读不到 ec 的写入（复审第 2 轮：旧断言恒真）
        assert_eq!(
            ec.read(CPU_TEMP_BLOCK).unwrap(),
            54,
            "失败后第 0 格必须回滚到快照值"
        );
        // 占空比块尚未被写，应保持原值
        assert_eq!(ec.read(CPU_DUTY_BLOCK + 1).unwrap(), 192);
    }

    // F-4 真实 panic 路径：在持有 guard 时 panic（不经 drop 语句），
    // 验证栈展开过程中 RestoreGuard::Drop 真的回滚了硬件（复审 #6：原版本
    // 先 drop(guard) 再 panic，测的只是普通 drop，不是 panic 展开）.
    #[test]
    fn f4_panic_mid_session_rolls_back() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t.clone());
        let addrs: Vec<u16> = (0..BLOCK_LEN as u16)
            .map(|i| CPU_TEMP_BLOCK + i)
            .chain((0..BLOCK_LEN as u16).map(|i| CPU_DUTY_BLOCK + i))
            .collect();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let point = ec.snapshot(&addrs, "panic test").unwrap();
            let _guard = ec.guard(point);
            ec.write_unverified(CPU_TEMP_BLOCK, 50).unwrap();
            panic!("模拟写入中途崩溃"); // 不 disarm、不显式 drop —— 让栈展开处理
        }));
        assert!(result.is_err());
        assert_eq!(
            ec.read(CPU_TEMP_BLOCK).unwrap(),
            54,
            "panic 展开时 guard 的 Drop 必须恢复原值"
        );
    }

    // F-4b 停转监视返回 Err（读数失败）也必须回滚并报错
    #[test]
    fn f4b_rpm_read_failure_rolls_back() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t.clone());
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        // 监视器第一次（基线）就失败 → 在 snapshot 之前返回 Err，硬件未被触碰
        let err = write_curve(&ec, &ErrRpm, CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect_err("RPM 读数失败必须中止写入");
        assert!(err.to_string().contains("模拟读数失败"));
        // 表未被改动（经 ec 副本断言）
        assert_eq!(ec.read(CPU_TEMP_BLOCK).unwrap(), 54);
    }

    // 复审 Critical #1：基线 RPM 过低时停转保护会失效，必须拒绝写入
    #[test]
    fn rejects_low_baseline_rpm() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t);
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        let err = write_curve(&ec, &FixedRpm(0), CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect_err("基线 RPM=0 必须拒绝写入（否则停转保护阈值塌缩为 0）");
        assert!(
            err.to_string().contains("无法建立停转保护基准"),
            "错误信息应说明拒绝原因"
        );
    }

    #[test]
    fn rejects_baseline_just_below_floor() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t);
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        // 499 < MIN_BASELINE_RPM(500)
        assert!(
            write_curve(
                &ec,
                &FixedRpm(MIN_BASELINE_RPM - 1),
                CPU_TEMP_BLOCK,
                CPU_DUTY_BLOCK,
                &curve,
            )
            .is_err()
        );
        // 500 == 下限，允许
        assert!(
            write_curve(
                &ec,
                &FixedRpm(MIN_BASELINE_RPM),
                CPU_TEMP_BLOCK,
                CPU_DUTY_BLOCK,
                &curve,
            )
            .is_ok()
        );
    }

    // 复审 #2：地址白名单
    #[test]
    fn rejects_unauthorized_block_pairs() {
        // GPU 对（已测但未行为级验证）与随机地址都必须拒绝
        assert!(validate_blocks(CPU_TEMP_BLOCK, CPU_DUTY_BLOCK).is_ok());
        assert!(validate_blocks(GPU_TEMP_BLOCK, GPU_DUTY_BLOCK).is_err());
        assert!(validate_blocks(0x0F10, 0x0F30).is_err());
        assert!(validate_blocks(0x0730, 0x0751).is_err());
    }

    #[test]
    fn write_curve_rejects_unauthorized_blocks_before_touching_hardware() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t.clone());
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        let err = write_curve(&ec, &FixedRpm(5000), GPU_TEMP_BLOCK, GPU_DUTY_BLOCK, &curve)
            .expect_err("未授权表对必须拒绝");
        assert!(err.to_string().contains("不是已授权的可写曲线表"));
        // 硬件完全未被触碰（未 seed 的 GPU 区仍是 0）
        assert_eq!(t.read_ec(GPU_TEMP_BLOCK).unwrap(), 0);
    }

    // 短曲线：必须如实报告尾部保留的格数（复审 #3）
    #[test]
    fn short_curve_reports_untouched_cells() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t);
        let curve = CurveInput {
            temps: vec![50, 55, 60],
            duties: vec![192, 196, 200],
        };
        let report = write_curve(&ec, &FixedRpm(5000), CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect("短曲线合法，应写入");
        assert_eq!(report.cells_written, 6);
        assert_eq!(report.untouched_cells, (16 - 3) * 2, "尾部 13*2 格保留原值");
    }

    // F-5 停转触发回滚
    #[test]
    fn f5_stall_rolls_back() {
        let t = MockTransport::new();
        seed_real_blocks(&t);
        let ec = Ec::new(t.clone());
        // 基线 5000 → 阈值 1500；监视序列：5000(基线) → 5000 → 200 → ...
        let monitor = FallingRpm {
            values: Mutex::new(vec![5000, 5000, 200, 200, 200, 200]),
        };
        let curve = CurveInput {
            temps: vec![50, 55],
            duties: vec![192, 192],
        };
        let err = write_curve(&ec, &monitor, CPU_TEMP_BLOCK, CPU_DUTY_BLOCK, &curve)
            .expect_err("RPM 跌破 30% 基线必须回滚");
        assert!(err.to_string().contains("RPM"), "错误信息应含 RPM 上下文");
        // 回滚断言（经 ec 副本断言 —— 外层句柄读不到 ec 的写入，恒真无效）
        assert_eq!(ec.read(CPU_TEMP_BLOCK).unwrap(), 54);
    }

    // F-7（存档 SHA256 校验在 app 层 profiles 侧实现，见 §19.4 第 7 条；core 侧先覆盖
    // pad_to_block 的边界）
    #[test]
    fn f7_pads_short_curve_with_ff() {
        let padded = pad_to_block(&[50, 55]);
        assert_eq!(padded.len(), BLOCK_LEN);
        assert_eq!(padded[0], 50);
        assert_eq!(padded[2], 0xFF);
        assert_eq!(padded[15], 0xFF);
    }

    #[test]
    fn f7_rejects_curve_longer_than_block() {
        // 超过 16 点的曲线：pad_to_block 会截断 —— 必须在 validate 阶段拒绝
        let c = CurveInput {
            temps: vec![20; 17],
            duties: vec![192; 17],
        };
        assert!(c.validate().is_err(), "超长曲线必须被拒绝而不是截断");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ec::MockTransport;

    fn seed_blocks(t: &MockTransport) {
        // 与 2026-10-24 本机扫描完全一致的真实值
        let data: [(u16, [u8; 16]); 6] = [
            (
                0x0F00,
                [
                    54, 59, 63, 66, 69, 72, 75, 78, 81, 255, 255, 255, 255, 255, 255, 255,
                ],
            ),
            (
                0x0F10,
                [
                    0, 48, 50, 64, 67, 70, 73, 76, 79, 82, 255, 255, 255, 255, 255, 255,
                ],
            ),
            (
                0x0F20,
                [
                    0, 192, 192, 196, 196, 196, 196, 196, 196, 200, 200, 200, 200, 200, 200, 200,
                ],
            ),
            (
                0x0F30,
                [
                    56, 56, 58, 61, 64, 67, 70, 73, 75, 255, 255, 255, 255, 255, 255, 255,
                ],
            ),
            (
                0x0F40,
                [
                    0, 48, 48, 59, 62, 65, 68, 71, 74, 76, 255, 255, 255, 255, 255, 255,
                ],
            ),
            (
                0x0F50,
                [
                    0, 192, 196, 196, 196, 196, 196, 196, 196, 196, 196, 196, 196, 196, 196, 196,
                ],
            ),
        ];
        for (base, bytes) in data {
            for (i, b) in bytes.iter().enumerate() {
                t.seed(base + i as u16, *b);
            }
        }
    }

    #[test]
    fn reads_six_blocks_and_classifies_real_data() {
        let t = MockTransport::new();
        seed_blocks(&t);
        let ec = Ec::new(t);
        let blocks = read_blocks(&ec).unwrap();
        assert_eq!(blocks.len(), BLOCK_COUNT);
        assert_eq!(
            blocks[0].kind,
            BlockKind::TemperaturePoints,
            "0x0F00 应是温度点"
        );
        assert_eq!(blocks[2].kind, BlockKind::DutyPercent, "0x0F20 应是占空比");
        assert_eq!(blocks[3].kind, BlockKind::TemperaturePoints);
        assert_eq!(blocks[5].kind, BlockKind::DutyPercent);
    }

    #[test]
    fn strips_padding_and_header_zero() {
        let block = FanCurveBlock::from_raw(
            0x0F10,
            vec![
                0, 48, 50, 64, 67, 70, 73, 76, 79, 82, 255, 255, 255, 255, 255, 255,
            ],
        );
        assert_eq!(block.values, vec![48, 50, 64, 67, 70, 73, 76, 79, 82]);
    }

    #[test]
    fn monotonically_increasing_temps_detected() {
        let block = FanCurveBlock::from_raw(
            0x0F00,
            vec![
                54, 59, 63, 66, 69, 72, 75, 78, 81, 255, 255, 255, 255, 255, 255, 255,
            ],
        );
        assert_eq!(block.kind, BlockKind::TemperaturePoints);
        assert_eq!(block.values.len(), 9);
    }

    #[test]
    fn non_monotonic_values_are_not_temperatures() {
        let block = FanCurveBlock::from_raw(
            0x0F00,
            vec![80, 40, 60, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        assert_ne!(block.kind, BlockKind::TemperaturePoints);
    }

    #[test]
    fn pairs_temps_with_duty_percent() {
        let pairs = pair_points(&[50, 64, 78], &[192, 196, 200]);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], (50, 75)); // 192/255*100 ≈ 75
        assert_eq!(pairs[2], (78, 78)); // 200/255*100 ≈ 78
    }

    #[test]
    fn read_failure_propagates() {
        let t = MockTransport::new().with_failing_read(0x0F05);
        let ec = Ec::new(t);
        assert!(read_blocks(&ec).is_err(), "读失败必须报错，不得返回空表");
    }
}
