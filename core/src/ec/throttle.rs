//! EC 访问节流 —— 让"限速"成为不可绕过的基础设施.
//!
//! # 为什么必须存在
//!
//! EC 是 8051 级单片机, **同时负责风扇控制与温控**. 以数百次/秒泛洪 IOCTL
//! 会让它停止响应, 进而导致风扇/温控失控与平台硬复位
//! (本项目开发过程中真实发生过一次无转储硬复位). 详见 `docs/SPEC.md` §5.1.
//!
//! # 为什么放在传输层而不是调用层
//!
//! 限速如果写在业务代码里, 任何人新增一条访问路径都可能绕过它.
//! 因此本模块的 [`AccessThrottle`] 由 [`super::WindowsAcpiTransport`] 在
//! **唯一的 IOCTL 出口**处调用 —— 只要访问真实硬件, 就一定经过限速,
//! 调用方无法选择性跳过.
//!
//! 离线测试用的 [`super::MockTransport`] 不经过限速, 否则 33 个单测会被拖慢.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 默认最小访问间隔 = 20 ms (50 次/秒).
///
/// 这是 `docs/SPEC.md` §5.1 规定的默认安全速率.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(20);

/// 允许的最小间隔 = 10 ms (100 次/秒). 请求低于该值会被强制夹回.
///
/// 越过这条线就有让 EC 停摆的风险, 因此它不提供配置开关.
pub const HARD_MIN_INTERVAL: Duration = Duration::from_millis(10);

/// 串行化 + 限速的访问闸门.
///
/// 语义: "任意两次访问之间至少间隔 `min_interval`". 内部用 `Mutex` 保证
/// 多线程同时调用时也不会并发冲进硬件 (EC 不支持并发访问).
#[derive(Debug)]
pub struct AccessThrottle {
    min_interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl Default for AccessThrottle {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessThrottle {
    /// 用默认间隔 (50 次/秒) 构造.
    #[must_use]
    pub fn new() -> Self {
        Self::with_min_interval(DEFAULT_MIN_INTERVAL)
    }

    /// 用指定间隔构造. 低于 [`HARD_MIN_INTERVAL`] 的请求会被夹回该值.
    #[must_use]
    pub fn with_min_interval(interval: Duration) -> Self {
        Self {
            min_interval: interval.max(HARD_MIN_INTERVAL),
            last: Mutex::new(None),
        }
    }

    /// 实际生效的最小间隔.
    #[must_use]
    pub fn min_interval(&self) -> Duration {
        self.min_interval
    }

    /// 阻塞到下一次访问被允许为止.
    ///
    /// 首次调用不等待; 之后每次都会补齐与上一次之间的间隔.
    /// 锁中毒时继续工作 (宁可限速也不要因为一次 panic 就失去保护).
    pub fn wait(&self) {
        let mut guard = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = *guard {
            let elapsed = previous.elapsed();
            if elapsed < self.min_interval {
                std::thread::sleep(self.min_interval - elapsed);
            }
        }
        *guard = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_fifty_accesses_per_second() {
        let t = AccessThrottle::new();
        assert_eq!(t.min_interval(), Duration::from_millis(20));
    }

    #[test]
    fn clamps_below_hard_floor() {
        let t = AccessThrottle::with_min_interval(Duration::from_millis(0));
        assert_eq!(
            t.min_interval(),
            HARD_MIN_INTERVAL,
            "0 ms 必须被夹回硬下限, 否则可绕过限速"
        );

        let t = AccessThrottle::with_min_interval(Duration::from_micros(500));
        assert_eq!(t.min_interval(), HARD_MIN_INTERVAL);
    }

    #[test]
    fn keeps_interval_above_floor() {
        let t = AccessThrottle::with_min_interval(Duration::from_millis(50));
        assert_eq!(t.min_interval(), Duration::from_millis(50));
    }

    #[test]
    fn first_access_does_not_block() {
        let t = AccessThrottle::with_min_interval(HARD_MIN_INTERVAL);
        let start = Instant::now();
        t.wait();
        assert!(
            start.elapsed() < HARD_MIN_INTERVAL,
            "首次访问不应等待, 实际等了 {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn enforces_spacing_between_accesses() {
        let t = AccessThrottle::with_min_interval(HARD_MIN_INTERVAL);
        t.wait();
        let start = Instant::now();
        t.wait();
        let gap = start.elapsed();
        // 允许 OS 调度抖动, 只断言"补足了大部分间隔"
        assert!(
            gap >= HARD_MIN_INTERVAL - Duration::from_millis(2),
            "两次访问间隔 {gap:?} 小于硬下限 {HARD_MIN_INTERVAL:?}"
        );
    }

    #[test]
    fn serializes_concurrent_callers() {
        use std::sync::Arc;
        use std::time::Duration as D;

        let t = Arc::new(AccessThrottle::with_min_interval(HARD_MIN_INTERVAL));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let t = Arc::clone(&t);
            handles.push(std::thread::spawn(move || {
                for _ in 0..3 {
                    t.wait();
                }
            }));
        }
        let start = Instant::now();
        for h in handles {
            h.join().unwrap();
        }
        // 12 次访问, 至少应耗掉 11 个间隔
        let min_expected = HARD_MIN_INTERVAL * 11;
        assert!(
            start.elapsed() >= min_expected - D::from_millis(5),
            "并发 12 次访问仅耗时 {:?}, 限速未生效",
            start.elapsed()
        );
    }
}
