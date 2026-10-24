# 已验证范围（可信任，不必重复验证）

> 最后更新：2026-10-24（v0.1.3）。详细命令与输出见 `docs/VERIFICATION-LEDGER.md` 与 `evidence/`。

| 项 | 命令 | 结果 | 日期 |
|---|---|---|---|
| 核心单测 | `cd core && cargo test --offline` | 46 passed + 1 doctest | 2026-10-24 |
| clippy（core + app 两个 crate） | `cargo clippy --all-targets` | 两个包均 0 warning | 2026-10-24 |
| fmt（core + app 两个 crate） | `cargo fmt --check` | 两个包均 exit 0 | 2026-10-24 |
| IPC 命令层单测 | `cargo test --bin mechrevo-max` | 7 passed | 2026-10-24 |
| IPC 契约 | `cargo test --test ipc_contract` | 8 passed | 2026-10-24 |
| 前端语法 | `node --check app/ui/app.js` | 通过 | 2026-10-24 |
| 类型检查（含 main.rs） | `cargo check --all-targets` | 通过 | 2026-10-24 |
| release 构建 | `cargo build --release` | exe 5 091 840 B，sha256 `1b279b85…59c7b` | 2026-10-24 |
| 覆盖率基线 | `cargo llvm-cov`（core） | 行覆盖 86.56%，排除 windows.rs 后 97.1% | 2026-10-24 |
| 独立复审 | reviewer 子代理 | CONDITIONAL PASS → R1/R2/R3/R4 已全部修复并复验 | 2026-10-24 |

**未验证**：真机硬件测试（`--ignored`，按安全策略不做）、GUI 运行时路径、覆盖率、跨机型。
