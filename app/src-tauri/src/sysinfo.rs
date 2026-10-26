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
    /// 当前电量 (%)；容量不可知时为 `None`，绝不伪造 0%.
    pub percent: Option<u32>,
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
    /// 内存已用 (MB)；查询失败时为 `None`.
    pub mem_used_mb: Option<u64>,
    /// 内存总量 (MB)；查询失败时为 `None`.
    pub mem_total_mb: Option<u64>,
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
    gpu_cache: Mutex<Option<GpuCacheEntry>>,
    slow_cache: Mutex<Option<SlowCacheEntry>>,
    device: Mutex<Option<DeviceInfo>>,
    slow_stale: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone)]
struct GpuCacheEntry {
    at: Instant,
    info: Option<GpuInfo>,
}

#[derive(Debug, Clone)]
struct SlowCacheEntry {
    at: Instant,
    data: SlowData,
}

#[derive(Debug, Clone, Default)]
struct SlowData {
    battery: Option<BatteryInfo>,
    disk: Option<DiskInfo>,
    device: Option<DeviceInfo>,
}

impl SysSampler {
    /// 最近一次慢速查询是否失败（前端据此标注“缓存数据”）.
    pub fn slow_is_stale(&self) -> bool {
        if self.slow_stale.load(std::sync::atomic::Ordering::Acquire) {
            return true;
        }
        self.slow_cache
            .lock()
            .ok()
            .and_then(|cache| {
                cache
                    .as_ref()
                    .map(|entry| entry.at.elapsed() >= SLOW_REFRESH)
            })
            .unwrap_or(false)
    }

    /// 采集一次（内部按需刷新缓存）.
    pub fn sample(&self) -> SysSnapshot {
        let (mem_total_mb, mem_used_mb, mem_used_pct) = memory().unwrap_or((0, 0, None));
        let slow = self.slow_data();
        let device = slow.device.clone().unwrap_or_default();

        SysSnapshot {
            cpu_util_pct: self.cpu_util(),
            mem_used_mb: (mem_used_mb > 0).then_some(mem_used_mb),
            mem_total_mb: (mem_total_mb > 0).then_some(mem_total_mb),
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
        if let Some(entry) = guard.as_ref() {
            if entry.at.elapsed() < GPU_REFRESH {
                return entry.info.clone();
            }
        }
        let fresh = query_gpu();
        *guard = Some(GpuCacheEntry {
            at: Instant::now(),
            info: fresh.clone(),
        });
        fresh
    }

    fn slow_data(&self) -> SlowData {
        let mut guard = match self.slow_cache.lock() {
            Ok(g) => g,
            Err(_) => return SlowData::default(),
        };
        if let Some(entry) = guard.as_ref() {
            if entry.at.elapsed() < SLOW_REFRESH {
                return entry.data.clone();
            }
        }
        let Some(fresh) = query_slow() else {
            self.slow_stale
                .store(true, std::sync::atomic::Ordering::Release);
            log::warn!("慢速系统查询失败，保留上次采样");
            return guard
                .as_ref()
                .map(|entry| entry.data.clone())
                .unwrap_or_default();
        };
        // 静态信息只取一次，避免每次都被空值覆盖
        let merged = SlowData {
            battery: fresh.battery,
            disk: fresh.disk,
            device: fresh
                .device
                .or_else(|| guard.as_ref().and_then(|entry| entry.data.device.clone())),
        };
        *guard = Some(SlowCacheEntry {
            at: Instant::now(),
            data: merged.clone(),
        });
        self.slow_stale
            .store(false, std::sync::atomic::Ordering::Release);
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
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    use windows_sys::Win32::System::Threading::OpenProcess;

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

    pub fn memory() -> Option<(u64, u64, Option<f32>)> {
        // SAFETY: MEMORYSTATUSEX 是 POD 结构体，全零是合法初值；
        // dwLength 必须由调用方填成结构体实际大小。
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: dwLength 已按结构体实际大小填好，指针指向栈上有效结构体。
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 {
            return None;
        }
        let total_mb = Some(status.ullTotalPhys / (1024 * 1024));
        let used_mb = Some((status.ullTotalPhys - status.ullAvailPhys) / (1024 * 1024));
        let pct = if status.ullTotalPhys > 0 {
            Some(status.dwMemoryLoad as f32)
        } else {
            None
        };
        Some((total_mb?, used_mb?, pct))
    }

    const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(15);
    const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

    const PROCESS_TERMINATE: u32 = 0x0001;

    struct JobHandle(HANDLE);

    const PROCESS_SET_QUOTA: u32 = 0x0100;

    impl JobHandle {
        fn new() -> Option<Self> {
            // SAFETY: 匿名 Job、默认安全属性。
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return None;
            }
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: unsafe { std::mem::zeroed() },
                IoInfo: unsafe { std::mem::zeroed() },
                ProcessMemoryLimit: 0,
                JobMemoryLimit: 0,
                PeakProcessMemoryUsed: 0,
                PeakJobMemoryUsed: 0,
            };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: info 为栈上有效结构体，长度一致。
            let ok = unsafe {
                SetInformationJobObject(
                    raw,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_mut(&mut info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            (ok != 0).then_some(Self(raw))
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // SAFETY: 本对象唯一持有 Job 句柄，Drop 只关闭一次。
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn attach_process_to_job(job: HANDLE, pid: u32) -> Option<()> {
        // SAFETY: pid 来自刚创建的子进程；请求分配与终止权限。
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            log::warn!("打开子进程句柄失败: {}", std::io::Error::last_os_error());
            return None;
        }
        // SAFETY: job 与 process 均为有效句柄。
        let ok = unsafe { AssignProcessToJobObject(job, process) };
        if ok == 0 {
            log::warn!("子进程加入 Job 失败: {}", std::io::Error::last_os_error());
        }
        let _ = unsafe { CloseHandle(process) };
        (ok != 0).then_some(())
    }

    fn terminate_job_tree(job: HANDLE) {
        // SAFETY: job 由调用线程持有；显式终止整组进程，释放继承 pipe 的写端。
        if unsafe { TerminateJobObject(job, 1) } == 0 {
            log::warn!("终止进程组失败: {}", std::io::Error::last_os_error());
        }
    }

    #[cfg(not(windows))]
    fn terminate_pid(_pid: u32) {}

    /// 并发排空一个输出管道；限制总大小，超限即失败。
    fn read_bounded<R: Read + Send + 'static>(
        mut reader: R,
        failed: std::sync::mpsc::Sender<()>,
    ) -> thread::JoinHandle<Option<Vec<u8>>> {
        thread::spawn(move || {
            let mut output = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => return Some(output),
                    Ok(count) => {
                        if output.len().saturating_add(count) > MAX_OUTPUT_BYTES {
                            let _ = failed.send(());
                            return None;
                        }
                        output.extend_from_slice(&chunk[..count]);
                    }
                    Err(_) => {
                        let _ = failed.send(());
                        return None;
                    }
                }
            }
        })
    }

    /// 执行只读外部命令，并设置硬超时。
    ///
    /// stdout/stderr 在进程运行期间并发排空，避免输出填满管道后卡死。
    /// 超时先终止进程再 wait，保证 pipe 的写端被释放。
    fn run_bounded(program: &str, args: &[&str]) -> Option<std::process::Output> {
        run_bounded_with_timeout(program, args, EXTERNAL_TIMEOUT)
    }

    pub(super) fn run_bounded_with_timeout(
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Option<std::process::Output> {
        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let job = JobHandle::new()?;
        attach_process_to_job(job.0, child.id())?;
        let deadline = Instant::now() + timeout;
        let (failed_sender, failure_receiver) = std::sync::mpsc::channel();
        let stdout = read_bounded(child.stdout.take()?, failed_sender.clone());
        let stderr = read_bounded(child.stderr.take()?, failed_sender.clone());

        let mut completed = false;
        while Instant::now() < deadline {
            if failure_receiver.try_recv().is_ok() {
                log::warn!("外部查询输出超过上限或读取失败: {program}");
                break;
            }
            match child.try_wait() {
                Ok(Some(_)) => {
                    completed = true;
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    log::warn!("查询子进程状态失败 {program}: {error}");
                    break;
                }
            }
        }
        if !completed {
            log::warn!("外部查询超时，终止整个进程组: {program}");
            terminate_job_tree(job.0);
        }
        let status = child.wait().ok()?;
        if !completed {
            return None;
        }
        let stdout = stdout.join().ok().flatten()?;
        let stderr = stderr.join().ok().flatten()?;
        Some(std::process::Output {
            status,
            stdout,
            stderr,
        })
    }

    /// `nvidia-smi` 的 CSV 行解析器。任一字段异常都返回 `None`，不以 0 填充。
    pub(super) fn parse_gpu_row(line: &str) -> Option<GpuInfo> {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() != 9 || f[0].is_empty() {
            return None;
        }
        Some(GpuInfo {
            name: f[0].to_owned(),
            util_pct: f[1].parse().ok()?,
            mem_util_pct: f[2].parse().ok()?,
            temp_c: f[3].parse().ok()?,
            clock_mhz: f[4].parse().ok()?,
            mem_clock_mhz: f[5].parse().ok()?,
            power_w: f[6].parse().ok()?,
            vram_used_mb: f[7].parse().ok()?,
            vram_total_mb: f[8].parse().ok()?,
        })
    }

    /// 用 `nvidia-smi` 取 GPU 实时数据（不经过 EC）.
    pub fn query_gpu() -> Option<GpuInfo> {
        let out = run_bounded(
            r"C:\Windows\System32\nvidia-smi.exe",
            &[
                "--query-gpu=name,utilization.gpu,utilization.memory,temperature.gpu,clocks.current.graphics,clocks.current.memory,power.draw,memory.used,memory.total",
                "--format=csv,noheader,nounits",
            ],
        )?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        parse_gpu_row(text.lines().next()?)
    }

    const PS: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

    /// 电池 + SSD + 静态信息，一次 PowerShell 取齐（约 300 ms，60 s 一次）.
    pub fn query_slow() -> Option<super::SlowData> {
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
$batteryTempC = if ($bt -and $bt.Temperature -gt 0) {
  $value = [double]$bt.Temperature / 10.0 - 273.15
  if ($value -ge -40 -and $value -le 150) { [math]::Round($value) } else { $null }
} else { $null }
[pscustomobject]@{
  battery = [pscustomobject]@{
    on_ac = [bool]$bs.PowerOnline; charging = [bool]$bs.Charging; discharging = [bool]$bs.Discharging
    remaining_mwh = [int]$bs.RemainingCapacity; full_mwh = [int]$fc.FullChargedCapacity
    design_mwh = [int]$sd.DesignedCapacity; voltage_mv = [int]$bs.Voltage
    cycles = if ($cc -and $cc.CycleCount -gt 0) { [int]$cc.CycleCount } else { $null }
    temp_c = $batteryTempC
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
        let out = run_bounded(PS, &["-NoProfile", "-NonInteractive", "-Command", script])?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Some(parse_slow(text.trim()))
    }

    pub(super) fn parse_slow(json: &str) -> super::SlowData {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            log::warn!("系统信息 JSON 解析失败: {}", &json[..json.len().min(160)]);
            return super::SlowData::default();
        };
        let u32_of = |v: &serde_json::Value, k: &str| -> Option<u32> {
            v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32)
        };

        let battery = value.get("battery").filter(|b| !b.is_null()).and_then(|b| {
            let info = BatteryInfo {
                on_ac: b.get("on_ac").and_then(serde_json::Value::as_bool)?,
                charging: b.get("charging").and_then(serde_json::Value::as_bool)?,
                discharging: b.get("discharging").and_then(serde_json::Value::as_bool)?,
                percent: None,
                remaining_mwh: u32_of(b, "remaining_mwh")?,
                full_mwh: u32_of(b, "full_mwh")?,
                design_mwh: u32_of(b, "design_mwh").unwrap_or(0),
                voltage_mv: u32_of(b, "voltage_mv")?,
                cycles: u32_of(b, "cycles"),
                temp_c: u32_of(b, "temp_c"),
                charge_limit_pct: None,
            };
            // 有 BatteryStatus 类并不等于有电池：虚拟机可能给全零容量。
            // 保留电源连接状态，但拒绝伪造成“0% 电池”。
            (info.full_mwh > 0 && info.voltage_mv > 0).then_some(info)
        });

        let disk = value.get("disk").filter(|d| !d.is_null()).and_then(|d| {
            // 空的 WMI 对象不代表磁盘存在；容量是缺失字段时的必要事实。
            let size_gb = d.get("size_gb").and_then(|x| x.as_u64())?;
            (size_gb > 0).then_some(DiskInfo {
                model: d
                    .get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
                size_gb,
                health: d
                    .get("health")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned(),
                temp_c: u32_of(d, "temp_c"),
                temp_max_c: u32_of(d, "temp_max_c"),
                wear_pct: u32_of(d, "wear_pct"),
            })
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
fn memory() -> Option<(u64, u64, Option<f32>)> {
    None
}
#[cfg(not(windows))]
fn query_gpu() -> Option<GpuInfo> {
    None
}
#[cfg(not(windows))]
fn query_slow() -> Option<SlowData> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_returns_plausible_numbers() {
        let Some((total, used, pct)) = memory() else {
            return; // 非 Windows 或调用失败：跳过
        };
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

    /// 解析器确定性测试：喂**固定 JSON**，断言解析结果。
    ///
    /// 教训（2026-10-25，CI 连红两次）：最初这里断言"磁盘型号非空、电池容量非 0"，
    /// 结果 GitHub 的 Windows 虚拟机既没有真电池（`BatteryStatus` 类存在但容量为 0）、
    /// `Get-PhysicalDisk` 的 FriendlyName 也可能为空 —— 测试红了但产品没坏。
    /// **解析逻辑要用 fixture 测；真机只做"不 panic"的冒烟。**
    #[test]
    #[cfg(windows)]
    fn parses_known_payload_exactly() {
        let json = r#"{"battery":{"on_ac":true,"charging":false,"discharging":false,"remaining_mwh":46740,"full_mwh":46740,"design_mwh":0,"voltage_mv":11986,"cycles":null,"temp_c":null},"disk":{"model":"YMTC PC300-1TB-B","size_gb":953,"health":"Healthy","temp_c":41,"temp_max_c":83,"wear_pct":0},"device":{"cpu":"12th Gen Intel(R) Core(TM) i9-12900HX","cpu_threads":24,"cpu_cores":16,"os":"Microsoft Windows 10 Pro"}}"#;
        let data = imp::parse_slow(json);

        let b = data.battery.expect("电池应被解析");
        assert!(b.on_ac && !b.charging && !b.discharging);
        assert_eq!(b.full_mwh, 46740);
        assert_eq!(b.remaining_mwh, 46740);
        assert_eq!(b.voltage_mv, 11986);
        assert_eq!(
            b.design_mwh, 0,
            "本机不提供设计容量 → 0，界面显示‘本机不提供’"
        );
        assert!(b.cycles.is_none(), "null 应解析为 None 而不是 0");

        let d = data.disk.expect("磁盘应被解析");
        assert_eq!(d.model, "YMTC PC300-1TB-B");
        assert_eq!(d.size_gb, 953);
        assert_eq!(d.temp_c, Some(41));
        assert_eq!(d.temp_max_c, Some(83));
        assert_eq!(d.wear_pct, Some(0));

        let dev = data.device.expect("静态信息应被解析");
        assert!(dev.cpu.contains("i9-12900HX"));
        assert_eq!(dev.cpu_threads, 24);
        assert_eq!(dev.cpu_cores, 16);
        assert!(dev.os.contains("Windows"));
    }

    #[test]
    #[cfg(windows)]
    fn absent_battery_stays_absent_even_when_class_exists() {
        let data = imp::parse_slow(
            r#"{"battery":{"on_ac":false,"charging":false,"discharging":false,"remaining_mwh":0,"full_mwh":0,"design_mwh":0,"voltage_mv":0}}"#,
        );
        assert!(
            data.battery.is_none(),
            "WMI 类存在但容量全 0 不代表有电池，不能伪造成 0% 电量"
        );
    }

    #[test]
    #[cfg(windows)]
    fn valid_disk_survives_missing_battery() {
        let data = imp::parse_slow(
            r#"{"battery":{"remaining_mwh":0,"full_mwh":0,"voltage_mv":0},"disk":{"model":"Virtual Disk","size_gb":64,"health":"Healthy","temp_c":null}}"#,
        );
        assert!(data.battery.is_none());
        assert_eq!(data.disk.expect("磁盘仍应保留").size_gb, 64);
    }

    #[test]
    #[cfg(windows)]
    fn invalid_gpu_row_is_rejected_not_zeroed() {
        assert!(imp::parse_gpu_row("GeForce,[N/A],[N/A],40,210,405,5.91,1288,8188").is_none());
    }

    #[test]
    #[cfg(windows)]
    fn battery_temperature_decikelvin_becomes_celsius() {
        let raw = 2982.0_f64;
        let celsius = raw / 10.0 - 273.15;
        assert!((-40.0..=150.0).contains(&celsius));
        assert_eq!(celsius.round() as i32, 25);

        let invalid = 0.0_f64 / 10.0 - 273.15;
        assert!(!(-40.0..=150.0).contains(&invalid));
    }

    #[test]
    #[cfg(windows)]
    fn valid_gpu_row_is_parsed() {
        let gpu = imp::parse_gpu_row("RTX 4060,4,22,40,210,405,5.91,1288,8188")
            .expect("有效 GPU 行应解析");
        assert_eq!(gpu.util_pct, 4);
        assert_eq!(gpu.temp_c, 40);
        assert_eq!(gpu.power_w, 5.91);
    }

    #[test]
    #[cfg(windows)]
    fn bounded_query_returns_for_normal_child() {
        let out =
            imp::run_bounded_with_timeout("cmd.exe", &["/C", "echo hello"], Duration::from_secs(5))
                .expect("正常子进程应返回");
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    #[cfg(windows)]
    fn bounded_query_kills_silent_hanging_child() {
        let start = std::time::Instant::now();
        let result = imp::run_bounded_with_timeout(
            "cmd.exe",
            &["/C", "ping 127.0.0.1 -n 8 >nul"],
            Duration::from_millis(300),
        );
        assert!(result.is_none(), "悬挂子进程必须按超时失败");
        assert!(start.elapsed() < Duration::from_secs(6), "不能永久等待");
    }

    #[test]
    #[cfg(windows)]
    fn bounded_query_rejects_excessive_output_without_deadlock() {
        let start = std::time::Instant::now();
        let result = imp::run_bounded_with_timeout(
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "1..70000 | ForEach-Object { '0123456789abcdef0123456789abcdef' }",
            ],
            Duration::from_secs(10),
        );
        assert!(
            result.is_none(),
            "超过输出上限应失败，不得无限等待或无限增长"
        );
        assert!(start.elapsed() < Duration::from_secs(15));
    }

    #[test]
    #[cfg(windows)]
    fn bounded_query_stops_pipe_filling_output() {
        let result = imp::run_bounded_with_timeout(
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "1..50 | ForEach-Object { '0123456789abcdef' }",
            ],
            // CI 冷启动下 powershell 首次运行可能 >5s（run 40351913217 偶发），
            // 放宽到 20s；防挂起语义由上方 6s 悬挂测试另行保证。
            Duration::from_secs(20),
        )
        .expect("持续输出但未超上限时应完成");
        assert!(!result.stdout.is_empty());
    }

    /// 真机冒烟：只保证「跑得通、不 panic」，不对具体硬件下断言。
    #[test]
    fn slow_query_runs_without_panicking() {
        let _ = query_slow();
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
