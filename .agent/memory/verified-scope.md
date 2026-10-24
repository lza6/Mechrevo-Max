# 已验证范围（可信任，不必重复验证）

> 最后更新：2026-10-24（v0.2.0）。详细命令与输出见 `docs/VERIFICATION-LEDGER.md` 与 `evidence/`。

| 项 | 命令 | 结果 | 日期 |
|---|---|---|---|
| 核心单测 | `cd core && cargo test --offline` | 49 passed + 1 doctest | 2026-10-24 |
| clippy（core + app 两个 crate） | `cargo clippy --all-targets` | 两个包均 0 warning | 2026-10-24 |
| fmt（core + app 两个 crate） | `cargo fmt --check` | 两个包均 exit 0 | 2026-10-24 |
| IPC 命令层单测 | `cargo test --bin mechrevo-max` | 7 passed | 2026-10-24 |
| IPC 契约 | `cargo test --test ipc_contract` | 8 passed | 2026-10-24 |
| 前端语法 | `node --check app/ui/app.js` | 通过 | 2026-10-24 |
| 类型检查（含 main.rs） | `cargo check --all-targets` | 通过 | 2026-10-24 |
| release 构建 | `cargo build --release` | exe 5 091 840 B，sha256 `1b279b85…59c7b` | 2026-10-24 |
| 覆盖率基线 | `cargo llvm-cov`（core） | 行覆盖 86.56%，排除 windows.rs 后 97.1% | 2026-10-24 |
| 独立复审（第 1 轮） | reviewer 子代理 | CONDITIONAL PASS → R1/R2/R3/R4 已全部修复（第 2 轮复验结果待回） | 2026-10-24 |
| CI（含代码改动） | run 40100014152 | success，全部步骤成功 | 2026-10-24 |
| 温度寄存器 | EC 探测 + 交叉参照 | CPU 0x043E r=0.857（ACPI）、GPU 0x044F r=0.989（nvidia-smi） | 2026-10-24 |
| 真实 GUI 端到端 | 启动 exe + 截图 | 72/43 °C、风扇 5486/5250、均衡高亮（751=0x40）、内存 62.6 MB | 2026-10-24 |
| release 日志落盘 | 读 `%LOCALAPPDATA%\Mechrevo-Max\logs` | 有启动行 | 2026-10-24 |

**未验证**：真机硬件测试（`--ignored`，按安全策略不做）、GUI 运行时路径、覆盖率、跨机型。
