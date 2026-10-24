# 已知问题与风险

> 最后更新：2026-10-24（含独立复审修复）。分级依据 `docs/计划书/下一步改进指南.md` §6。

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

## 独立复审（v0.1.2）发现并已修复
- **R1** `AccessThrottle::wait()` 释放锁后 IOCTL 在锁外 → 慢 IOCTL 会重叠。改为 `run()` 整个闭包持锁
- **R2** 闸门是 per-instance → 每命令首访零间隔、新路径可绕过。改为进程级 `global()`
- **R3** 台账把 clippy/fmt 记成无条件通过，实际只跑 core；app fmt 失败 + clippy 3 警告。已修 + 标注 scope
- **R4** `mode.rs` 注释称撤销会写提交寄存器，实际不写。注释已对齐并标注未验证

## 仍开放
| # | 项 | 级别 | 说明 |
|---|---|---|---|
| ~~P1-04~~ | ~~温度读数缺失~~ | **已修（v0.2.0）** | CPU `0x043E` / GPU `0x044F`，与 ACPI 热区（r=0.857）和 nvidia-smi（r=0.989）交叉验证；真实 GUI 已显示 72/43 °C |
| ~~P2-02~~ | ~~无 CI~~ | **已修（v0.1.5+）** | 已有 verify / coverage / package 三 job，并多次真实成功；后续还需保持每次发布核对 |
| P2-03 | 危险操作无二次确认 | P2 | **已修（v0.5.1）** | 切档/强冷/功耗/配置应用先确认；浏览器验证取消不发 IPC、确认才发 IPC。撤销仍仅覆盖最近一次 |
| P2-06 | 滑块范围双份事实源 | P2 | **已修（v0.5.1）** | `StatusPayload.limits_meta` 从 `core::control` 范围常量下发，前端初始化读取 |
| P2-07 | Mode(2) vs OperatingMode(3) 不对齐 | P2 | "均衡"档用 Gaming 默认值，UI 无独立 tab |
| ~~P2-08~~ | ~~MSI/NSIS 从未构建验证~~ | **已定位（v0.1.5）** | CI package job 首次真跑即失败：`Couldn't find a .ico icon` —— `bundle.icon` 从未包含 `icon.ico`，配置本身不可能打包成功。已修，待 v0.1.5 的 CI 复核 |
| P2-10 | 与"禁 E2E"策略冲突的脚本仍在 | P2 | `scripts/e2e_ui.ps1`、`scripts/cdp_probe.mjs` |
| P3-01 | `read_block` 地址回绕 | P3 | **已修（v0.5.1）** | `checked_add` + EC 上界校验；新增越界与上界单测 |
| P3-03 | 缺 `docs/adr/` | P3 | **已修（v0.5.1）** | 新增 `docs/adr/0001-sampling-decoupled-from-ec.md` |
| P3-07 | `tauri.conf.json` `center:true` 与实际定位冲突 | P3 | setup 里贴左上角覆盖它 |
| ~~P3-08~~ | ~~无覆盖率工具与基线~~ | **已修（v0.1.3）** | 已安装 `cargo-llvm-cov`；core 行覆盖 86.56%（排除 `ec/windows.rs` 后 97.1%），app 采样路径待纳入 |
| P3-09 | 只有一张成功态截图 | P3 | 缺"EC 未连接""权限不足"证据 |
| ~~P3-11~~ | ~~`get_status` 280ms 持锁~~ | **P1（已修）** | **根因不是「IOCTL 次数多」，而是命令跑在 UI 线程上**：Tauri v2 的 `#[tauri::command]` 默认 `ExecutionContext::Blocking`，阻塞分支不 spawn（官方 doc：`async` 选项「只在需要不阻塞主线程时才用」）。已全部改 `async fn` + `spawn_blocking`。**实测帧率仍待用户在场验证** |
| ~~P3-12~~ | ~~`RestoreGuard` 在 release 下 Drop 失败仅 `eprintln!`~~ | **已修（v0.1.4）** | 已有 `log` 文件 logger + panic hook，release 下也可追溯；本轮未重复执行真实回滚 |
| P3-13 | 契约测试参数名仍是手写表 | P3 | 源码解析只覆盖命令名，参数名漂移仍抓不到 |
| A-01 | 外部采样在 EC 锁内且可能无限等待 | **P1（已修，v0.5.1）** | 采样移至锁外 + 外部查询 15 秒超时与回收；EC 失败仍保留免驱数据 |
| A-02 | 无电池/`[N/A]`/未知内存被显示为 0 | **P1（已修，v0.5.1）** | 用 Option 表示未知；全零电池不构造对象；GPU 无效行拒绝；前端 21 条路径验证 |
| A-03 | 电池温度单位错误（独立 reviewer） | **P1（已修，v0.5.1）** | WMI 十分之一开尔文 → `T/10 - 273.15`，范围校验与 fixture；reviewer 复验 Approve |
| A-04 | 风扇曲线失败每轮全量重试 | **P1（已修，v0.5.1）** | 失败退避 30 秒、明确重试提示；不会每 2 秒重读 96 次 |
| A-05 | 版本与日志路径只有元素没有值 | **P2（已修，v0.5.1）** | payload 带真实版本与日志目录；浏览器断言显示非空 |

## 第 3 轮调研新增（2026-10-24，explorer）
| # | 项 | 级别 | 说明 |
|---|---|---|---|
| ~~N1~~ | ~~release 下没有安装任何 logger~~ | **已修（v0.1.4）** | `eprintln!` 因 `windows_subsystem="windows"` 必丢；Tauri 自身 9 个文件 20 处 `log::error!` 也全部被丢弃 |
| ~~N2~~ | ~~WebView2 默认需联网~~ | **已修（v0.1.4，改 skip）** | 离线机器装不上；`tauri.conf.json` 未配置 `windows.webviewInstallMode` |
| ~~N3~~ | ~~a11y 四处~~ | **已修（v0.1.4）**；剩余：真机读屏播报仍待验证 | ~~`label` 包住 `output` → 滑块可访问名随值漂移；缺 `aria-valuetext`；`role=alert`+`hidden` 播报不稳定；`.tabs` 无 `role` | P2 | 纯前端可修，播报需读屏实测 |
| N4 | 依赖审计 | 信息 | `cargo-audit`：419 依赖 **0 漏洞**；2 条 informational（proc-macro-error 仅构建期、glib 仅 Linux） |
| N5 | 自动更新评估 | 决策 | **建议不做**：违背离线承诺；私钥泄露=远程写 EC 能力且不可撤销；前置 MSI/NSIS 从未构建。改用「手动检查新版本」 |

## 战略级盲点（未解决）
- **单点依赖官方闭源驱动** `UWACPIDriver.sys`：若其 IOCTL 变化或用户从未装官方控制台，本项目不可用
- **跨机型成本**：EC 地址不通用，每接一个机型近乎一次全新逆向（v0.3.0 才有抽象，且 blocked 于第二台机器）
