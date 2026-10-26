# 变更索引（文件 → 行为）

## v0.5.2（2026-10-25，可靠性复审）
| 文件 | 行为变化 |
|---|---|
| `main.rs` | EC 部分读取失败进入 `read_warnings`；命令错误按 command+generation 归属；配置锁 profiles→ec；版本/日志/恢复命令 |
| `sysinfo.rs` | 启动即加入 Job Object（PROCESS_SET_QUOTA），超时/输出超限显式 TerminateJobObject；输出并发排空与 2 MiB 上限 |
| `profiles.rs` | 配置临时文件+sync_all+原子替换+备份；损坏 NeedsAttention；I/O 区分；显式恢复 |
| `app/ui/app.js` | 同时显示部分读取警告与操作失败；损坏配置恢复按钮与确认 |
| `tools/qa_browser.cjs` | 28 条浏览器验收；恢复确认与部分失败场景 |


## v0.5.1（2026-10-25，采样与交付边界修复）
| 文件 | 行为变化 |
|---|---|
| `app/src-tauri/src/sysinfo.rs` | 外部查询 15 秒超时与回收；无电池/无效 GPU/未知内存不伪造 0；电池温度单位转换；失败缓存标注 |
| `app/src-tauri/src/main.rs` | 采样移出 EC 锁；EC 失败仍返回免驱数据；版本/日志路径/范围元数据进 payload |
| `app/ui/app.js` | 未知值显示 `—`；曲线失败退避；危险写入二次确认；范围来自 core |
| `core/src/ec/mod.rs` | `read_block` 禁止越界/地址环绕，新增边界测试 |
| `tools/qa_browser.cjs` | 可复跑的独立 Chromium + mock IPC 验收（21 条），不访问硬件 |
| `docs/`、`.agent/`、`.wolf/` | ADR、技能、bug 记录、历史状态纠偏、归档与 manifest |


## v0.1.3（2026-10-24，执行线程修正）
| 文件 | 行为变化 |
|---|---|
| `app/src-tauri/src/main.rs` | 6 个命令拆成「同步业务体 + `async fn`/`spawn_blocking` 包装」；`AppState` 改 `Arc`；`get_status` 返回 `Result`（恒 Ok）；`undo_*` 先查记录再碰硬件；+3 单测 |
| `policy.md` | 覆盖率事实更正（工具已装、86.56% 基线）；「100% 分支」→「行覆盖 ≥95% + Err 分支约定」 |
| `test_ledger.md` | 新增接缝 S11 命令执行线程 |

## v0.1.2（2026-10-24，独立复审修复）
| 文件 | 行为变化 |
|---|---|
| `core/src/ec/throttle.rs` | `wait()` → `run(closure)`：**整个访问持锁**（保证不重叠 + 间隔）；新增进程级 `global()`（OnceLock）；间隔从上次访问**结束**计时 |
| `core/src/ec/windows.rs` | `ioctl()` 把 `ioctl_unguarded()` 包进 `global().run()`；删除 per-instance 闸门字段与 `with_throttle` |
| `core/src/mode.rs` | 注释与实现对齐（撤销只回写字段，不写 `0x0767`），标注"是否需补提交未验证" |
| `core/src/lib.rs` | crate 文档第 4 条改为"持锁 + 进程级"表述 |
| `app/src-tauri/src/main.rs` | `get_status` 合并 `0x751` 重复读取；`clamp` 替代 `min/max`；测试去掉 `err().expect()` |
| 版本 | core / app / tauri.conf 同步 0.1.2 |

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
| 2026-10-25 | core/src/fancurve.rs | +写会话(validate/write_curve/pad_to_block/write_settled/RpmMonitor) | 83bf78a |
| 2026-10-25 | core/src/ec/transport.rs | MockTransport 手写 Clone | 83bf78a |
| 2026-10-25 | evidence/fan-dump-20261025.json | V-1 只读重扫 96 字节,与基线 0 diff | 83bf78a |
