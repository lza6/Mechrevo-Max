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
use std::sync::{Arc, Mutex};
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
    /// 串行化所有 EC 访问.
    ///
    /// EC 硬件不支持并发访问（`docs/SPEC.md` §7）。此前的实现只保护了撤销栈，
    /// 没有保护 EC 本身：2 秒轮询与用户写入会各自打开句柄同时读写，
    /// 可能读到"写了一半"的状态。现在所有命令都必须先取这把锁。
    ec: Mutex<()>,
    /// 最近一次改动，用于一键撤销.
    undo: Mutex<UndoStack>,
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
        mechrevo_core::Error::DeviceOpen { source, .. } if source.raw_os_error() == Some(5) => (
            "elevation",
            "权限不足 —— 访问 EC 需要管理员权限。".to_owned(),
        ),
        other => ("driver", other.to_string()),
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

/// 读取当前全部状态（同步业务体，跑在 blocking 线程池上）.
///
/// EC 不可用时也返回结构化结果，不抛错，让前端能渲染可行动的提示.
fn status_snapshot(state: &AppState) -> StatusPayload {
    // 串行化：轮询与用户操作不会同时进硬件
    let _guard = match lock_ec(state) {
        Ok(guard) => guard,
        Err((hint, message)) => {
            let (can_undo_power, can_undo_mode) = undo_flags(state);
            return unavailable_with(hint, message, can_undo_power, can_undo_mode);
        }
    };
    let (can_undo_power, can_undo_mode) = undo_flags(state);

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

    // 档位字段只读一次：`current()` 与 `raw_fields()` 都会读 0x751，
    // 合并后每轮轮询少一次 EC 访问（20 ms 限速下等于少 20 ms 延迟）。
    let raw = mode_ctl.raw_fields().ok();
    let mode_key = raw
        .and_then(|(field1, _)| OperatingMode::from_field1(field1))
        .map(OperatingMode::key);

    StatusPayload {
        ec: EcStatus::Ready,
        project_id: Some(snap.project_id),
        power_source: Some(snap.power_source),
        is_ac_powered: Some(snap.is_ac_powered()),
        cpu_fan_rpm: Some(snap.cpu_fan_rpm),
        gpu_fan_rpm: Some(snap.gpu_fan_rpm),
        mode: mode_key,
        mode_raw: raw,
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
    format!("内部任务异常（{e}），请重启本程序")
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
    tauri::async_runtime::spawn_blocking(move || switch_mode(&worker_state, target))
        .await
        .map_err(join_error)?
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
    tauri::async_runtime::spawn_blocking(move || apply_power_limits(&worker_state, target, limits))
        .await
        .map_err(join_error)?
}

/// 撤销最近一次功耗改动.
#[tauri::command]
async fn undo_power(state: SharedState<'_>) -> Result<String, String> {
    let worker_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || undo_power_now(&worker_state))
        .await
        .map_err(join_error)?
}

/// 撤销最近一次档位切换.
#[tauri::command]
async fn undo_mode(state: SharedState<'_>) -> Result<String, String> {
    let worker_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || undo_mode_now(&worker_state))
        .await
        .map_err(join_error)?
}

/// 以管理员身份重新启动本程序（当前进程随即退出）.
#[tauri::command]
async fn relaunch_elevated() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(relaunch_elevated_now)
        .await
        .map_err(join_error)?
}

fn main() {
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
    let w = (f64::from(screen.width) * 0.90).clamp(720.0, 1440.0) as u32;
    let h = (f64::from(screen.height) * 0.90).clamp(520.0, 980.0) as u32;

    let _ = window.set_size(PhysicalSize::new(w, h));

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
}
