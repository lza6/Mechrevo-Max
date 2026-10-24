# 测试范围台账

| 模块 | 测试类型 | 数量 | 状态 | 最后通过 |
|---|---|---|---|---|
| `ec::transport`（Mock） | 单元 | 4 | passed | 2026-10-24 |
| `ec::throttle`（限速+不重叠+进程级） | 单元 | 8 | passed | 2026-10-24 |
| `ec`（Ec/守卫/快照） | 单元 | 11 | passed | 2026-10-24 |
| `control`（功耗墙） | 单元 | 6 | passed | 2026-10-24 |
| `mode`（档位 + 强冷位） | 单元 | 9 | passed | 2026-10-24 |
| `sensors`（含温度） | 单元 | 8 | passed | 2026-10-24 |
| `namespace` | 单元 | 3 | passed | 2026-10-24 |
| 合计（core） | 单元 | 49 | passed | 2026-10-24 |
| doctest | 文档 | 1 | passed | 2026-10-24 |
| `main.rs`（锁/串行化/中毒/无记录路径） | 单元（bin） | 7 | passed | 2026-10-24 |
| IPC 契约 | 集成 | 8 | passed | 2026-10-24 |
| 真机硬件 | 集成 | 7 | **ignored（按策略不跑）** | 2026-10-17 |
| UI 路径 | E2E | 0 | **禁用（用户指示）** | — |

覆盖率：**core 行覆盖 86.56%，排除 `ec/windows.rs` 后 97.1%（2026-10-24 历史基线；本轮未重跑，app 新增采样路径未统计）**。

## 2026-10-25 定向审计修复（v0.5.1）

| 模块 | 测试类型 | 结果 | 证据 |
|---|---|---|---|
| core 全部 | 单元 + doctest | **70 passed + 1 passed** | `RUSTDOC=<rustup 工具链真实路径> cargo test --offline` |
| app 全部（含采样、配置、日志） | 单元（bin） | **32 passed** | `cargo test --offline --bin mechrevo-max` |
| IPC 契约 | 集成 | **8 passed** | `cargo test --offline --test ipc_contract` |
| 采样边界 | fixture | **11 passed** | 无电池/合法磁盘/无效与合法 GPU/温度换算等 |
| 前端降级与交互 | 独立 Chromium + mock IPC | **21/21，errors=[]** | `node tools/qa_browser.cjs`；非真实硬件验证 |
| 真机健康 | 只读 smoke | **519 次读取 0 失败** | `tools/ec_health.py`；0x751=0x00；没有写入 |
| 独立审查 | 静态复核 + 证据 | **Approve** | Request Changes → 温度单位修复 → 复验通过 |
| 真机写入与窗口外观 | E2E | **blocked** | 遵守禁止真机 UI 自动化；不以 mock 替代 |
| 长时 soak、多机型、干净机实装 | 集成 | **未验证** | 不在本轮声称完成 |
