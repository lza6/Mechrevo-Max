# 变更索引（文件 → 行为）

## v0.1.1（2026-10-24）
| 文件 | 行为变化 |
|---|---|
| `core/src/ec/throttle.rs`（新） | `AccessThrottle`：任意两次访问间隔 ≥ `min_interval`；默认 20 ms，硬下限 10 ms，低于即夹回 |
| `core/src/ec/windows.rs` | `ioctl()` 首行调用限速；`WindowsAcpiTransport` 不再 `Clone`，新增 `with_throttle` / `min_interval` |
| `core/src/ec/mod.rs` | 新增 `Ec::guard()` / `RestoreGuard`（RAII 回滚）+ 4 个单测 |
| `core/src/mode.rs` | `MODE_REGS` 去掉 `0x0767`（自清零命令寄存器不进快照） |
| `core/src/lib.rs` | 导出 `AccessThrottle`/`RestoreGuard`/常量；crate 文档补"限速"纪律 |
| `core/Cargo.toml` | 删除 `serde_json`/`tracing`/`tempfile`；版本 0.1.1 |
| `core/tests/hardware.rs` | 两个写测试改用 `RestoreGuard`；`ec()` 注释说明限速在传输层 |
| `app/src-tauri/src/main.rs` | `AppState.ec` 互斥串行化全部命令；锁中毒返回 `restart` 提示；`undo_*` 成功才出栈；新增 4 个单测 |
| `app/src-tauri/tests/ipc_contract.rs` | 新增源码解析（`parse_ipc_table`）+ 3 个测试（共 8 个） |
| `app/ui/app.js` | 新增 `IPC` 契约表；`setBusy` 停/启轮询；`refresh(force)` 在飞标记；`restart` 提示分支 |
| `app/ui/index.html` | `role="alert"`、`role="status" aria-live="polite"` |
| `app/ui/style.css` | `:focus-visible`、`prefers-reduced-motion` |
| `docs/protocol/ec-baseline.md`（新） | 脱敏可复核协议基线（地址表 + 扫描统计 + 复现命令 + 未验证清单） |
| `docs/ARCHITECTURE.md` | §3/§4 增补限速、守卫、串行化、提交寄存器决策 |
| `docs/DEPLOYMENT.md` | §7 发布清单新增 bin 单测、契约源码解析、文档一致性、合规命令 |
| `docs/VERIFICATION-LEDGER.md` | 44/8/4 测试数与新覆盖项 |
| `.gitignore` | 增 `graft/` |
| 删除 | 空目录 `protocol/`、`test/` |
