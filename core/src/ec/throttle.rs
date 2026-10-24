//! EC 访问节流 —— 让"限速 + 不并发"成为不可绕过的基础设施.
//!
//! # 为什么必须存在
//!
//! EC 是 8051 级单片机, **同时负责风扇控制与温控**. 以数百次/秒泛洪 IOCTL
//! 会让它停止响应, 进而导致风扇/温控失控与平台硬复位
//! (本项目开发过程中真实发生过一次无转储硬复位). 详见 `docs/SPEC.md` §5.1.
//!
//! # 两个必须同时成立的保证
//!
//! 1. **间隔**: 任意两次硬件访问的起点之间至少相隔 [`DEFAULT_MIN_INTERVAL`].
//! 2. **互斥**: 两次硬件访问**不允许重叠** —— EC 不支持并发.
//!
//! 只在访问前 `sleep` 是**不够**的: 那样只约束了"起点间隔", 如果单次 IOCTL 本身
//! 耗时超过间隔, 两次访问仍然会重叠. 因此本模块提供 [`AccessThrottle::run`]:
//! **整个访问过程持锁**, 从"等待间隔"一直持到"访问返回".
//!
//! # 为什么是进程级单例
//!
//! 如果限速器是 `WindowsAcpiTransport` 的字段, 那么每 `new()` 一次就得到一份
//! 空状态 —— 每条 IPC 命令各建一个 transport 时, 每条命令的**第一次**访问都是
//! 零间隔, 等于绕过。因此真实硬件出口统一取 [`global`], 全进程共享同一份状态。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 默认最小访问间隔 = 20 ms (50 次/秒).
///
/// 这是 `docs/SPEC.md` §5.1 规定的默认安全速率.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(20);

/// 允许的最小间隔 = 10 ms (100 次/秒). 请求低于该值会被强制夹回.
///
/// 越过这条线就有让 EC 停摆的风险, 因此它不提供配置开关.
pub const HARD_MIN_INTERVAL: Duration = Duration::from_millis(10);

/// 进程级限速闸门 —— 真实硬件访问的唯一入口.
///
/// 首次调用时以 [`DEFAULT_MIN_INTERVAL`] 初始化, 之后全进程共用.
#[must_use]
pub fn global() -> &'static AccessThrottle {
    static GLOBAL: OnceLock<AccessThrottle> = OnceLock::new();
    GLOBAL.get_or_init(AccessThrottle::new)
}

/// 串行化 + 限速的访问闸门.
///
/// 语义: "任意两次访问之间至少间隔 `min_interval`, 且两次访问不重叠".
/// 间隔从**上一次访问结束**开始计时 (比从开始计时更保守).
#[derive(Debug)]
pub struct AccessThrottle {
    min_interval: Duration,
    last: Mutex<Option<Instant>>,
    /// 同时进入闸门的访问者数量 (峰值应恒为 1, 供测试断言).
    inside: Mutex<usize>,
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
            inside: Mutex::new(0),
        }
    }

    /// 实际生效的最小间隔.
    #[must_use]
    pub fn min_interval(&self) -> Duration {
        self.min_interval
    }

    /// 在限速 + 互斥闸门内执行一次真实硬件访问.
    ///
    /// **整个闭包执行期间持锁**: 因此另一个线程不可能在本次访问返回前进入,
    /// 这就同时保证了"不重叠"与"间隔达标". 闭包内不得再次调用 [`Self::run`]
    /// (会自我死锁) —— 单次 IOCTL 天然满足该约束.
    ///
    /// 锁中毒时继续工作: 宁可丢一次中毒标记, 也不要因为一次 panic 就失去限速保护.
    pub fn run<R>(&self, access: impl FnOnce() -> R) -> R {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < self.min_interval {
                std::thread::sleep(self.min_interval - elapsed);
            }
        }

        {
            let mut inside = self.inside.lock().unwrap_or_else(|e| e.into_inner());
            *inside += 1;
        }

        let result = access();

        {
            let mut inside = self.inside.lock().unwrap_or_else(|e| e.into_inner());
            *inside -= 1;
        }
        // 从"访问结束"开始计时下一次间隔
        *last = Some(Instant::now());

        result
    }

    /// 当前同时处于闸门内的访问者数量 (诊断/测试用).
    #[must_use]
    pub fn active(&self) -> usize {
        *self.inside.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn default_is_fifty_accesses_per_second() {
        assert_eq!(
            AccessThrottle::new().min_interval(),
            Duration::from_millis(20)
        );
        assert_eq!(global().min_interval(), DEFAULT_MIN_INTERVAL);
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
        let value = t.run(|| 42_u8);
        assert_eq!(value, 42, "run 必须原样返回闭包结果");
        assert!(
            start.elapsed() < HARD_MIN_INTERVAL,
            "首次访问不应等待, 实际等了 {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn enforces_spacing_between_accesses() {
        let t = AccessThrottle::with_min_interval(HARD_MIN_INTERVAL);
        t.run(|| ());
        let start = Instant::now();
        t.run(|| ());
        let gap = start.elapsed();
        assert!(
            gap >= HARD_MIN_INTERVAL - Duration::from_millis(2),
            "两次访问间隔 {gap:?} 小于硬下限 {HARD_MIN_INTERVAL:?}"
        );
    }

    /// 关键测试: 真正检测"访问重叠".
    ///
    /// 旧的 `wait()` 实现只 sleep 不持锁, 单次访问耗时超过间隔时两个访问会重叠,
    /// 而当时的测试只断言总耗时, 无法发现 —— 这条断言峰值并发恒为 1.
    #[test]
    fn run_never_allows_overlapping_access() {
        let t = Arc::new(AccessThrottle::with_min_interval(HARD_MIN_INTERVAL));
        let inside = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let accesses_per_thread = 5;

        let mut handles = Vec::new();
        for _ in 0..4 {
            let t = Arc::clone(&t);
            let inside = Arc::clone(&inside);
            let peak = Arc::clone(&peak);
            handles.push(std::thread::spawn(move || {
                for _ in 0..accesses_per_thread {
                    t.run(|| {
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        // 模拟一次"比限速间隔还慢"的 IOCTL
                        std::thread::sleep(HARD_MIN_INTERVAL * 2);
                        inside.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "出现并发访问 —— 闸门没有覆盖实际访问过程"
        );
        assert_eq!(t.active(), 0, "退出后不应残留访问者计数");
    }

    #[test]
    fn run_serializes_and_costs_at_least_one_interval_each() {
        let t = AccessThrottle::with_min_interval(HARD_MIN_INTERVAL);
        let start = Instant::now();
        for _ in 0..5 {
            t.run(|| std::thread::sleep(Duration::from_millis(1)));
        }
        // 5 次访问: 4 个间隔 + 5 ms 工作量
        let min_expected = HARD_MIN_INTERVAL * 4 + Duration::from_millis(5);
        assert!(
            start.elapsed() >= min_expected - Duration::from_millis(5),
            "5 次访问仅耗时 {:?}, 限速或串行化未生效",
            start.elapsed()
        );
    }

    #[test]
    fn global_throttle_is_shared_across_transports() {
        // 关键: 进程级单例 —— 两次取用必须是同一份状态
        let first = global() as *const AccessThrottle;
        let second = global() as *const AccessThrottle;
        assert_eq!(first, second, "global() 必须返回同一实例");
    }
}
