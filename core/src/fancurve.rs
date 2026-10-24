//! 风扇曲线表（**只读**）.
//!
//! # 这是什么
//!
//! EC 里有若干 16 字节的表，本机实测（2026-10-24 全量扫描）在 `0x0F00`–`0x0F5F`
//! 看到 6 个块，形态是「一串温度点 + 0xFF 填充」和「一串占空比（192/196/200）」交替出现。
//!
//! # 为什么只读、且不宣称"这就是风扇曲线"
//!
//! - 官方 profile 里只有 `FAN.TableName = "M1T1"` 这类**表名**，真正的曲线数值在
//!   `UserFanTables/PH6PG*` 资源里，本机备份中没有，**无法逐点交叉核对**。
//! - 因此本模块只做三件有把握的事：读出这 6 个块的原始字节、按数据特征分类
//!   （温度点 / 占空比）、给出"候选配对"，并在返回结构里带上
//!   [`AddrStatus::CommunityUnverified`] 的语义 —— **任何一处写入都不基于本模块**。
//!
//! 写曲线是**高风险操作**（写错风扇可能不转 → 过热），必须等只读展示被确认无害、
//! 且与官方表交叉核对之后再做。

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
