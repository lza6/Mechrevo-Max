# 已验证范围（可信任，不必重复验证）

> 最后更新：2026-10-24（v0.1.1）。详细命令与输出见 `docs/VERIFICATION-LEDGER.md` 与 `evidence/`。

| 项 | 命令 | 结果 | 日期 |
|---|---|---|---|
| 核心单测 | `cd core && cargo test --offline` | 44 passed + 1 doctest | 2026-10-24 |
| clippy | `cargo clippy --all-targets` | 0 warning | 2026-10-24 |
| fmt | `cargo fmt --check` | 干净 | 2026-10-24 |
| IPC 命令层单测 | `cargo test --bin mechrevo-max` | 4 passed | 2026-10-24 |
| IPC 契约 | `cargo test --test ipc_contract` | 8 passed | 2026-10-24 |
| 前端语法 | `node --check app/ui/app.js` | 通过 | 2026-10-24 |
| 类型检查（含 main.rs） | `cargo check --all-targets` | 通过 | 2026-10-24 |
| release 构建 | `cargo build --release` | 见 v0.1.1 证据 | 2026-10-24 |

**未验证**：真机硬件测试（`--ignored`，按安全策略不做）、GUI 运行时路径、覆盖率、跨机型。
