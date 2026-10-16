//! 机械革命 (Mechrevo) 笔记本硬件控制核心库.
//!
//! # 项目背景
//!
//! 官方「机械革命电竞控制台」(ControlCenter3) 是一个 .NET Native AOT 编译的 UWP 应用,
//! 常驻内存约 178 MB (主程序 137 MB + 托盘 41 MB), 且存在若干落地缺陷
//! (例如安装后不触发 PnP 扫描导致 EC 驱动无设备可绑). 本库是自研替代方案的核心:
//! 直接通过内核驱动访问 EC, 不依赖官方任何组件.
//!
//! # 安全设计
//!
//! 写 EC 是真硬件操作. 本库强制以下纪律:
//!
//! 1. **失败必须报错** —— 绝不像官方驱动那样把失败伪造成成功, 也不用 0 表示读失败.
//! 2. **写后读回校验** —— [`ec::Ec::write_verified`] 是默认写入方式.
//! 3. **改动前存回滚点** —— [`ec::Ec::snapshot`] + [`ec::Ec::restore`].
//! 4. **传输层可替换** —— 协议逻辑能用 [`ec::MockTransport`] 离线测试.
//!
//! # 快速开始
//!
//! ```no_run
//! use mechrevo_core::ec::{Ec, WindowsAcpiTransport};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ec = Ec::new(WindowsAcpiTransport::new()?);
//! let project_id = ec.read(0x0740)?;
//! println!("项目 ID = {project_id}");
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod control;
pub mod ec;
pub mod error;
pub mod mode;
pub mod namespace;
pub mod sensors;

pub use control::{ChangeRecord, PowerController, PowerLimits};
pub use ec::{Ec, EcTransport, MockTransport, RollbackPoint, WindowsAcpiTransport};
pub use error::{Error, Result};
pub use mode::{ModeChange, ModeController, OperatingMode};
pub use namespace::{AddrStatus, Mode, Reg};
pub use sensors::{SensorSnapshot, Sensors};
