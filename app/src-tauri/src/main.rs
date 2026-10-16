//! 机械革命控制台 —— Tauri 后端.
//!
//! 职责：把 `mechrevo-core` 的能力暴露给前端，并持有"最近一次改动"的状态，
//! 使用户随时可以一键撤销。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mechrevo_core::ec::{Ec, WindowsAcpiTransport};
use mechrevo_core::namespace::Mode as PowerMode;
use mechrevo_core::{
    ModeChange, ModeController, OperatingMode, PowerController, PowerLimits, Sensors,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

/// 最近一次改动，用于一键撤销.
#[derive(Default)]
struct UndoStack {
    power: Option<mechrevo_core::ChangeRecord>,
    mode: Option<ModeChange>,
}

/// 全局应用状态.
#[derive(Default)]
struct AppState {
    undo: Mutex<UndoStack>,
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
        /// 提示类型: `driver` = 驱动/设备未就绪; `elevation` = 权限不足.
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
    cpu_fan_rpm: Option<u16>,
    gpu_fan_rpm: Option<u16>,
    mode: Option<&'static str>,
    mode_raw: Option<(u8, u8)>,
    gaming_power: Option<PowerLimitsDto>,
    office_power: Option<PowerLimitsDto>,
    can_undo_power: bool,
    can_undo_mode: bool,
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
        mechrevo_core::Error::DeviceOpen { source, .. }
            if source.raw_os_error() == Some(5) =>
        {
            (
                "elevation",
                "权限不足 —— 访问 EC 需要管理员权限。".to_owned(),
            )
        }
        other => ("driver", other.to_string()),
    }
}

/// 构造一个"EC 不可用"的状态载荷.
fn unavailable(err: &mechrevo_core::Error, can_undo_power: bool, can_undo_mode: bool) -> StatusPayload {
    let (hint, message) = classify(err);
    StatusPayload {
        ec: EcStatus::Unavailable { message, hint },
        project_id: None,
        power_source: None,
        is_ac_powered: None,
        cpu_fan_rpm: None,
        gpu_fan_rpm: None,
        mode: None,
        mode_raw: None,
        gaming_power: None,
        office_power: None,
        can_undo_power,
        can_undo_mode,
    }
}

/// 读取当前全部状态. EC 不可用时也返回结构化结果，不抛错让前端能渲染提示.
#[tauri::command]
fn get_status(state: State<'_, AppState>) -> StatusPayload {
    let undo = state.undo.lock().expect("undo 锁中毒");
    let can_undo_power = undo.power.is_some();
    let can_undo_mode = undo.mode.is_some();
    drop(undo);

    let ec = match open_ec() {
        Ok(ec) => ec,
        Err(e) => return unavailable(&e, can_undo_power, can_undo_mode),
    };

    // 设备对象可能存在但打不开（典型是权限不足），这里用一次真实读取来区分
    let mode_ctl = ModeController::new(&ec);
    let power_ctl = PowerController::new(&ec);
    let sensors = Sensors::new(&ec);

    let snap = match sensors.snapshot() {
        Ok(s) => s,
        Err(e) => return unavailable(&e, can_undo_power, can_undo_mode),
    };

    StatusPayload {
        ec: EcStatus::Ready,
        project_id: Some(snap.project_id),
        power_source: Some(snap.power_source),
        is_ac_powered: Some(snap.is_ac_powered()),
        cpu_fan_rpm: Some(snap.cpu_fan_rpm),
        gpu_fan_rpm: Some(snap.gpu_fan_rpm),
        mode: mode_ctl.current().ok().flatten().map(|m| m.key()),
        mode_raw: mode_ctl.raw_fields().ok(),
        gaming_power: power_ctl
            .read_defaults(PowerMode::Gaming)
            .ok()
            .map(PowerLimitsDto::from),
        office_power: power_ctl
            .read_defaults(PowerMode::Office)
            .ok()
            .map(PowerLimitsDto::from),
        can_undo_power,
        can_undo_mode,
    }
}

/// 以管理员身份重新启动本程序（当前进程随即退出）.
///
/// 用 PowerShell 的 `Start-Process -Verb RunAs` 触发 UAC，
/// 避免依赖构建期嵌入的 manifest（本机 RC.EXE 在中文路径下会失败）.
#[tauri::command]
fn relaunch_elevated() -> Result<(), String> {
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

/// 切换运行档位.
#[tauri::command]
fn set_mode(state: State<'_, AppState>, mode: String) -> Result<String, String> {
    let target = OperatingMode::from_key(&mode)
        .ok_or_else(|| format!("未知档位: {mode}"))?;

    let ec = open_ec().map_err(|e| e.to_string())?;
    let ctl = ModeController::new(&ec);
    let change = ctl.switch(target).map_err(|e| e.to_string())?;
    let name = target.display_name().to_owned();

    let mut undo = state.undo.lock().map_err(|e| e.to_string())?;
    undo.mode = Some(change);
    Ok(format!("已切换到{name}档"))
}

/// 设置某模式的默认功耗.
#[tauri::command]
fn set_power_limits(
    state: State<'_, AppState>,
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
    let ec = open_ec().map_err(|e| e.to_string())?;
    let ctl = PowerController::new(&ec);
    let record = ctl.apply_defaults(target, &limits).map_err(|e| e.to_string())?;

    let mut undo = state.undo.lock().map_err(|e| e.to_string())?;
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
#[tauri::command]
fn undo_power(state: State<'_, AppState>) -> Result<String, String> {
    let ec = open_ec().map_err(|e| e.to_string())?;
    let mut undo = state.undo.lock().map_err(|e| e.to_string())?;
    let Some(record) = undo.power.take() else {
        return Err("没有可撤销的功耗改动".into());
    };
    let n = record.undo(&ec).map_err(|e| e.to_string())?;
    Ok(format!("已还原 {n} 个寄存器"))
}

/// 撤销最近一次档位切换.
#[tauri::command]
fn undo_mode(state: State<'_, AppState>) -> Result<String, String> {
    let ec = open_ec().map_err(|e| e.to_string())?;
    let mut undo = state.undo.lock().map_err(|e| e.to_string())?;
    let Some(change) = undo.mode.take() else {
        return Err("没有可撤销的档位切换".into());
    };
    let n = change.undo(&ec).map_err(|e| e.to_string())?;
    Ok(format!("已还原 {n} 个寄存器"))
}

fn main() {
    enable_dpi_awareness();

    tauri::Builder::default()
        .manage(AppState::default())
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
        return;
    };
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };

    let screen = monitor.size();
    let origin = monitor.position();

    // 目标尺寸取「期望值」与「屏幕 90%」的较小者
    let w = (f64::from(screen.width) * 0.90).min(1440.0).max(720.0) as u32;
    let h = (f64::from(screen.height) * 0.90).min(980.0).max(520.0) as u32;

    let _ = window.set_size(PhysicalSize::new(w, h));

    // 定位到显示器左上角留固定边距.
    // 刻意不用 center()/居中计算 —— 在 150% 缩放下 Tauri 的坐标换算不一致,
    // 会让窗口跑到屏幕外。贴左上角 + 尺寸限制在屏幕 90% 内, 结果必然可见.
    let margin = 40;
    let x = origin.x + margin;
    let y = origin.y + margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
