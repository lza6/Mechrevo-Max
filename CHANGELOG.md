# 变更日志

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

---

## [Unreleased]

### 规划中
- CPU / GPU 温度读数（待确认寄存器）
- 自动更新机制
- 风扇曲线 / 电压 / MUX（高风险，需先评审）

---

## [0.1.1] — 2026-10-24

护栏补丁版：**不新增硬件功能**，只补齐"安全、可信、可回归"的地基。
全部改动可在无硬件条件下验证（mock 单测 + 静态检查 + 源码级契约解析）。

### 修复（安全）

- **真机测试在断言失败时不回滚硬件**（P0）—— 新增 `RestoreGuard`（RAII），
  panic 展开路径也会完成还原；`core/tests/hardware.rs` 的两个写测试全部改用守卫。
- **EC 访问未串行化**（P1）—— `AppState` 新增 EC 访问互斥，所有 IPC 命令
  （读写/撤销）都必须先取锁；前端在"忙"期间暂停 2 秒轮询，并给 `refresh`
  加在飞标记防止请求重叠。
- **Rust 侧无访问限速护栏**（P1）—— 新增 `AccessThrottle`，由
  `WindowsAcpiTransport` 在**唯一 IOCTL 出口**处调用：默认 20 ms（50 次/秒），
  硬下限 10 ms（100 次/秒），调用方无法绕过。
- **`undo_power` / `undo_mode` 失败时丢弃回滚记录**（P1）—— 改为只有真正
  撤销成功才出栈，失败时保留记录并提示"可重试"。
- **锁中毒导致 `get_status` panic**（P2）—— 改为返回结构化状态 + `restart`
  提示；前端新增"需要重启程序"分支。
- **撤销档位时误写自清零的命令寄存器**（P2）—— 提交寄存器 `0x0767` 移出快照集合。

### 修复（工程与文档）

- **证据链断裂**（P0）：新增 `docs/protocol/ec-baseline.md` —— 脱敏入库版协议基线
  （7 项交叉核对地址 + 传感器/档位编码 + 全量扫描统计 + 复现命令 + 未验证项清单）；
  原始 `reverse/evidence/*.json` 继续不入库。`README` / `ec-access.md` /
  `workflow_status.md` / `CONTRIBUTING.md` 的引用一并改正。
- **IPC 契约测试存在人工同步盲区**（P2）：前端新增 `IPC` 契约表，
  `ipc_contract.rs` 用 `include_str!` 解析 `app/ui/app.js` 源码，
  双向比对命令集，并断言"不存在直接传字符串命令名的 invoke 调用"。
- 修正 4 处文档过期：需求矩阵 R10/R11（实际已闭环）、`workflow_status.md`
  的"下一步"（B1 早已解决）、工具数 12→16、`ec_health.ps1`（不存在）。
- 发布清单新增"文档一致性自检"与合规自动检查命令。
- 删除 3 个死依赖（`serde_json`、`tracing`、`tempfile`）；删除两个空目录
  （`protocol/`、`test/`）；`graft/` 加入 `.gitignore`。

### 可访问性

- 新增 `:focus-visible` 焦点样式（此前键盘用户看不到焦点）；状态栏加
  `role="status" aria-live="polite"`；告警区加 `role="alert"`；
  支持 `prefers-reduced-motion`。

### 测试

- 核心库单测 **33 → 44**（新增：节流夹取/间隔/并发串行、回滚守卫
  drop/disarm/panic 路径、提交寄存器不入快照）。
- IPC 契约测试 **5 → 8**（新增：源码契约表双向比对、禁止裸字符串命令名、
  手写表与源码一致性）。
- 新增二进制单测 **4 个**（锁中毒提示、撤销栈容错、EC 访问串行化）。
- 实际执行：`cargo test` 44 passed + 1 doctest；`cargo clippy --all-targets`
  0 warning；`cargo fmt --check` 干净；`cargo test --bin mechrevo-max` 4 passed；
  `cargo test --test ipc_contract` 8 passed；`node --check app/ui/app.js` 通过。

### 已知限制（与 0.1.0 相同）

- 温度读数仍未实现（v0.2.0，需一次用户在场的只读探测）
- 仅在本机（旷世16 Pro GM6PQ0X）验证；跨机型地址不通用
- 真机集成测试与 UI 自动化 E2E 按安全策略不执行

---

## [0.1.0] — 2026-10-17

首个可用版本。核心链路已在真机完整验证。

### 新增

**核心库 `mechrevo-core`**
- EC 访问层
  - `EcTransport` trait 抽象，真机实现 `WindowsAcpiTransport`（IOCTL `0x9C40A488` / `0x9C40A48C`）
  - `MockTransport` 供离线测试，可注入「写了不生效」「读取失败」等故障
  - `Ec<T>`：写后读回校验、位操作、`RollbackPoint` 快照与还原
- 运行档位切换（`ModeController`）
  - 均衡 / 办公 / 狂暴三档，组合 `0x751` + `0x7AB`，提交寄存器 `0x0767`
  - 正确识别提交寄存器「写后自清零」特性，不做读回校验
- 功耗墙控制（`PowerController`）
  - PL1 / PL2 / PL4 读写，范围校验（PL1/PL2: 5–200W，PL4: 0–255W）
  - 写入前存回滚点，失败自动回滚已写入项
- 传感器读数（`Sensors`）
  - CPU / GPU 风扇转速、电源来源、项目 ID
- 寄存器地址表（`namespace`）
  - 每项标注可信度：`Verified` / `CommunityUnverified` / `NotApplicable`

**桌面应用（Tauri v2）**
- 运行档位一键切换，当前档位高亮 + 原始寄存器值显示
- 实时读数（CPU/GPU 风扇、供电状态），2 秒轮询
- 默认功耗墙滑块（游戏/办公分别配置）
- 一键撤销（档位切换与功耗改动均可回退）
- EC 不可用时给出可执行的修复指引
- 权限不足时提供「以管理员身份重启」按钮
- 启动自检：UI 元素 id 与 JS 映射不一致时立刻报告
- 滑块编辑锁：防止轮询覆盖用户正在拖动的值

**工具（Python，16 个）**
- EC 读取 / 扫描 / 验证 / 健康自检 / 档位实验 / 稳定性测试
- ACPI 表导出与检索、AML 解析（EC 操作区、设备定义）
- 驱动版本对比、官方注册表解析

**诊断脚本（PowerShell）**
- `hw_health.ps1` 硬件健康快照
- `diagnose_reboot.ps1` 重启原因分析
- 驱动枚举、运行态采集、UIA 树导出

**文档**
- `docs/SPEC.md` 规格基线
- `docs/ARCHITECTURE.md` 架构与扩展指引
- `docs/DEPLOYMENT.md` 构建/发布/回滚
- `docs/TROUBLESHOOTING.md` 排障
- `docs/CONTRIBUTING.md` 贡献指南
- `docs/VERIFICATION-LEDGER.md` 验证台账
- `docs/protocol/ec-access.md` 协议权威来源
- `docs/REQUIREMENTS-TRACEABILITY.md` 需求追踪矩阵

### 安全

- **EC 访问强制限速**（默认 ≤50 次/秒，硬上限 100 次/秒，代码内强制）
- 写入一律读回校验，不信任驱动返回值
- 所有写入可回滚
- 不联网、不收集任何用户数据
- 不打包、不分发任何官方二进制 / 驱动 / 反编译产物

### 已修复（相对官方控制台的缺陷）

| 官方缺陷 | 本项目的做法 |
|---|---|
| 内存占用 178 MB | 单进程，运行内存约 52 MB |
| 主程序 .NET Native AOT 无法审计 | 全 Rust 源码 + 33 单测 |
| MQTT 通信密码明文硬编码 | 无进程间通信，无凭据 |
| 通信失败被 `catch {}` 完全静默 | 失败一律报错并显示到界面 |
| 装完 EC 通道不可用（PnP 未扫描） | 文档显式说明修复步骤，界面给出指引 |
| 能力表数据自相矛盾 | 每项能力标注验证状态 |

### 已知限制

- 仅支持 Windows x64
- 需要管理员权限
- 需先安装官方控制台以获取 `UWACPIDriver.sys`（本项目不分发该驱动）
- 温度读数尚未实现
- 仅在本机（MECHREVO 旷世16 Pro GM6PQ0X，BIOS N.1.06MRO16）验证
- 跨机型地址表不通用，新机型需实机重新验证

### 重要事件记录

2026-10-17 02:11 本机发生一次「无转储硬复位」（Event 41 Kernel-Power）。
开发过程中的 CPU 满载压力测试与 EC 泛洪（约 620 次/秒）都可能是触发器。
**已据此实施 EC 访问限速，并禁止一切硬件压力测试。**
同模式事件在 2026-10-13 与 2026-10-16 也曾出现，均早于本次工作，
说明是机器既有的偶发问题。详见 `workflow_status.md`。
