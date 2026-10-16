//! 传感器读数 —— 风扇转速、电源状态等只读信息.
//!
//! 全部为**只读**操作，是安全的功能入口。

use crate::ec::{Ec, EcTransport};
use crate::error::Result;
use crate::namespace;

/// 一份传感器快照.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SensorSnapshot {
    /// 项目 ID（机型标识）.
    pub project_id: u8,
    /// 电源来源原始编码.
    pub power_source: u8,
    /// CPU 风扇转速 (RPM).
    pub cpu_fan_rpm: u16,
    /// GPU 风扇转速 (RPM).
    pub gpu_fan_rpm: u16,
}

impl SensorSnapshot {
    /// 电源是否接着适配器.
    ///
    /// 本机实测适配器下 `ecPowSource = 7`；其余取值待实测确认，
    /// 因此这里只把 7 判定为"接电"，其他一律返回 `false` 而不是猜测.
    #[must_use]
    pub fn is_ac_powered(&self) -> bool {
        self.power_source == 7
    }
}

/// 传感器读取器.
pub struct Sensors<'a, T: EcTransport> {
    ec: &'a Ec<T>,
}

impl<'a, T: EcTransport> Sensors<'a, T> {
    /// 绑定一个 EC 访问器.
    pub fn new(ec: &'a Ec<T>) -> Self {
        Self { ec }
    }

    /// 读取 CPU 风扇转速.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn cpu_fan_rpm(&self) -> Result<u16> {
        combine_rpm(
            self.ec.read(namespace::FAN1_RPM_HIGH.addr)?,
            self.ec.read(namespace::FAN1_RPM_LOW.addr)?,
        )
    }

    /// 读取 GPU 风扇转速.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn gpu_fan_rpm(&self) -> Result<u16> {
        combine_rpm(
            self.ec.read(namespace::FAN2_RPM_HIGH.addr)?,
            self.ec.read(namespace::FAN2_RPM_LOW.addr)?,
        )
    }

    /// 读取项目 ID.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn project_id(&self) -> Result<u8> {
        self.ec.read(namespace::PROJECT_ID.addr)
    }

    /// 读取电源来源.
    ///
    /// # Errors
    /// 读取失败时返回错误.
    pub fn power_source(&self) -> Result<u8> {
        self.ec.read(namespace::POWER_SOURCE.addr)
    }

    /// 一次性读取全部传感器.
    ///
    /// # Errors
    /// 任一读取失败即返回错误.
    pub fn snapshot(&self) -> Result<SensorSnapshot> {
        Ok(SensorSnapshot {
            project_id: self.project_id()?,
            power_source: self.power_source()?,
            cpu_fan_rpm: self.cpu_fan_rpm()?,
            gpu_fan_rpm: self.gpu_fan_rpm()?,
        })
    }
}

/// 把高/低字节组合成 16 位转速值（高字节在前）.
fn combine_rpm(high: u8, low: u8) -> Result<u16> {
    Ok(u16::from(high) << 8 | u16::from(low))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ec::MockTransport;

    fn seeded() -> Ec<MockTransport> {
        let t = MockTransport::new();
        t.seed(0x0740, 16); // 项目 ID
        t.seed(0x0490, 7); // 接适配器
        t.seed(0x0464, 0x0F);
        t.seed(0x0465, 0x82); // CPU 风扇 3970 RPM
        t.seed(0x046C, 0x00);
        t.seed(0x046B, 0x4B); // GPU 风扇 75
        Ec::new(t)
    }

    #[test]
    fn reads_cpu_fan_rpm_from_real_values() {
        let ec = seeded();
        assert_eq!(Sensors::new(&ec).cpu_fan_rpm().unwrap(), 3970);
    }

    #[test]
    fn reads_gpu_fan_rpm_with_swapped_byte_order() {
        let ec = seeded();
        assert_eq!(Sensors::new(&ec).gpu_fan_rpm().unwrap(), 75);
    }

    #[test]
    fn reads_project_id() {
        let ec = seeded();
        assert_eq!(Sensors::new(&ec).project_id().unwrap(), 16);
    }

    #[test]
    fn detects_ac_power() {
        let ec = seeded();
        let snap = Sensors::new(&ec).snapshot().unwrap();
        assert!(snap.is_ac_powered());
        assert_eq!(snap.project_id, 16);
    }

    #[test]
    fn unknown_power_source_is_not_reported_as_ac() {
        let snap = SensorSnapshot {
            project_id: 16,
            power_source: 0,
            cpu_fan_rpm: 0,
            gpu_fan_rpm: 0,
        };
        assert!(!snap.is_ac_powered(), "未知编码不得猜测为接电");
    }

    #[test]
    fn propagates_read_failure() {
        let t = MockTransport::new().with_failing_read(0x0464);
        let ec = Ec::new(t);
        assert!(Sensors::new(&ec).cpu_fan_rpm().is_err());
    }
}
