//! 机械革命控制台 —— Tauri 后端.
//!
//! 职责：把 `mechrevo-core` 的能力暴露给前端，并持有"最近一次改动"的状态，
//! 使用户随时可以一键撤销。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mechrevo_core::ec::{Ec, WindowsAcpiTransport};
use mechrevo_core::fancurve::FanCurveBlock;
use mechrevo_core::namespace::Mode as PowerMode;
use mechrevo_core::profile::Profile;
use mechrevo_core::{
    FanBoostChange, ModeChange, ModeController, OperatingMode, PowerController, PowerLimits,
    Sensors,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

mod logging;
mod profiles;
mod sysinfo;

/// 最近一次改动，用于一键撤销.
#[derive(Default)]
struct UndoStack {
    power: Option<mechrevo_core::ChangeRecord>,
    mode: Option<ModeChange>,
    fan_boost: Option<FanBoostChange>,
}

/// 全局应用状态.
#[derive(Default)]
struct AppState {
    /// 串行化所有 EC 访问.
    ///
    /// EC 硬件不支持并发访问（`docs/SPEC.md` §7）。此前的实现只保护了撤销栈，
    /// 没有保护 EC 本身：2 秒轮询与用户写入会各自打开句柄同时读写，
    /// 可能读到"写了一半"的状态。现在所有命令都必须先取这把锁。
    ec: Mutex<()>,
    /// 最近一次改动，用于一键撤销.
    undo: Mutex<UndoStack>,
    /// 系统信息采样器（CPU/内存/GPU/电池/SSD，全部免驱只读）.
    sampler: sysinfo::SysSampler,
    /// 最近一次失败的原因 —— 让失败"留在界面上"而不是两秒后消失.
    ///
    /// 此前失败只写进状态栏，下一次轮询就把文案冲掉，用户回头看不出发生过什么。
    /// 这里在成功操作时清空、失败时保留，由 `get_status` 回放给前端（回放而非一次性
    /// `emit`：事件在监听未注册或窗口已销毁时会直接丢失，Tauri 不做重放）。
    last_error: Mutex<Option<OperationError>>,
    /// 每条命令的调用代次；旧请求的迟到结果不得覆盖新请求.
    operation_generation: std::sync::atomic::AtomicU64,
    /// 串行化配置的读-改-写，避免并发保存/删除覆盖彼此的更新.
    profiles: Mutex<()>,
}

/// 一条带调用代次的错误：用于阻止旧请求的迟到结果清除新失败.
#[derive(Clone)]
struct OperationError {
    command: &'static str,
    generation: u64,
    message: String,
}

/// 读出当前失败原因（供 `get_status` 回放）.
fn last_error(state: &AppState) -> Option<String> {
    state.last_error.lock().ok().and_then(|slot| {
        slot.as_ref()
            .map(|error| format!("[{}] {}", error.command, error.message))
    })
}

/// 分配一次调用代次；每个异步命令在开始时获取，结果回来时携带它.
fn next_generation(state: &AppState) -> u64 {
    state
        .operation_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1
}

fn lock_profiles(state: &AppState) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    state
        .profiles
        .lock()
        .map_err(|_| "配置状态异常，请重启程序后再修改配置".to_owned())
}

/// 统一处理命令结果：仅允许**同代调用**更新自己的错误状态。
fn note_result(
    state: &AppState,
    command: &'static str,
    generation: u64,
    result: &Result<String, String>,
) {
    let Ok(mut slot) = state.last_error.lock() else {
        return;
    };
    match result {
        Ok(_) => {
            if slot
                .as_ref()
                .is_some_and(|error| error.command == command && error.generation == generation)
            {
                *slot = None;
            }
        }
        Err(message) => {
            // 不允许旧请求的迟到失败覆盖更新的结果。
            let should_record = slot
                .as_ref()
                .is_none_or(|error| error.command != command || error.generation < generation);
            if should_record {
                *slot = Some(OperationError {
                    command,
                    generation,
                    message: message.clone(),
                });
                log::warn!("[{command} #{generation}] 操作失败: {message}");
            }
        }
    }
}

/// 取 EC 访问锁 —— 锁中毒时给出可行动的提示，而不是 panic.
fn lock_ec(state: &AppState) -> Result<std::sync::MutexGuard<'_, ()>, (&'static str, String)> {
    state.ec.lock().map_err(|_| {
        (
            "restart",
            "内部状态异常（上一次操作异常终止）。请重启本程序，EC 通道会自动恢复。".to_owned(),
        )
    })
}

/// 取消栈当前状态；锁中毒时返回"都不可撤销"而不是 panic.
fn undo_flags(state: &AppState) -> (bool, bool) {
    match state.undo.lock() {
        Ok(undo) => (undo.power.is_some(), undo.mode.is_some()),
        Err(_) => (false, false),
    }
}

/// EC 访问状态.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EcStatus {
    /// 可用.
    Ready,
    /// 不可用，附带修复提示.
    Unavailable {
        /// 面向用户的说明.
        message: String,
        /// 提示类型: `driver` = 驱动/设备未就绪; `elevation` = 权限不足;
        /// `restart` = 程序内部状态异常，需重启。
        hint: &'static str,
    },
}

/// 传给前端的完整状态快照.
#[derive(Serialize)]
struct StatusPayload {
    ec: EcStatus,
    project_id: Option<u8>,
    power_source: Option<u8>,
    is_ac_powered: Option<bool>,
    cpu_temp_c: Option<u8>,
    gpu_temp_c: Option<u8>,
    cpu_fan_rpm: Option<u16>,
    gpu_fan_rpm: Option<u16>,
    mode: Option<&'static str>,
    mode_raw: Option<(u8, u8)>,
    gaming_power: Option<PowerLimitsDto>,
    office_power: Option<PowerLimitsDto>,
    /// 强冷（`0x0751` bit6）当前是否开启.
    fan_boost: Option<bool>,
    /// 固件侧基线功耗（只读，与 EC 默认功耗寄存器不是一回事）.
    firmware_power: Option<FirmwarePowerDto>,
    can_undo_power: bool,
    can_undo_mode: bool,
    can_undo_fan_boost: bool,
    /// 系统信息（CPU/内存/GPU/电池/SSD/静态设备信息）.
    system: sysinfo::SysSnapshot,
    /// 滑块范围，与 `core::control` 的写前校验共用同一事实源.
    limits_meta: LimitsMeta,
    /// 慢速系统查询是否失败（页面应标为缓存数据，而非假装实时）.
    system_slow_stale: bool,
    /// 应用版本.
    app_version: &'static str,
    /// 日志目录，供用户排障.
    log_dir: String,
    /// 当前轮次 EC 部分读取警告（不会跨轮次隐藏新错误）.
    read_warnings: Vec<String>,
    /// 上一次失败的原因（成功后清空）.
    last_error: Option<String>,
}

/// 功耗三元组的传输结构.
#[derive(Serialize, Clone, Copy)]
struct PowerLimitsDto {
    pl1: u8,
    pl2: u8,
    pl4: u8,
}

impl From<PowerLimits> for PowerLimitsDto {
    fn from(p: PowerLimits) -> Self {
        Self {
            pl1: p.pl1,
            pl2: p.pl2,
            pl4: p.pl4,
        }
    }
}

/// 构造一个 EC 访问器.
fn open_ec() -> Result<Ec<WindowsAcpiTransport>, mechrevo_core::Error> {
    Ok(Ec::new(WindowsAcpiTransport::new()?))
}

/// 把底层错误分类成用户可行动的提示.
fn classify(err: &mechrevo_core::Error) -> (&'static str, String) {
    match err {
        mechrevo_core::Error::DeviceNotReady { .. } => (
            "driver",
            "EC 驱动未就绪。请先执行下方命令，然后重启本程序。".to_owned(),
        ),
        mechrevo_core::Error::DeviceOpen { source, .. } if source.raw_os_error() == Some(5) => (
            "elevation",
            "权限不足 —— 访问 EC 需要管理员权限。".to_owned(),
        ),
        other => ("driver", other.to_string()),
    }
}

/// 滑块范围的单一事实源（来自 core 的写前校验常量）.
#[derive(Serialize)]
struct LimitsMeta {
    pl1: (u8, u8),
    pl2: (u8, u8),
    pl4: (u8, u8),
}

fn limits_meta() -> LimitsMeta {
    LimitsMeta {
        pl1: mechrevo_core::control::PL1_RANGE,
        pl2: mechrevo_core::control::PL2_RANGE,
        pl4: mechrevo_core::control::PL4_RANGE,
    }
}

/// 构造一个"EC 不可用"的状态载荷.
fn unavailable(
    err: &mechrevo_core::Error,
    can_undo_power: bool,
    can_undo_mode: bool,
) -> StatusPayload {
    let (hint, message) = classify(err);
    unavailable_with(hint, message, can_undo_power, can_undo_mode)
}

/// 电池百分比：容量可信时才计算；缺字段一律保持 None。
fn battery_percent(battery: &sysinfo::BatteryInfo) -> Option<u32> {
    battery
        .remaining_mwh
        .checked_mul(100)
        .and_then(|value| value.checked_div(battery.full_mwh))
        .map(|value| value.min(100))
}

/// 用已确定的提示类型构造"EC 不可用"载荷.
fn unavailable_with(
    hint: &'static str,
    message: String,
    can_undo_power: bool,
    can_undo_mode: bool,
) -> StatusPayload {
    StatusPayload {
        ec: EcStatus::Unavailable { message, hint },
        project_id: None,
        power_source: None,
        is_ac_powered: None,
        cpu_temp_c: None,
        gpu_temp_c: None,
        cpu_fan_rpm: None,
        gpu_fan_rpm: None,
        mode: None,
        mode_raw: None,
        gaming_power: None,
        office_power: None,
        fan_boost: None,
        firmware_power: None,
        can_undo_power,
        can_undo_mode,
        can_undo_fan_boost: false,
        read_warnings: Vec::new(),
        system: sysinfo::SysSnapshot::default(),
        limits_meta: limits_meta(),
        system_slow_stale: false,
        app_version: env!("CARGO_PKG_VERSION"),
        log_dir: logging::data_dir().join("logs").display().to_string(),
        last_error: None,
    }
}

/// 读取当前全部状态（同步业务体，跑在 blocking 线程池上）.
///
/// EC 不可用时也返回结构化结果，不抛错，让前端能渲染可行动的提示.
fn status_snapshot(state: &AppState) -> StatusPayload {
    // 外部查询绝不放进 EC 互斥：它可能超时，不能让 EC 驱动路径跟着等待。
    let mut system = state.sampler.sample();

    // 串行化：轮询与用户操作不会同时进硬件
    let _guard = match lock_ec(state) {
        Ok(guard) => guard,
        Err((hint, message)) => {
            let (can_undo_power, can_undo_mode) = undo_flags(state);
            let mut payload = unavailable_with(hint, message, can_undo_power, can_undo_mode);
            payload.system = system;
            return payload;
        }
    };
    let (can_undo_power, can_undo_mode) = undo_flags(state);
    let can_undo_fan_boost = state
        .undo
        .lock()
        .map(|u| u.fan_boost.is_some())
        .unwrap_or(false);

    let ec = match open_ec() {
        Ok(ec) => ec,
        Err(e) => {
            let mut payload = unavailable(&e, can_undo_power, can_undo_mode);
            payload.system = system;
            return payload;
        }
    };

    // 设备对象可能存在但打不开（典型是权限不足），这里用一次真实读取来区分
    let mode_ctl = ModeController::new(&ec);
    let power_ctl = PowerController::new(&ec);
    let sensors = Sensors::new(&ec);

    let snap = match sensors.snapshot() {
        Ok(s) => s,
        Err(e) => {
            let mut payload = unavailable(&e, can_undo_power, can_undo_mode);
            payload.system = system;
            return payload;
        }
    };

    // 档位字段只读一次：`current()` 与 `raw_fields()` 都会读 0x751，
    // 合并后每轮轮询少一次 EC 访问（20 ms 限速下等于少 20 ms 延迟）。
    // 部分字段失败时必须可见；有效字段继续展示，不能把整页判为不可用。
    let mut read_warnings = Vec::new();

    // EC 里的充电上限（0x7B9，低 7 位为百分比）——只读；失败也要留痕。
    if let Some(b) = system.battery.as_mut() {
        if b.full_mwh > 0 {
            b.percent = battery_percent(b);
        }
        match ec.read(0x07B9) {
            Ok(raw) => {
                let pct = raw & 0x7F;
                if (1..=100).contains(&pct) {
                    b.charge_limit_pct = Some(pct);
                }
            }
            Err(error) => read_warnings.push(format!("充电上限读取失败: {error}")),
        }
    }

    let fan_boost = match mode_ctl.fan_boost() {
        Ok(value) => Some(value),
        Err(error) => {
            read_warnings.push(format!("强冷位读取失败: {error}"));
            None
        }
    };
    let firmware_power = match power_ctl.read_firmware_baseline() {
        Ok(value) => Some(FirmwarePowerDto::from(value)),
        Err(error) => {
            read_warnings.push(format!("固件功耗基线读取失败: {error}"));
            None
        }
    };
    let raw = match mode_ctl.raw_fields() {
        Ok(value) => Some(value),
        Err(error) => {
            read_warnings.push(format!("档位字段读取失败: {error}"));
            None
        }
    };
    let mode_key = raw
        .and_then(|(field1, _)| OperatingMode::from_field1(field1))
        .map(OperatingMode::key);

    StatusPayload {
        ec: EcStatus::Ready,
        project_id: Some(snap.project_id),
        power_source: Some(snap.power_source),
        is_ac_powered: Some(snap.is_ac_powered()),
        cpu_temp_c: Some(snap.cpu_temp_c),
        gpu_temp_c: Some(snap.gpu_temp_c),
        cpu_fan_rpm: Some(snap.cpu_fan_rpm),
        gpu_fan_rpm: Some(snap.gpu_fan_rpm),
        mode: mode_key,
        mode_raw: raw,
        gaming_power: match power_ctl.read_defaults(PowerMode::Gaming) {
            Ok(value) => Some(PowerLimitsDto::from(value)),
            Err(error) => {
                read_warnings.push(format!("游戏默认功耗读取失败: {error}"));
                None
            }
        },
        office_power: match power_ctl.read_defaults(PowerMode::Office) {
            Ok(value) => Some(PowerLimitsDto::from(value)),
            Err(error) => {
                read_warnings.push(format!("办公默认功耗读取失败: {error}"));
                None
            }
        },
        fan_boost,
        firmware_power,
        read_warnings,
        system,
        limits_meta: limits_meta(),
        system_slow_stale: state.sampler.slow_is_stale(),
        app_version: env!("CARGO_PKG_VERSION"),
        log_dir: logging::data_dir().join("logs").display().to_string(),
        can_undo_power,
        can_undo_mode,
        can_undo_fan_boost,
        last_error: last_error(state),
    }
}

/// 以管理员身份重新启动本程序（同步业务体；当前进程随即退出）.
///
/// 用 PowerShell 的 `Start-Process -Verb RunAs` 触发 UAC，
/// 避免依赖构建期嵌入的 manifest（本机 RC.EXE 在中文路径下会失败）.
fn relaunch_elevated_now() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法获取自身路径: {e}"))?;
    let exe_str = exe.to_string_lossy().replace('\'', "''");

    let script = format!("Start-Process -FilePath '{exe_str}' -Verb RunAs");
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .status()
        .map_err(|e| format!("调用 PowerShell 失败: {e}"))?;

    if status.success() {
        // 提权实例已启动，退出当前非提权实例
        std::process::exit(0);
    }
    Err("提权启动被取消或失败".to_owned())
}

/// 切换运行档位（同步业务体）.
fn switch_mode(state: &AppState, target: OperatingMode) -> Result<String, String> {
    // 持锁覆盖"打开 EC → 切换 → 记入撤销栈"的整个过程
    let _guard = lock_ec(state).map_err(|(_, message)| message)?;

    let ec = open_ec().map_err(|e| e.to_string())?;
    let ctl = ModeController::new(&ec);
    let change = ctl.switch(target).map_err(|e| e.to_string())?;
    let name = target.display_name().to_owned();

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.mode = Some(change);
    Ok(format!("已切换到{name}档"))
}

/// 设置某模式的默认功耗（同步业务体）.
fn apply_power_limits(
    state: &AppState,
    target: PowerMode,
    limits: PowerLimits,
) -> Result<String, String> {
    let (pl1, pl2, pl4) = (limits.pl1, limits.pl2, limits.pl4);

    let _guard = lock_ec(state).map_err(|(_, message)| message)?;

    let ec = open_ec().map_err(|e| e.to_string())?;
    let ctl = PowerController::new(&ec);
    let record = ctl
        .apply_defaults(target, &limits)
        .map_err(|e| e.to_string())?;

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.power = Some(record);
    Ok(format!(
        "{}功耗已设为 PL1={} PL2={} PL4={}",
        target.display_name(),
        pl1,
        pl2,
        pl4
    ))
}

/// 撤销最近一次功耗改动.
///
/// 只有**真正撤销成功**才把记录出栈：失败时记录保留，用户可以重试
/// （此前是 `take()` 后直接撤销，失败即永久丢失回滚点）。
fn undo_power_now(state: &AppState) -> Result<String, String> {
    // 先判断有没有记录：没有就直接给准确文案，不必先打开设备
    let record = {
        let undo = state
            .undo
            .lock()
            .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
        undo.power.clone()
    };
    let Some(record) = record else {
        return Err("没有可撤销的功耗改动".into());
    };

    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;

    let restored = record
        .undo(&ec)
        .map_err(|e| format!("撤销失败（回滚点已保留，可重试）: {e}"))?;

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.power = None;
    Ok(format!("已还原 {restored} 个寄存器"))
}

/// 撤销最近一次档位切换.
fn undo_mode_now(state: &AppState) -> Result<String, String> {
    let change = {
        let undo = state
            .undo
            .lock()
            .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
        undo.mode.clone()
    };
    let Some(change) = change else {
        return Err("没有可撤销的档位切换".into());
    };

    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;

    let restored = change
        .undo(&ec)
        .map_err(|e| format!("撤销失败（回滚点已保留，可重试）: {e}"))?;

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.mode = None;
    Ok(format!("已还原 {restored} 个寄存器"))
}

/// 一次寄存器读取的结果（供“寄存器浏览器”使用）.
#[derive(Serialize, Debug)]
struct RegReading {
    addr: u16,
    value: u8,
}

/// 寄存器浏览器一次最多读多少地址（防止界面被刷爆、也避免长占 EC）.
const MAX_REGISTER_QUERY: usize = 64;

/// 开关强冷（同步业务体）.
fn set_fan_boost_now(state: &AppState, enabled: bool) -> Result<String, String> {
    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;
    let ctl = ModeController::new(&ec);
    let change = ctl.set_fan_boost(enabled).map_err(|e| e.to_string())?;

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.fan_boost = Some(change);
    Ok(if enabled {
        "强冷已开启".to_owned()
    } else {
        "强冷已关闭".to_owned()
    })
}

/// 撤销强冷开关（同步业务体）.
fn undo_fan_boost_now(state: &AppState) -> Result<String, String> {
    let change = {
        let undo = state
            .undo
            .lock()
            .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
        undo.fan_boost.clone()
    };
    let Some(change) = change else {
        return Err("没有可撤销的强冷操作".into());
    };

    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;
    let restored = change
        .undo(&ec)
        .map_err(|e| format!("撤销失败（回滚点已保留，可重试）: {e}"))?;

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.fan_boost = None;
    Ok(format!("已还原 {restored} 个寄存器"))
}

/// 只读若干寄存器（同步业务体）.
fn read_registers_now(state: &AppState, addrs: &[u16]) -> Result<Vec<RegReading>, String> {
    if addrs.is_empty() {
        return Err("请先填写要读取的地址".into());
    }
    if addrs.len() > MAX_REGISTER_QUERY {
        return Err(format!("一次最多读取 {MAX_REGISTER_QUERY} 个地址"));
    }
    if let Some(bad) = addrs
        .iter()
        .copied()
        .find(|a| *a > mechrevo_core::ec::EC_MAX_ADDR)
    {
        return Err(format!(
            "地址 0x{bad:04X} 超出 EC 范围 0x0000-0x{:04X}",
            mechrevo_core::ec::EC_MAX_ADDR
        ));
    }

    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(addrs.len());
    for &addr in addrs {
        let value = ec
            .read(addr)
            .map_err(|e| format!("读 0x{addr:04X} 失败: {e}"))?;
        out.push(RegReading { addr, value });
    }
    Ok(out)
}

/// 应用一条自定义配置（同步业务体）：功耗墙 +（可选）强冷.
///
/// 自定义配置统一写到 Gaming 默认功耗寄存器（`0x730`-`0x732`）——本机只有这一组
/// 「默认功耗」可写；如果以后确认了别的模式也有独立寄存器，再扩展。
fn apply_profile_now(state: &AppState, name: &str) -> Result<String, String> {
    let dir = logging::data_dir();
    let list = match profiles::load_profiles(&dir) {
        profiles::ProfileLoad::Ready(list) => list,
        profiles::ProfileLoad::NeedsAttention { message, backup: _ } => {
            return Err(format!("配置需要先处理，未应用任何改动：{message}"));
        }
    };
    let profile = list
        .iter()
        .find(|p| p.name == name)
        .cloned()
        .ok_or_else(|| format!("找不到配置「{name}」"))?;
    profile.validate().map_err(|e| e.to_string())?;

    let _profiles_guard = lock_profiles(state)?;
    let limits =
        PowerLimits::new(profile.pl1, profile.pl2, profile.pl4).map_err(|e| e.to_string())?;
    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;

    // 1) 功耗墙（内部已 snapshot + 逐项校验 + 失败回滚）
    let power = PowerController::new(&ec);
    let record = power
        .apply_defaults(PowerMode::Gaming, &limits)
        .map_err(|e| format!("写入功耗墙失败: {e}"))?;

    // 2) 可选强冷；失败就把功耗墙也回滚，不留半套状态
    let fan_change = if profile.fan_boost {
        match ModeController::new(&ec).set_fan_boost(true) {
            Ok(change) => Some(change),
            Err(e) => {
                let _ = record.undo(&ec);
                return Err(format!("开启强冷失败，已回滚功耗墙: {e}"));
            }
        }
    } else {
        None
    };

    let mut undo = state
        .undo
        .lock()
        .map_err(|_| "撤销栈异常，请重启程序".to_owned())?;
    undo.power = Some(record);
    undo.fan_boost = fan_change;
    Ok(format!(
        "已应用配置「{}」：{}",
        profile.name,
        profile.summary()
    ))
}

/// 风扇曲线读取结果（只读；含"候选配对"，不是协议结论）.
#[derive(Serialize)]
struct FanCurvesDto {
    blocks: Vec<FanCurveBlock>,
    /// 候选配对：曲线 A/B 的 (温度, 占空比%) —— 只是展示用的猜测.
    curves: Vec<CurveSeries>,
    note: String,
}

#[derive(Serialize)]
struct CurveSeries {
    label: String,
    temp_block: u16,
    duty_block: u16,
    points: Vec<CurvePoint>,
}

#[derive(Serialize)]
struct CurvePoint {
    temp_c: u8,
    duty_pct: u8,
}

/// 固件基线功耗（只读，与"EC 默认功耗寄存器"并排展示）.
#[derive(Serialize)]
struct FirmwarePowerDto {
    pl1: u8,
    pl2: u8,
    pl4: u8,
    extra: u8,
}

impl From<mechrevo_core::FirmwareBaseline> for FirmwarePowerDto {
    fn from(b: mechrevo_core::FirmwareBaseline) -> Self {
        Self {
            pl1: b.pl1,
            pl2: b.pl2,
            pl4: b.pl4,
            extra: b.extra,
        }
    }
}

/// 读风扇曲线（同步业务体，只读）.
fn read_fan_curves_now(state: &AppState) -> Result<FanCurvesDto, String> {
    let _guard = lock_ec(state).map_err(|(_, message)| message)?;
    let ec = open_ec().map_err(|e| e.to_string())?;
    let blocks = mechrevo_core::fancurve::read_blocks(&ec).map_err(|e| e.to_string())?;

    // 候选配对：温度块 0/3 ↔ 占空比块 2/5（本机实测的排布；未与官方表交叉核对）
    let mut curves = Vec::new();
    for (label, ti, di) in [("曲线 A（候选）", 0usize, 2usize), ("曲线 B（候选）", 3, 5)]
    {
        let (Some(temp), Some(duty)) = (blocks.get(ti), blocks.get(di)) else {
            continue;
        };
        let points = mechrevo_core::fancurve::pair_points(&temp.values, &duty.values)
            .into_iter()
            .map(|(t, d)| CurvePoint {
                temp_c: t,
                duty_pct: d,
            })
            .collect();
        curves.push(CurveSeries {
            label: label.to_owned(),
            temp_block: temp.base,
            duty_block: duty.base,
            points,
        });
    }

    Ok(FanCurvesDto {
        blocks,
        curves,
        note: "只读展示。官方 profile 只有表名（如 M1T1），真正的曲线数值不在本机备份里，               因此『温度块 ↔ 占空比块』的配对未经交叉核对；本程序不会写这些表。"
            .to_owned(),
    })
}

// ============================================================
// IPC 命令层
//
// 为什么全部是 `async fn` + `spawn_blocking`：
// Tauri v2 的 `#[tauri::command]` **默认在 WebView2 的 UI（主）线程上同步执行**
// （`tauri-macros` 的 wrapper 默认 `ExecutionContext::Blocking`，其阻塞分支不 spawn；
// 官方 doc 原文："Runs the command on the async runtime instead of blocking the main thread …
// this option is only needed for synchronous functions that should not run on the main thread"）。
// 而 `get_status` 一轮约 14 次 IOCTL、受"两次访问不重叠"闸门约束 ≈ 280 ms；
// 留在 UI 线程等于每 2 秒卡住窗口消息循环 280 ms（拖动/悬停/点击都会迟滞）。
// 因此：命令本体只做参数解析，真正的 EC 工作交给 blocking 线程池。
// 命令名与参数名保持不变，`ipc_contract.rs` 与前端 `IPC` 表无需改动。
// ============================================================

/// 共享状态的命令参数类型（`Arc` 是为了能搬进 `'static` 的 blocking 任务）。
type SharedState<'a> = State<'a, Arc<AppState>>;

/// 把后台任务失败转成用户可读文案.
fn join_error(e: tauri::Error) -> String {
    let message = format!("内部任务异常（{e}），请重启本程序");
    log::error!("{message}");
    message
}

/// 读取当前全部状态.
///
/// 返回类型是 `Result`，但**永远返回 `Ok`**：EC 不可用属于正常业务状态，
/// 会用结构化载荷（`ec.kind = "unavailable"` + `hint`）表达，不让前端收到
/// rejection。改成 `Result` 只是因为"含引用的 async 命令必须返回 `Result`"
/// （Tauri v2 的编译期约束），**前端看到的载荷形状与 v0.1.2 完全一致**。
#[tauri::command]
async fn get_status(state: SharedState<'_>) -> Result<StatusPayload, String> {
    let worker_state = Arc::clone(state.inner());
    let fallback_state = Arc::clone(state.inner());
    let payload =
        match tauri::async_runtime::spawn_blocking(move || status_snapshot(&worker_state)).await {
            Ok(payload) => payload,
            Err(e) => {
                let (can_undo_power, can_undo_mode) = undo_flags(&fallback_state);
                unavailable_with("restart", join_error(e), can_undo_power, can_undo_mode)
            }
        };
    Ok(payload)
}

/// 切换运行档位.
#[tauri::command]
async fn set_mode(state: SharedState<'_>, mode: String) -> Result<String, String> {
    let target = OperatingMode::from_key(&mode).ok_or_else(|| format!("未知档位: {mode}"))?;
    let worker_state = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || switch_mode(&worker_state, target))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "set_mode", generation, &result);
    result
}

/// 设置某模式的默认功耗.
#[tauri::command]
async fn set_power_limits(
    state: SharedState<'_>,
    mode: String,
    pl1: u8,
    pl2: u8,
    pl4: u8,
) -> Result<String, String> {
    let target = match mode.as_str() {
        "gaming" => PowerMode::Gaming,
        "office" => PowerMode::Office,
        other => return Err(format!("未知模式: {other}")),
    };
    let limits = PowerLimits::new(pl1, pl2, pl4).map_err(|e| e.to_string())?;

    let worker_state = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || {
        apply_power_limits(&worker_state, target, limits)
    })
    .await
    .map_err(join_error)?;
    note_result(&recorder, "set_power_limits", generation, &result);
    result
}

/// 撤销最近一次功耗改动.
#[tauri::command]
async fn undo_power(state: SharedState<'_>) -> Result<String, String> {
    let worker_state = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || undo_power_now(&worker_state))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "undo_power", generation, &result);
    result
}

/// 撤销最近一次档位切换.
#[tauri::command]
async fn undo_mode(state: SharedState<'_>) -> Result<String, String> {
    let worker_state = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || undo_mode_now(&worker_state))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "undo_mode", generation, &result);
    result
}

/// 以管理员身份重新启动本程序（当前进程随即退出）.
#[tauri::command]
async fn relaunch_elevated() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(relaunch_elevated_now)
        .await
        .map_err(join_error)?
}

/// 开关强冷.
#[tauri::command]
async fn set_fan_boost(state: SharedState<'_>, enabled: bool) -> Result<String, String> {
    let worker = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || set_fan_boost_now(&worker, enabled))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "set_fan_boost", generation, &result);
    result
}

/// 撤销强冷开关.
#[tauri::command]
async fn undo_fan_boost(state: SharedState<'_>) -> Result<String, String> {
    let worker = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || undo_fan_boost_now(&worker))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "undo_fan_boost", generation, &result);
    result
}

/// 只读若干寄存器（寄存器浏览器）.
#[tauri::command]
async fn read_registers(
    state: SharedState<'_>,
    addrs: Vec<u16>,
) -> Result<Vec<RegReading>, String> {
    let worker = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || read_registers_now(&worker, &addrs))
        .await
        .map_err(join_error)?
}

/// 配置加载的 IPC 形状：可读列表，或需要用户先修复文件的明确错误.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProfilesPayload {
    Ready {
        profiles: Vec<Profile>,
    },
    NeedsAttention {
        message: String,
        backup: Option<std::path::PathBuf>,
    },
}

/// 列出已保存的自定义配置.
#[tauri::command]
async fn list_profiles(state: SharedState<'_>) -> Result<ProfilesPayload, String> {
    let dir = logging::data_dir();
    let profiles_state = Arc::clone(state.inner());
    let payload = match tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_profiles(&profiles_state)?;
        Ok::<_, String>(profiles::load_profiles(&dir))
    })
    .await
    .map_err(join_error)?
    {
        Ok(profiles::ProfileLoad::Ready(profiles)) => ProfilesPayload::Ready { profiles },
        Ok(profiles::ProfileLoad::NeedsAttention { message, backup }) => {
            ProfilesPayload::NeedsAttention { message, backup }
        }
        Err(message) => ProfilesPayload::NeedsAttention {
            message,
            backup: None,
        },
    };
    Ok(payload)
}

/// 从已保留的 `.broken` 备份恢复配置（用户显式确认后调用）.
#[tauri::command]
async fn restore_profiles(state: SharedState<'_>) -> Result<Vec<Profile>, String> {
    let dir = logging::data_dir();
    let profiles_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_profiles(&profiles_state)?;
        profiles::restore_broken_profiles(&dir)
    })
    .await
    .map_err(join_error)?
}

/// 保存（或覆盖同名）自定义配置.
#[tauri::command]
async fn save_profile(state: SharedState<'_>, profile: Profile) -> Result<Vec<Profile>, String> {
    let dir = logging::data_dir();
    let profiles_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_profiles(&profiles_state)?;
        let mut list = match profiles::load_profiles(&dir) {
            profiles::ProfileLoad::Ready(list) => list,
            profiles::ProfileLoad::NeedsAttention { message, backup: _ } => {
                return Err(format!("配置需要先处理，未覆盖原文件：{message}"));
            }
        };
        profiles::upsert_profile(&mut list, profile)?;
        profiles::save_profiles(&dir, &list)?;
        Ok(list)
    })
    .await
    .map_err(join_error)?
}

/// 删除自定义配置.
#[tauri::command]
async fn delete_profile(state: SharedState<'_>, name: String) -> Result<Vec<Profile>, String> {
    let dir = logging::data_dir();
    let profiles_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_profiles(&profiles_state)?;
        let mut list = match profiles::load_profiles(&dir) {
            profiles::ProfileLoad::Ready(list) => list,
            profiles::ProfileLoad::NeedsAttention { message, backup: _ } => {
                return Err(format!("配置需要先处理，未覆盖原文件：{message}"));
            }
        };
        profiles::remove_profile(&mut list, &name);
        profiles::save_profiles(&dir, &list)?;
        Ok(list)
    })
    .await
    .map_err(join_error)?
}

/// 应用一条自定义配置.
#[tauri::command]
async fn apply_profile(state: SharedState<'_>, name: String) -> Result<String, String> {
    let worker = Arc::clone(state.inner());
    let recorder = Arc::clone(state.inner());
    let generation = next_generation(state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || apply_profile_now(&worker, &name))
        .await
        .map_err(join_error)?;
    note_result(&recorder, "apply_profile", generation, &result);
    result
}

/// 读风扇曲线（只读）.
#[tauri::command]
async fn get_fan_curves(state: SharedState<'_>) -> Result<FanCurvesDto, String> {
    let worker = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || read_fan_curves_now(&worker))
        .await
        .map_err(join_error)?
}

fn main() {
    // 必须在任何可能失败的逻辑之前：release 下没有 stderr，日志是唯一的事后证据
    let log_dir = logging::init();
    logging::install_panic_hook();
    log::info!(
        "mechrevo-max {} 启动；日志目录 {:?}",
        env!("CARGO_PKG_VERSION"),
        log_dir
    );

    enable_dpi_awareness();

    tauri::Builder::default()
        .manage(Arc::new(AppState::default()))
        .setup(|app| {
            fit_window_to_monitor(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_mode,
            set_power_limits,
            undo_power,
            undo_mode,
            set_fan_boost,
            undo_fan_boost,
            read_registers,
            list_profiles,
            restore_profiles,
            save_profile,
            delete_profile,
            apply_profile,
            get_fan_curves,
            relaunch_elevated
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

/// 声明本进程 DPI 感知.
///
/// 必须在创建任何窗口之前调用. 否则在 150% 缩放的屏幕上，进程会被 Windows
/// 判定为 DPI 不感知并整体放大 1.5 倍，窗口物理尺寸超出屏幕、右侧被裁掉且渲染模糊.
///
/// 用运行时 API 而不是嵌入清单，避免 `embed-resource` 在中文路径 + 旧 SDK 下的编译问题.
#[cfg(windows)]
fn enable_dpi_awareness() {
    /// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`
    const PER_MONITOR_AWARE_V2: isize = -4;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }

    // SAFETY: 传入的是 Windows 文档定义的合法上下文常量，无内存副作用.
    // 返回 0 表示失败（例如系统版本过旧或已被设置过），此处无需处理.
    unsafe {
        SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
fn enable_dpi_awareness() {}

/// 按当前显示器尺寸调整窗口并居中.
///
/// 统一用**物理像素**计算：混用逻辑/物理尺寸在不同 DPI 下会被二次缩放，
/// 导致窗口超出屏幕、右侧内容被裁掉。这里限制为屏幕的 90%，并用 `PhysicalSize`
/// 规避缩放歧义。
fn fit_window_to_monitor(app: &mut tauri::App) {
    use tauri::{Manager as _, PhysicalPosition, PhysicalSize};

    let Some(window) = app.get_webview_window("main") else {
        log::error!("[窗口] 找不到 main 窗口!");
        return;
    };
    let monitor = window.current_monitor();
    log::info!("[窗口] current_monitor = {monitor:?}");
    let Ok(Some(monitor)) = monitor else {
        log::error!("[窗口] 没有可用显示器，跳过尺寸适配");
        return;
    };

    let screen = monitor.size();
    let origin = monitor.position();

    // 目标尺寸取「期望值」与「屏幕 90%」的较小者
    let w = (f64::from(screen.width) * 0.90).clamp(720.0, 1440.0) as u32;
    let h = (f64::from(screen.height) * 0.90).clamp(520.0, 980.0) as u32;

    log::info!(
        "[窗口] 计算尺寸 w={w} h={h}（屏幕 {}x{}）",
        screen.width,
        screen.height
    );
    if let Err(e) = window.set_size(PhysicalSize::new(w, h)) {
        log::error!("[窗口] set_size 失败: {e}");
    }
    if let Err(e) = window.show() {
        log::error!("[窗口] show 失败: {e}");
    }
    log::info!("[窗口] 完成适配, visible={:?}", window.is_visible());

    // 定位到显示器左上角留固定边距.
    // 刻意不用 center()/居中计算 —— 在 150% 缩放下 Tauri 的坐标换算不一致,
    // 会让窗口跑到屏幕外。贴左上角 + 尺寸限制在屏幕 90% 内, 结果必然可见.
    let margin = 40;
    let x = origin.x + margin;
    let y = origin.y + margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 制造一次锁中毒：持有锁的线程 panic，锁即被标记为 poisoned.
    fn poison<T>(lock: &Mutex<T>) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.lock().unwrap();
            panic!("故意 panic 以毒化锁");
        }));
        assert!(result.is_err(), "内部 panic 应被捕获");
        assert!(lock.is_poisoned(), "锁应已中毒");
    }

    #[test]
    fn fresh_state_has_nothing_to_undo() {
        let state = AppState::default();
        assert_eq!(undo_flags(&state), (false, false));
    }

    #[test]
    fn lock_ec_returns_actionable_hint_instead_of_panicking() {
        let state = AppState::default();
        poison(&state.ec);

        let (hint, message) = match lock_ec(&state) {
            Ok(_) => panic!("锁中毒时必须返回错误"),
            Err(pair) => pair,
        };
        assert_eq!(hint, "restart", "应给出 restart 提示而不是驱动修复命令");
        assert!(
            message.contains("重启"),
            "提示文案应告诉用户重启程序，实际: {message}"
        );
    }

    #[test]
    fn undo_flags_tolerates_poisoned_undo_lock() {
        let state = AppState::default();
        poison(&state.undo);
        // 不得 panic，返回"都不能撤销"即可
        assert_eq!(undo_flags(&state), (false, false));
    }

    #[test]
    fn lock_ec_serializes_access() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let state = Arc::new(AppState::default());
        let inside = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let state = Arc::clone(&state);
            let inside = Arc::clone(&inside);
            let max_seen = Arc::clone(&max_seen);
            handles.push(std::thread::spawn(move || {
                for _ in 0..20 {
                    let _guard = lock_ec(&state).expect("锁不应中毒");
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    // 模拟一次 EC 访问的临界区
                    std::thread::yield_now();
                    inside.fetch_sub(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "临界区内同时出现多个访问者 —— EC 访问没有串行化"
        );
    }

    #[test]
    fn status_snapshot_reports_restart_hint_when_ec_lock_poisoned() {
        let state = AppState::default();
        poison(&state.ec);

        let payload = status_snapshot(&state);
        match payload.ec {
            EcStatus::Unavailable { hint, ref message } => {
                assert_eq!(hint, "restart", "锁中毒应给 restart 提示");
                assert!(message.contains("重启"), "文案应引导重启，实际: {message}");
            }
            EcStatus::Ready => panic!("锁中毒时不得报告 EC 可用"),
        }
        assert!(payload.project_id.is_none(), "不可用时不得给出任何读数");
    }

    #[test]
    fn undo_power_without_record_reports_clear_error_before_touching_hardware() {
        // 无记录时应先返回准确文案，而不是"EC 不可用"——也不需要真机
        let state = AppState::default();
        let err = undo_power_now(&state).unwrap_err();
        assert!(err.contains("没有可撤销的功耗改动"), "实际: {err}");
    }

    #[test]
    fn undo_mode_without_record_reports_clear_error_before_touching_hardware() {
        let state = AppState::default();
        let err = undo_mode_now(&state).unwrap_err();
        assert!(err.contains("没有可撤销的档位切换"), "实际: {err}");
    }
    #[test]
    fn note_result_records_failure_and_clears_on_success() {
        let state = AppState::default();
        assert!(last_error(&state).is_none(), "初始不应有历史错误");

        note_result(
            &state,
            "set_power_limits",
            1,
            &Err("写入校验失败 @ 0x0731".to_owned()),
        );
        assert_eq!(
            last_error(&state).as_deref(),
            Some("[set_power_limits] 写入校验失败 @ 0x0731"),
            "失败必须留在状态里，而不是两秒后被轮询冲掉"
        );

        note_result(
            &state,
            "set_power_limits",
            1,
            &Ok("已切换到狂暴档".to_owned()),
        );
        assert!(
            last_error(&state).is_none(),
            "同一操作重试成功后应清空历史错误"
        );
    }

    #[test]
    fn success_of_another_command_does_not_hide_failure() {
        let state = AppState::default();
        note_result(&state, "set_mode", 1, &Err("校验失败".to_owned()));
        note_result(&state, "set_power_limits", 2, &Ok("功耗已保存".to_owned()));
        assert_eq!(last_error(&state).as_deref(), Some("[set_mode] 校验失败"));

        note_result(&state, "set_mode", 1, &Ok("已切换到狂暴档".to_owned()));
        assert!(last_error(&state).is_none(), "同一命令成功后才允许清除");
    }

    #[test]
    fn profiles_lock_failure_is_actionable_not_panic() {
        let state = AppState::default();
        poison(&state.profiles);
        let error = lock_profiles(&state).unwrap_err();
        assert!(error.contains("重启"), "实际: {error}");
    }

    #[test]
    fn older_result_cannot_clear_or_replace_newer_failure() {
        let state = AppState::default();
        // A=1 先发起，B=2 后发起；B 先失败，A 后成功：B 的错误必须保留。
        note_result(&state, "set_mode", 2, &Err("B 较新失败".to_owned()));
        note_result(&state, "set_mode", 1, &Ok("A 较旧成功".to_owned()));
        assert_eq!(last_error(&state).as_deref(), Some("[set_mode] B 较新失败"));

        // A 的迟到失败也不能覆盖 B 的较新失败。
        note_result(&state, "set_mode", 1, &Err("A 较旧失败".to_owned()));
        assert_eq!(last_error(&state).as_deref(), Some("[set_mode] B 较新失败"));
    }

    #[test]
    fn read_registers_rejects_out_of_range_before_touching_hardware() {
        let state = AppState::default();
        let err = read_registers_now(&state, &[0x1000]).unwrap_err();
        assert!(err.contains("超出 EC 范围"), "实际: {err}");
    }

    #[test]
    fn read_registers_rejects_empty_and_too_many() {
        let state = AppState::default();
        assert!(read_registers_now(&state, &[])
            .unwrap_err()
            .contains("请先填写"));

        let too_many: Vec<u16> = (0..=MAX_REGISTER_QUERY as u16).collect();
        let err = read_registers_now(&state, &too_many).unwrap_err();
        assert!(err.contains("最多读取"), "实际: {err}");
    }

    #[test]
    fn unavailable_payload_does_not_carry_stale_last_error() {
        // EC 不可用是"当前状态"，不是"上一次失败"；不得把历史错误混进来误导用户
        let payload = unavailable_with("driver", "EC 驱动未就绪".to_owned(), false, false);
        assert!(payload.last_error.is_none());
    }

    #[test]
    fn percentage_is_calculated_only_for_known_capacity() {
        use crate::sysinfo::BatteryInfo;
        let mut battery = BatteryInfo {
            on_ac: true,
            remaining_mwh: 23000,
            full_mwh: 46000,
            voltage_mv: 12000,
            ..BatteryInfo::default()
        };
        battery.percent = battery
            .remaining_mwh
            .checked_mul(100)
            .and_then(|value| value.checked_div(battery.full_mwh))
            .map(|value| value.min(100));
        assert_eq!(battery_percent(&battery), Some(50));
        assert_eq!(battery_percent(&sysinfo::BatteryInfo::default()), None);
    }
}
