# 已知问题与风险

> 最后更新：2026-10-24。分级依据 `docs/计划书/下一步改进指南.md` §6。

## 已修复（v0.1.1）
- P0-01 真机测试 panic 不回滚 → `RestoreGuard`
- P0-02 证据链断裂 → `docs/protocol/ec-baseline.md` 脱敏入库
- P1-01 EC 访问未串行化 → `AppState.ec` 互斥 + 忙时停轮询
- P1-02 undo 失败丢记录 → 成功才出栈
- P1-03 Rust 侧无限速 → `AccessThrottle`（唯一 IOCTL 出口）
- P1-05 文档过期 4 处 → 已修正 + 发布清单加一致性检查
- P2-01 契约测试盲区 → 解析 `app.js` IPC 表
- P2-04 可访问性 → focus-visible / aria-live / reduced-motion
- P2-05 死依赖 → 已删
- P2-09 提交寄存器进快照 → 已移出

## 仍开放
| # | 项 | 级别 | 说明 |
|---|---|---|---|
| P1-04 | 温度读数缺失 | P1 | v0.2.0；需用户在场的只读探测（≤50 次/秒、空闲态、不施负载） |
| P2-02 | 无 CI | P2 | 每次发布靠人工清单；已在清单加自检项 |
| P2-03 | 危险操作无二次确认 | P2 | 仅撤销兜底，且只覆盖最后一次 |
| P2-06 | 滑块范围双份事实源 | P2 | `control.rs` 常量 vs `index.html` min/max |
| P2-07 | Mode(2) vs OperatingMode(3) 不对齐 | P2 | "均衡"档用 Gaming 默认值，UI 无独立 tab |
| P2-08 | MSI/NSIS 从未构建验证 | P2 | 配置声明了 targets，Release 只发过 exe |
| P2-10 | 与"禁 E2E"策略冲突的脚本仍在 | P2 | `scripts/e2e_ui.ps1`、`scripts/cdp_probe.mjs` |
| P3-01 | `read_block` 地址回绕 | P3 | `wrapping_add`，跨 0x0FFF 静默环绕 |
| P3-03 | 缺 `docs/adr/` | P3 | SPEC §9 要求 |
| P3-07 | `tauri.conf.json` `center:true` 与实际定位冲突 | P3 | setup 里贴左上角覆盖它 |
| P3-08 | 无覆盖率工具与基线 | P3 | v0.2.1 |
| P3-09 | 只有一张成功态截图 | P3 | 缺"EC 未连接""权限不足"证据 |

## 战略级盲点（未解决）
- **单点依赖官方闭源驱动** `UWACPIDriver.sys`：若其 IOCTL 变化或用户从未装官方控制台，本项目不可用
- **跨机型成本**：EC 地址不通用，每接一个机型近乎一次全新逆向（v0.3.0 才有抽象，且 blocked 于第二台机器）
