//! 构建脚本.
//!
//! 刻意**不**自定义 Windows 清单：`embed-resource` 查找 `rc.exe` 的行为在含中文的
//! 路径 + 旧版 SDK 下容易失败，且手写清单一旦有细微问题会导致进程直接无法加载。
//!
//! DPI 感知改为在 `src/main.rs` 启动时通过 `SetProcessDpiAwarenessContext` 设置，
//! 效果相同且不依赖构建期资源。

fn main() {
    tauri_build::build()
}
