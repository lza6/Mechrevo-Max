# 验证台账

> **用途**：避免重复验证。
>
> **怎么用**：
> 1. 改动前先查本文件 —— 若目标区域「上次验证」之后**未改动**，可跳过重复验证
> 2. 改动后**必须**更新对应行的「最后改动」与「验证」两列
> 3. 新增验证项时追加到对应分组
>
> **状态含义**：
> - `✅ 已验证` = 实际跑过，命令与结果记录在「证据」列
> - `⚙️ 静态确认` = 只读代码确认，未实际运行
> - `❓ 待验证` = 未验证
> - `🚫 禁用` = 按安全策略刻意不做（见 `docs/SPEC.md` §5.1）

最后全量验证：**2026-10-24**

---

## 1. 核心库（`core/`）

| 项 | 状态 | 证据 | 最后改动 |
|---|---|---|---|
| 单元测试（46 个） | ✅ 已验证 | `cargo test` → `46 passed; 0 failed` | 2026-10-24 |
| clippy 全目标（**scope: core + app 两个 crate**） | ✅ 已验证 | `cargo clippy --all-targets` → 两个包均 0 warning | 2026-10-24 |
| 格式化（**scope: core + app 两个 crate**） | ✅ 已验证 | `cargo fmt --check` → 两个包均 exit 0 | 2026-10-24 |
| 文档测试 | ✅ 已验证 | `cargo test` → `1 passed`（doctest） | 2026-10-24 |
| EC 访问限速 + 不重叠 | ✅ 已验证 | `ec::throttle` 单测 8 个：默认 50 次/秒、低于硬下限被夹回、首次不等待、间隔达标、**4 线程 ×「比间隔还慢的模拟 IOCTL」并发峰值恒为 1**、进程级单例（`global()` 两次取用同一实例） | 2026-10-24 |
| 回滚守卫（panic 路径） | ✅ 已验证 | `guard_restores_after_panic`（`catch_unwind` 后值已还原） | 2026-10-24 |

### 覆盖的测试要点（改动这些区域时须重跑）

| 模块 | 已覆盖 |
|---|---|
| `ec::transport` | Mock 读写、stuck 地址忽略写入、读失败返回 Err 而非 0 |
| `ec::throttle` | 默认间隔、硬下限夹取、首次不阻塞、间隔补足（从上次访问**结束**计时）、**访问不重叠（峰值并发=1）**、进程级单例共享 |
| `ec`（Ec\<T\>） | 写后读回、校验失败检测、位操作只动一位、越界拒绝、快照/回滚往返、快照遇读失败即报错、连续块读、**守卫 drop/disarm/restore_now/panic 回滚** |
| `control` | 读官方出厂值、越界前置拦截、应用+回滚、**寄存器卡住时自动回滚** |
| `mode` | 三档识别、未知编码返回 None（不猜）、三档往复切换、**提交寄存器自清零不误报**、卡住时回滚、key 往返、**提交寄存器不入快照** |
| `sensors` | CPU/GPU 风扇、项目 ID、AC 检测、**未知电源编码不猜测**、读失败传播 |
| `namespace` | Gaming/Office 地址不重叠、功耗寄存器均为 Verified、NotApplicable 标注 |

---

## 2. 桌面应用（`app/`）

| 项 | 状态 | 证据 | 最后改动 |
|---|---|---|---|
| IPC 契约测试（8 个） | ✅ 已验证 | `cargo test --test ipc_contract` → `8 passed` | 2026-10-24 |
| IPC 命令层单测（4 个） | ✅ 已验证 | `cargo test --bin mechrevo-max` → `4 passed`（锁中毒→restart 提示、撤销栈容错、EC 访问串行化） | 2026-10-24 |
| release 构建 | ✅ 已验证 | `cargo build --release` → 成功，exe 4 992 512 B（4.76 MiB），sha256 `aa4bcaaf…beae9cc9` | 2026-10-24 |
| 推送 | ✅ 已验证 | `git push origin main` → `898c4c2..1fe2a14`；`git ls-remote origin refs/heads/main` = `1fe2a14` | 2026-10-24 |
| Release 发布 | ✅ 已验证 | `tools/publish_release.py` 依次创建 v0.1.1（4 992 512 B）与 **v0.1.2**（**4 994 048 B**）；API 复核 v0.1.0/v0.1.1/v0.1.2 均 `draft=false`、asset 大小与本地构建一致，`releases/latest` = v0.1.2 | 2026-10-24 |
| 前端语法 | ✅ 已验证 | `node --check app/ui/app.js` → 通过 | 2026-10-24 |
| 前端源码契约解析 | ✅ 已验证 | `frontend_source_ipc_table_matches_backend`：解析 `app.js` 的 `IPC` 表并与后端命令集双向比对；`no_raw_string_command_names_in_frontend` 禁止裸字符串命令名 | 2026-10-24 |
| UI 元素 id 与 `ui` 映射一致性 | ✅ 静态确认 | 28 项逐一比对，全部匹配 | 2026-10-17 |
| 前端渲染与交互（浏览器 + mock 后端） | ✅ 已验证 | Playwright(Chromium) + 假 `invoke`：37/37 断言通过，`pageerror`/`console.error` 各 0；覆盖 ready / 驱动缺失 / 权限不足 / 锁中毒 四态 + 滑块脏检查 + 轮询停启 + IPC 参数形状 | 2026-10-24 |
| 可访问性 | ✅ 静态确认 | `:focus-visible` 覆盖按钮/滑块/tab；`role="status" aria-live` 状态栏；`role="alert"` 告警区；`prefers-reduced-motion` 降级 | 2026-10-24 |
| 界面真实渲染（真机窗口） | ✅ 已验证 | 截图 `docs/screenshots/main.png`（显示真实 EC 数据，v0.1.0 时采集） | 2026-10-17 |
| 界面渲染（浏览器替代验证） | ✅ 已验证 | `docs/screenshots/ui-ready.png`、`ui-ec-unavailable.png`、`ui-need-admin.png`；规格见 `docs/qa/ui-smoke-spec.md` | 2026-10-24 |
| 独立复审（reviewer 子代理） | ✅ 已验证 | 独立跑 `core` 46 / `app` bin 4 / 契约 8 / 两个 crate 的 fmt+clippy；产出 4 条 Required Changes（R1 限速锁未覆盖 IOCTL、R2 限速状态实例级、R3 台账超范围、R4 注释矛盾），**已全部修复** | 2026-10-24 |
| 前后端端到端（UI 自动化） | 🚫 禁用 | 见 `docs/SPEC.md` §5.1（避免反复启停驱动硬件） | — |

### IPC 契约测试覆盖

| 测试 | 作用 |
|---|---|
| `camel_case_conversion_matches_tauri` | 转换规则与 Tauri 一致 |
| `frontend_and_backend_command_sets_match_exactly` | 手写两表双向一致 |
| `frontend_source_ipc_table_matches_backend` | **解析前端真实源码**的 `IPC` 表，与后端命令集双向比对 |
| `no_raw_string_command_names_in_frontend` | 禁止绕过 `IPC` 表直接传字符串命令名 |
| `manual_contract_matches_source_ipc_table` | 手写参数表与源码契约表不漂移 |
| `parameter_names_match_with_camel_case_conversion` | 逐参数名匹配 |
| `no_snake_case_leaks_to_contract` | 前端参数不得含下划线（Tauri v2 静默失败点） |
| `command_names_are_lowercase_identifiers` | 命令名合法性 |

---

## 3. 硬件协议（真机）

| 项 | 状态 | 证据（日期） |
|---|---|---|
| EC 设备可访问 | ✅ 已验证 | `\\.\ACPIDriver` 打开成功（2026-10-17） |
| 关键寄存器读数合理 | ✅ 已验证 | 项目 ID=16、Gaming 75/75/200、Office 45/45/200 |
| 风扇实时采样 | ✅ 已验证 | 3970→3506 RPM 随负载变化（2026-10-17） |
| 档位三档切换 + 复原 | ✅ 已验证 | 均衡/办公/狂暴均读回一致，实验后完全复原 |
| 功耗写入 + 回滚 | ✅ 已验证 | Office PL1 45→50→回滚→45 |
| 全量扫描基线 | ✅ 已验证 | 4096 地址、2002 非零（2026-10-16） |
| EC 健康自检 | ✅ 已验证 | `tools/ec_health.py` → EC 正常，519 次读 0 失败 |
| 温度寄存器 | ❓ 待验证 | 未确认（见第 5 节） |
| 压力测试（CPU/GPU 满载、EC 泛洪） | 🚫 禁用 | 曾导致硬复位，见 `docs/SPEC.md` §5.1 |

**真机验证频率建议**：仅在你主动要求且机器空闲时做，优先用 `ec_health.py`（限速只读）。

---

## 4. 工具脚本（`tools/`）

| 脚本 | 状态 | 备注 |
|---|---|---|
| `ec_health.py` | ✅ 已验证 | 限速 50 次/秒，只读，输出结论 |
| `ec_read.py` | ✅ 已验证 | 读预置表或指定地址 |
| `ec_scan.py` | ✅ 已验证 | **已加限速**（默认 50 次/秒，上限 100） |
| `ec_verify.py` | ✅ 已验证 | 与官方注册表交叉核对 |
| `ec_probe.py` | ✅ 已验证 | 连接探测 |
| `ec_mode_exp.py` | ✅ 已验证 | 档位实验（`finally` 强制回滚） |
| `ec_stress.py` | ✅ 已验证 | **已加限速**（默认 20 次/秒，上限 100） |
| `acpi_dump.py` / `acpi_search.py` | ✅ 已验证 | 导出并搜索 ACPI 表（找到 INOU0000 在 DSDT 中） |
| `aml_ecmap.py` / `aml_inspect.py` | ✅ 已验证 | 解析 EC 操作区与设备定义 |
| `compare_drivers.py` | ✅ 已验证 | 三版驱动绑定同一硬件 ID |
| `parse_reg.py` | ✅ 已验证 | 解析官方能力表 |

## 5. 诊断脚本（`scripts/`）

| 脚本 | 状态 | 用途 |
|---|---|---|
| `ec_health.py`（即 `tools/ec_health.py`，无 ps1 版本） | ✅ | EC 健康 |
| `hw_health.ps1` | ✅ 已验证 | 温度/电源/风扇/EC 快照 |
| `diagnose_reboot.ps1` | ✅ 已验证 | 重启原因（Event 41/6008/1001/WHEA） |
| `enum_drivers.ps1` | ✅ 已验证 | 驱动包枚举 |
| `probe_runtime.ps1` | ✅ 已验证 | 官方进程运行态采集 |
| `uia_dump.ps1` | ⚙️ 静态确认 | UIA 树导出（WebView2 内容需 `--force-renderer-accessibility`） |
| `screenshot_window.ps1` | ⚠️ 部分可用 | 受 DPI 影响需全屏截图，见下 |
| `e2e_ui.ps1` | 🚫 禁用 | 真机 UI 自动化，按安全策略不做 |

**已知坑**：`screenshot_window.ps1` 用 `GetWindowRect` 返回的是**虚拟化坐标**
（PowerShell 自身 DPI 不感知），截图会偏移。诊断 UI 请用全屏截图
（`CopyFromScreen(0,0,w,h)` 覆盖整屏）。

---

## 6. 文档

| 文档 | 状态 | 最后同步 |
|---|---|---|
| `README.md` | ✅ | 2026-10-17 |
| `docs/SPEC.md` | ✅（含 §5.1 限速要求） | 2026-10-17 |
| `docs/ARCHITECTURE.md` | ✅ | 2026-10-17 |
| `docs/DEPLOYMENT.md` | ✅ | 2026-10-17 |
| `docs/TROUBLESHOOTING.md` | ✅ | 2026-10-17 |
| `docs/CONTRIBUTING.md` | ✅ | 2026-10-17 |
| `docs/protocol/ec-access.md` | ✅ | 2026-10-17 |
| `docs/REQUIREMENTS-TRACEABILITY.md` | ✅ | 2026-10-17 |
| `workflow_status.md` | ✅（含意外重启事件记录） | 2026-10-17 |

---

## 7. 待办 / 已知缺口

| 项 | 状态 | 说明 |
|---|---|---|
| 温度寄存器 | ❓ 待验证 | 差分实验因机器硬复位中止；需在安全前提下重做（**限速 + 不施加 CPU/GPU 满载**，可只做低速空闲采样观察） |
| 风扇曲线 / 电压 / MUX | ❓ 未实现 | 🔴 高风险，需先评审 + 用户逐项确认 |
| 自动更新 | ❓ 未实现 | 方案见 `docs/DEPLOYMENT.md` §4；需先决定托管方式与签名密钥 |
| 远机型兼容性 | ❓ 待验证 | 仅在本机（旷世16 Pro GM6PQ0X）验证过 |
| 长时运行稳定性 | ⚙️ 静态确认 | 未跑长时 soak（故意不做，见安全策略） |

---

## 8. 如何更新本文件

改动代码后，**必须**更新对应行的「最后改动」为当天日期，并把「状态」从
`✅ 已验证` 改回 `❓ 待验证`（除非你已重跑验证）。然后重跑受影响的验证项，
把命令与结果填进「证据」列。

**目的**：下一个人（或下一个会话的 AI）打开本文件就知道：
- 哪些可以信任、不用重跑
- 哪些因为改过而必须重跑
