//! EC 传输层抽象.
//!
//! 把"怎么和硬件说话"与"说什么"分开, 使协议逻辑可以脱离真机测试.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;

/// EC 字节级读写接口.
///
/// 实现者只需提供单字节读写 —— 批量、校验、位操作都由 [`super::Ec`] 组合出来.
pub trait EcTransport: Send + Sync {
    /// 读 1 字节.
    ///
    /// # Errors
    /// 底层访问失败时返回错误. **不得用 0 表示失败** —— 那是官方驱动的坑.
    fn read_ec(&self, addr: u16) -> Result<u8>;

    /// 写 1 字节.
    ///
    /// # Errors
    /// 底层访问失败时返回错误.
    fn write_ec(&self, addr: u16, value: u8) -> Result<()>;
}

/// 内存模拟传输层, 用于单元测试.
///
/// 支持模拟真实硬件的坑:
/// - [`MockTransport::with_stuck`] —— 某地址写了不生效 (官方驱动的"假成功")
/// - [`MockTransport::with_failing_read`] —— 某地址读失败
#[derive(Debug, Default)]
pub struct MockTransport {
    memory: Mutex<HashMap<u16, u8>>,
    stuck: Mutex<Vec<u16>>,
    failing_reads: Mutex<Vec<u16>>,
}

impl Clone for MockTransport {
    fn clone(&self) -> Self {
        Self {
            memory: Mutex::new(self.memory.lock().unwrap().clone()),
            stuck: Mutex::new(self.stuck.lock().unwrap().clone()),
            failing_reads: Mutex::new(self.failing_reads.lock().unwrap().clone()),
        }
    }
}

impl MockTransport {
    /// 新建空模拟层 (所有地址初始为 0).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 预设某地址的初始值.
    pub fn seed(&self, addr: u16, value: u8) {
        self.memory.lock().unwrap().insert(addr, value);
    }

    /// 标记某地址为"写入不生效" (读回永远是旧值).
    #[must_use]
    pub fn with_stuck(self, addr: u16) -> Self {
        self.stuck.lock().unwrap().push(addr);
        self
    }

    /// 标记某地址读取失败.
    #[must_use]
    pub fn with_failing_read(self, addr: u16) -> Self {
        self.failing_reads.lock().unwrap().push(addr);
        self
    }
}

impl EcTransport for MockTransport {
    fn read_ec(&self, addr: u16) -> Result<u8> {
        if self.failing_reads.lock().unwrap().contains(&addr) {
            return Err(Error::Ioctl {
                code: super::IOCTL_EC_READ,
                source: std::io::Error::other(format!("模拟读取失败 @ 0x{addr:04X}")),
            });
        }
        Ok(*self.memory.lock().unwrap().get(&addr).unwrap_or(&0))
    }

    fn write_ec(&self, addr: u16, value: u8) -> Result<()> {
        if self.stuck.lock().unwrap().contains(&addr) {
            // 模拟硬件忽略写入: 不更新内存
            return Ok(());
        }
        self.memory.lock().unwrap().insert(addr, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_stores_and_returns_values() {
        let t = MockTransport::new();
        t.write_ec(0x0100, 0xAB).unwrap();
        assert_eq!(t.read_ec(0x0100).unwrap(), 0xAB);
    }

    #[test]
    fn mock_returns_zero_for_unset() {
        let t = MockTransport::new();
        assert_eq!(t.read_ec(0x9999).unwrap(), 0);
    }

    #[test]
    fn stuck_address_ignores_writes() {
        let t = MockTransport::new().with_stuck(0x0100);
        t.write_ec(0x0100, 0xAB).unwrap();
        assert_eq!(t.read_ec(0x0100).unwrap(), 0, "stuck 地址应保持原值");
    }

    #[test]
    fn failing_read_returns_error_not_zero() {
        let t = MockTransport::new().with_failing_read(0x0100);
        assert!(t.read_ec(0x0100).is_err(), "读取失败必须报错, 不能返回 0");
    }
}
