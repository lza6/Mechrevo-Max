//! 系统信息采集（**免驱**，全部只读）.
//!
//! # 目的
//!
//! 官方控制台只给 CPU/GPU 使用率与温度、内存占比、SSD 温度、风扇百分比 —— 而且都是三位数级别的精度。
//! 本模块在**不安装任何驱动**的前提下，尽量把同一批数据取全，并补上官方没给的：
//! GPU 功耗 / 显存占用 / 实时频率、电池满充容量与充放电状态、SSD 磨损与温度上限。
//!
//! # 为什么分三档刷新频率
//!
//! | 数据 | 手段 | 频率 | 理由 |
//! |---|---|---|---|
//! | CPU 使用率 / 内存 | `GetSystemTimes` / `GlobalMemoryStatusEx` | 每次轮询（2 s） | 纯系统调用，微秒级 |
//! | GPU | `nvidia-smi` | ≥3 s | 进程启动约 50 ms，太频繁没必要 |
//! | 电池 / SSD | PowerShell + WMI/CIM | ≥60 s | 进程启动约 300 ms，且这些量变化极慢 |
//!
//! **注意**：这些都不走 EC，因此不会和 EC 抢通道（见 `docs/protocol/ec-baseline.md` §4.1）。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// GPU 刷新的最小间隔.
const GPU_REFRESH: Duration = Duration::from_secs(3);
/// 电池/SSD 刷新的最小间隔.
const SLOW_REFRESH: Duration = Duration::from_secs(60);

/// GPU 信息（来自 `nvidia-smi`）.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuInfo {
    /// 型号.
    pub name: String,
    /// 核心占用率 (%).
    pub util_pct: u32,
    /// 显存占用率 (%).
    pub mem_util_pct: u32,
    /// 温度 (°C).
    pub temp_c: u32,
    /// 核心频率 (MHz).
    pub clock_mhz: u32,
    /// 显存频率 (MHz).
    pub mem_clock_mhz: u32,
    /// 功耗 (W).
    pub power_w: f32,
    /// 已用显存 (MiB).
    pub vram_used_mb: u32,
    /// 总显存 (MiB).
    pub vram_total_mb: u32,
}

/// 电池信息.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// 是否接着适配器.
    pub on_ac: bool,
    /// 是否正在充电.
    pub charging: bool,
    /// 是否正在放电.
    pub discharging: bool,
    /// 当前电量 (%).
    pub percent: u32,
    /// 剩余容量 (mWh).
    pub remaining_mwh: u32,
    /// 满充容量 (mWh).
    pub full_mwh: u32,
    /// 设计容量 (mWh)；本机 WMI 不提供时为 0.
    pub design_mwh: u32,
    /// 电压 (mV).
    pub voltage_mv: u32,
    /// 循环次数；本机不提供时为 `None`.
    pub cycles: Option<u32>,
    /// 电池温度 (°C)；本机不提供时为 `None`.
    pub temp_c: Option<u32>,
    /// EC 里的充电上限 (%)；读不到为 `None`.
    pub charge_limit_pct: Option<u8>,
}

/// 磁盘信息.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskInfo {
    /// 型号.
    pub model: String,
    /// 容量 (GB).
    pub size_gb: u64,
    /// 健康状态（如 `Healthy`）.
    pub health: String,
    /// 温度 (°C).
    pub temp_c: Option<u32>,
    /// 温度上限 (°C).
    pub temp_max_c: Option<u32>,
    /// 磨损百分比.
    pub wear_pct: Option<u32>,
}

/// 静态设备信息（启动时取一次）.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// CPU 型号.
    pub cpu: String,
    /// 逻辑处理器数.
    pub cpu_threads: u32,
    /// 物理核心数（读取失败时为 0）.
    pub cpu_cores: u32,
    /// 内存总量 (MB).
    pub mem_total_mb: u64,
    /// 操作系统版本.
    pub os: String,
}

/// 一次系统快照（EC 之外的部分）.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SysSnapshot {
    /// CPU 使用率 (%).
    pub cpu_util_pct: Option<f32>,
    /// 内存已用 (MB).
    pub mem_used_mb: u64,
    /// 内存总量 (MB).
    pub mem_total_mb: u64,
    /// 内存使用率 (%).
    pub mem_used_pct: Option<f32>,
    /// GPU.
    pub gpu: Option<GpuInfo>,
    /// 电池.
    pub battery: Option<BatteryInfo>,
    /// 磁盘.
    pub disk: Option<DiskInfo>,
    /// 静态信息.
    pub device: DeviceInfo,
}

/// 采集器：持有上次采样状态与缓存，避免每次轮询都拉起进程.
#[derive(Debug, Default)]
pub struct SysSampler {
    cpu_prev: Mutex<Option<(u64, u64, u64)>>, // idle, kernel, user
    gpu_cache: Mutex<Option<(Instant, GpuInfo)>>,
    slow_cache: Mutex<Option<(Instant, SlowData)>>,
    device: Mutex<Option<DeviceInfo>>,
}

#[derive(Debug, Clone, Default)]
struct SlowData {
    battery: Option<BatteryInfo>,
    disk: Option<DiskInfo>,
    device: Option<DeviceInfo>,
}

impl SysSampler {
    /// 采集一次（内部按需刷新缓存）.
    pub fn sample(&self) -> SysSnapshot {
        let (mem_total_mb, mem_used_mb, mem_used_pct) = memory();
        let slow = self.slow_data();
        let device = slow.device.clone().unwrap_or_default();

        SysSnapshot {
            cpu_util_pct: self.cpu_util(),
            mem_used_mb,
            mem_total_mb,
            mem_used_pct,
            gpu: self.gpu(),
            battery: slow.battery,
            disk: slow.disk,
            device,
        }
    }

    fn cpu_util(&self) -> Option<f32> {
        let (idle, kernel, user) = system_times()?;
        let mut guard = self.cpu_prev.lock().ok()?;
        let prev = *guard;
        *guard = Some((idle, kernel, user));
        let (pi, pk, pu) = prev?;

        let d_idle = idle.saturating_sub(pi) as f64;
        let d_total = (kernel.saturating_sub(pk) + user.saturating_sub(pu)) as f64;
        if d_total <= 0.0 {
            return None;
        }
        let busy = (d_total - d_idle).max(0.0);
        Some(((busy / d_total) * 100.0) as f32)
    }

    fn gpu(&self) -> Option<GpuInfo> {
        let mut guard = self.gpu_cache.lock().ok()?;
        if let Some((at, info)) = guard.as_ref() {
            if at.elapsed() < GPU_REFRESH {
                return Some(info.clone());
            }
        }
        if let Some(info) = query_gpu() {
            *guard = Some((Instant::now(), info.clone()));
            Some(info)
        } else {
            guard.as_ref().map(|(_, info)| info.clone())
        }
    }

    fn slow_data(&self) -> SlowData {
        let mut guard = match self.slow_cache.lock() {
            Ok(g) => g,
            Err(_) => return SlowData::default(),
        };
        if let Some((at, data)) = guard.as_ref() {
            if at.elapsed() < SLOW_REFRESH {
                return data.clone();
            }
        }
        let fresh = query_slow();
        // 静态信息只取一次，避免每次都被空值覆盖
        let merged = SlowData {
            battery: fresh.battery,
            disk: fresh.disk,
            device: fresh
                .device
                .or_else(|| guard.as_ref().and_then(|(_, d)| d.device.clone())),
        };
        *guard = Some((Instant::now(), merged.clone()));
        if let Ok(mut dev) = self.device.lock() {
            if dev.is_none() {
                *dev = merged.device.clone();
            }
        }
        merged
    }
}

// ---------------- 平台实现 ----------------

#[cfg(windows)]
mod imp {
    use super::{BatteryInfo, DeviceInfo, DiskInfo, GpuInfo};
    use std::process::Command;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    fn filetime_to_u64(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }

    pub fn system_times() -> Option<(u64, u64, u64)> {
        let mut idle = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        // SAFETY: 三个指针都指向本栈帧上的有效 FILETIME；API 只写这些结构。
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        Some((
            filetime_to_u64(idle),
            filetime_to_u64(kernel),
            filetime_to_u64(user),
        ))
    }

    pub fn memory() -> (u64, u64, Option<f32>) {
        // SAFETY: MEMORYSTATUSEX 是 POD 结构体，全零是合法初值；
        // dwLength 必须由调用方填成结构体实际大小。
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: dwLength 已按结构体实际大小填好，指针指向栈上有效结构体。
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 {
            return (0, 0, None);
        }
        let total_mb = status.ullTotalPhys / (1024 * 1024);
        let used_mb = (status.ullTotalPhys - status.ullAvailPhys) / (1024 * 1024);
        let pct = if status.ullTotalPhys > 0 {
            Some(status.dwMemoryLoad as f32)
        } else {
            None
        };
        (total_mb, used_mb, pct)
    }

    /// 用 `nvidia-smi` 取 GPU 实时数据（不经过 EC）.
    pub fn query_gpu() -> Option<GpuInfo> {
        let out = Command::new(r"C:\Windows\System32\nvidia-smi.exe")
            .args([
                "--query-gpu=name,utilization.gpu,utilization.memory,temperature.gpu,clocks.current.graphics,clocks.current.memory,power.draw,memory.used,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().next()?;
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() < 9 {
            return None;
        }
        Some(GpuInfo {
            name: f[0].to_owned(),
            util_pct: f[1].parse().unwrap_or(0),
            mem_util_pct: f[2].parse().unwrap_or(0),
            temp_c: f[3].parse().unwrap_or(0),
            clock_mhz: f[4].parse().unwrap_or(0),
            mem_clock_mhz: f[5].parse().unwrap_or(0),
            power_w: f[6].parse().unwrap_or(0.0),
            vram_used_mb: f[7].parse().unwrap_or(0),
            vram_total_mb: f[8].parse().unwrap_or(0),
        })
    }

    const PS: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

    /// 电池 + SSD + 静态信息，一次 PowerShell 取齐（约 300 ms，60 s 一次）.
    pub fn query_slow() -> super::SlowData {
        let script = r#"
$ErrorActionPreference='SilentlyContinue'
$fc = Get-CimInstance -Namespace root/WMI -ClassName BatteryFullChargedCapacity | Select-Object -First 1
$bs = Get-CimInstance -Namespace root/WMI -ClassName BatteryStatus | Select-Object -First 1
$cc = Get-CimInstance -Namespace root/WMI -ClassName BatteryCycleCount | Select-Object -First 1
$bt = Get-CimInstance -Namespace root/WMI -ClassName BatteryTemperature | Select-Object -First 1
$sd = Get-CimInstance -Namespace root/WMI -ClassName BatteryStaticData | Select-Object -First 1
$pd = Get-PhysicalDisk | Select-Object -First 1
$rc = if ($pd) { Get-StorageReliabilityCounter -PhysicalDisk $pd } else { $null }
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem | Select-Object -First 1
[pscustomobject]@{
  battery = [pscustomobject]@{
    on_ac = [bool]$bs.PowerOnline; charging = [bool]$bs.Charging; discharging = [bool]$bs.Discharging
    remaining_mwh = [int]$bs.RemainingCapacity; full_mwh = [int]$fc.FullChargedCapacity
    design_mwh = [int]$sd.DesignedCapacity; voltage_mv = [int]$bs.Voltage
    cycles = if ($cc -and $cc.CycleCount -gt 0) { [int]$cc.CycleCount } else { $null }
    temp_c = if ($bt -and $bt.Temperature -gt 0 -and $bt.Temperature -lt 1000) { [int]($bt.Temperature/10) } else { $null }
  }
  disk = if ($pd) { [pscustomobject]@{
    model = $pd.FriendlyName; size_gb = [int]($pd.Size/1GB); health = [string]$pd.HealthStatus
    temp_c = if ($rc.Temperature -gt 0) { [int]$rc.Temperature } else { $null }
    temp_max_c = if ($rc.TemperatureMax -gt 0) { [int]$rc.TemperatureMax } else { $null }
    wear_pct = if ($null -ne $rc.Wear) { [int]$rc.Wear } else { $null }
  } } else { $null }
  device = [pscustomobject]@{
    cpu = [string]$cpu.Name; cpu_threads = [int]$env:NUMBER_OF_PROCESSORS
    cpu_cores = [int]$cpu.NumberOfCores; os = [string]$os.Caption
  }
} | ConvertTo-Json -Depth 4 -Compress
"#;
        let out = match Command::new(PS)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return super::SlowData::default(),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        parse_slow(text.trim())
    }

    pub(super) fn parse_slow(json: &str) -> super::SlowData {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            log::warn!("系统信息 JSON 解析失败: {}", &json[..json.len().min(160)]);
            return super::SlowData::default();
        };
        let u32_of = |v: &serde_json::Value, k: &str| -> Option<u32> {
            v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32)
        };

        let battery = value
            .get("battery")
            .filter(|b| !b.is_null())
            .map(|b| BatteryInfo {
                on_ac: b
                    .get("on_ac")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                charging: b
                    .get("charging")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                discharging: b
                    .get("discharging")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                percent: 0,
                remaining_mwh: u32_of(b, "remaining_mwh").unwrap_or(0),
                full_mwh: u32_of(b, "full_mwh").unwrap_or(0),
                design_mwh: u32_of(b, "design_mwh").unwrap_or(0),
                voltage_mv: u32_of(b, "voltage_mv").unwrap_or(0),
                cycles: u32_of(b, "cycles"),
                temp_c: u32_of(b, "temp_c"),
                charge_limit_pct: None,
            });

        let disk = value
            .get("disk")
            .filter(|d| !d.is_null())
            .map(|d| DiskInfo {
                model: d
                    .get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned(),
                size_gb: d.get("size_gb").and_then(|x| x.as_u64()).unwrap_or(0),
                health: d
                    .get("health")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned(),
                temp_c: u32_of(d, "temp_c"),
                temp_max_c: u32_of(d, "temp_max_c"),
                wear_pct: u32_of(d, "wear_pct"),
            });

        let device = value
            .get("device")
            .filter(|d| !d.is_null())
            .map(|d| DeviceInfo {
                cpu: d
                    .get("cpu")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
                cpu_threads: u32_of(d, "cpu_threads").unwrap_or(0),
                cpu_cores: u32_of(d, "cpu_cores").unwrap_or(0),
                mem_total_mb: 0,
                os: d
                    .get("os")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
            });

        super::SlowData {
            battery,
            disk,
            device,
        }
    }
}

#[cfg(windows)]
use imp::{memory, query_gpu, query_slow, system_times};

#[cfg(not(windows))]
fn system_times() -> Option<(u64, u64, u64)> {
    None
}
#[cfg(not(windows))]
fn memory() -> (u64, u64, Option<f32>) {
    (0, 0, None)
}
#[cfg(not(windows))]
fn query_gpu() -> Option<GpuInfo> {
    None
}
#[cfg(not(windows))]
fn query_slow() -> SlowData {
    SlowData::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_returns_plausible_numbers() {
        let (total, used, pct) = memory();
        if total == 0 {
            return; // 非 Windows 或调用失败：跳过
        }
        assert!(total > 1024, "内存总量应 > 1 GB，实际 {total} MB");
        assert!(used <= total, "已用不得大于总量");
        assert!(pct.unwrap_or(0.0) <= 100.0);
    }

    #[test]
    fn gpu_query_is_well_formed_when_available() {
        if let Some(gpu) = query_gpu() {
            assert!(!gpu.name.is_empty(), "GPU 名称不应为空");
            assert!(gpu.util_pct <= 100, "占用率不可能 > 100%");
            assert!(gpu.temp_c < 120, "温度不合理: {}", gpu.temp_c);
            assert!(gpu.vram_total_mb > 0, "显存总量应 > 0");
        }
    }

    /// 只断言**环境无关的不变量**。
    ///
    /// 教训（2026-10-25，CI 上翻车）：这里原本断言"磁盘型号非空、CPU 型号非空"，
    /// 结果在 GitHub 的 Windows 虚拟机上 `Get-PhysicalDisk` 的 FriendlyName 可能是空串，
    /// 测试直接红 —— 而产品本身没有问题。**测试不该依赖某台机器的具体硬件事实**，
    /// 只该依赖"解析出来的结构是否自洽"。
    #[test]
    fn slow_query_produces_structurally_valid_data() {
        let data = query_slow();
        if let Some(b) = &data.battery {
            assert!(
                b.full_mwh > 0 || b.remaining_mwh > 0,
                "电池存在时容量字段不应全为 0"
            );
            assert!(b.voltage_mv < 30_000, "电压不合理: {} mV", b.voltage_mv);
        }
        if let Some(d) = &data.disk {
            assert!(d.size_gb > 0, "磁盘存在时容量应 > 0");
            if let Some(t) = d.temp_c {
                assert!(t < 120, "磁盘温度不合理: {t}");
            }
        }
        if let Some(dev) = &data.device {
            assert!(dev.cpu_threads > 0, "逻辑处理器数应 > 0");
        }
    }

    #[test]
    fn cpu_util_second_sample_is_bounded() {
        let s = SysSampler::default();
        let _ = s.sample();
        std::thread::sleep(Duration::from_millis(120));
        let snap = s.sample();
        if let Some(u) = snap.cpu_util_pct {
            assert!((0.0..=100.0).contains(&u), "CPU 使用率应在 0-100，实际 {u}");
        }
    }

    #[test]
    fn json_parser_rejects_garbage_without_panicking() {
        if cfg!(windows) {
            let data = imp::parse_slow("not json at all");
            assert!(data.battery.is_none() && data.disk.is_none());
        }
    }
}
