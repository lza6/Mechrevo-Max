# 项目任务追踪（权威源）

> 本文件是**任务状态**的单一权威来源。之前由 `workflow_status.md` 与 `docs/REQUIREMENTS-TRACEABILITY.md` 兼任，二者已出现过期漂移（见 `docs/计划书/下一步改进指南.md` §1.4）。
>
> 规则：
> 1. 只有**观察到可复现的交付证据**才允许改状态为 `done`；
> 2. 状态取值：`todo` / `doing` / `done` / `blocked` / `n/a`（附理由）；
> 3. 每次状态变更必须写 `证据` 列（命令输出、文件路径、截图、Release URL）；
> 4. 验证细节仍在 `docs/VERIFICATION-LEDGER.md`，本文件不重复。
>
> 最后更新：2026-10-24

---

## 1. 项目身份

| 项 | 值 |
|---|---|
| 名称 | Mechrevo-Max（机械革命自研控制台） |
| 形态 | Windows x64 桌面应用（Rust `mechrevo-core` + Tauri v2） |
| 仓库 | https://github.com/lza6/Mechrevo-Max |
| 当前版本 | v0.4.0（固件基线 PL 可见 + 风扇曲线只读；已发布） |
| 主分支 HEAD | 见 `git log -1`（本文档自身提交后哈希会滞后一格，因此不写死）；已推送的 tag：`v0.1.0`、`v0.1.1`、`v0.1.2` |
| 规格基线 | `docs/SPEC.md`（安全红线在 §5.1） |

---

## 2. 里程碑（M）

| # | 里程碑 | 目标状态 | 验收标准 | 证据 | 状态 |
|---|---|---|---|---|---|
| M1 | v0.1.0 首个可用版本 | ✅ 已达成 | 核心链路真机验证 + 发布 | tag `v0.1.0`、`mechrevo-max.exe` 4.76 MiB | done |
| M2 | v0.1.1 护栏 + v0.1.2 复审修复 | ✅ 已达成 | P0/P1 全部离线可修项关闭；已推送并发版 | 核心 46 + bin 4 + 契约 8 全绿；tag `v0.1.2`；[Release v0.1.2](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.1.2)（asset 4 994 048 B）；证据 `evidence/v0.1.1.json`、`evidence/v0.1.2.json` | done |
| M3 | v0.2.0 可观测性（温度） | ✅ 已达成 | 与 ACPI 热区 / nvidia-smi 交叉吻合（r=0.857/0.989）；探测全程限速只读 | `evidence/v0.2.0.json`；真实 GUI 截图 | done |
| M4 | v0.2.1 工程化 | 规划 | CI 必绿 + 覆盖率基线 + 打包验证 | 待填 | todo |
| M5 | v0.3.0 多机型 | blocked | 第二台机型只读可用 + 未知机型安全降级 | 需第二台机器 | blocked |
| M6 | v0.4.0 产品化 | 规划 | 干净系统按向导走通全链路 | 待填 | todo |
| M7 | v0.5+ 高风险写入 | 规划 | 逐项评审 + 行为级有效性验证 | 待填 | todo |
| M8 | v1.0.0 稳定版 | 规划 | API 冻结 + ≥3 机型 + 文档齐备 | 待填 | todo |

---

## 3. 当前迭代任务（v0.1.1）

详见 `docs/计划书/下一步改进指南.md` §9。此处只跟踪状态。

| # | 任务 | 对应缺陷 | 优先级 | 状态 | 证据 |
|---|---|---|---|---|---|
| T-011-01 | `AppState` 加 EC 访问互斥 | P1-01 | P1 | **done** | `main.rs` `AppState.ec`；`lock_ec_serializes_access` 单测断言临界区内并发数恒为 1 |
| T-011-02 | `undo_*` 失败时保留回滚记录 | P1-02 | P1 | **done** | 改为"成功才出栈"，失败文案提示可重试 |
| T-011-03 | `AccessThrottle` 限速（唯一 IOCTL 出口） | P1-03 | P1 | **done** | `core/src/ec/throttle.rs` + `windows.rs::ioctl` 首行；6 个单测 |
| T-011-04 | 真机测试 RAII 回滚 guard | P0-01 | P0 | **done** | `Ec::guard` + `RestoreGuard`；`guard_restores_after_panic` 单测；`hardware.rs` 两处写测试改用守卫 |
| T-011-05 | busy 期间暂停轮询 | P1-01 | P1 | **done** | `app.js` `startPolling/stopPolling`、`refresh(force)` 在飞标记 |
| T-011-06 | 脱敏协议基线文档 + 改 5 处引用 | P0-02 | P0 | **done** | `docs/protocol/ec-baseline.md`；README / ec-access / workflow_status / CONTRIBUTING 引用已改 |
| T-011-07 | 修正 4 处文档过期 | P1-05 | P1 | **done** | 需求矩阵 R10/R11、`workflow_status` 下一步、工具数 12→16、`ec_health.ps1` |
| T-011-08 | 删除死依赖 `serde_json`/`tracing`/`tempfile` | P2-05 | P2 | **done** | `core/Cargo.toml`；`Cargo.lock` 净减 194 行 |
| T-011-09 | 清理空目录 + `graft/` 入 ignore | P3-04 / P3-10 | P3 | **done** | 删除 `protocol/`、`test/`；`.gitignore` 增 `graft/` |
| T-011-10 | 发布清单加文档一致性检查 | P1-05 | P1 | **done** | `docs/DEPLOYMENT.md` §7 |
| T-011-11 | 三处版本号同步 + CHANGELOG 0.1.1 | — | — | **done** | `core`/`app`/`tauri.conf.json` 均 0.1.1；CHANGELOG 已加段 |
| T-011-12 | 提交寄存器移出快照（原 P2-09） | P2-09 | P2 | **done** | `mode.rs` `MODE_REGS` 2 项；`rollback_point_excludes_commit_register` 单测 |
| T-011-13 | 契约测试解析前端源码（原 P2-01） | P2-01 | P2 | **done** | `app.js` `IPC` 表 + `parse_ipc_table` / `no_raw_string_command_names_in_frontend` |
| T-011-14 | 可访问性：焦点可见 / aria-live / reduced-motion（原 P2-04） | P2-04 | P2 | **done** | `style.css` 新增段 + `index.html` role/aria-live |
| T-011-15 | 闸门改为持锁执行整个 IOCTL（复审 R1） | R1 | P1 | **done** | `AccessThrottle::run()`；`run_never_allows_overlapping_access` 断言峰值并发=1 |
| T-011-16 | 闸门改为进程级单例（复审 R2） | R2 | P1 | **done** | `ec::throttle::global()`；`global_throttle_is_shared_across_transports` |
| T-011-17 | 台账 clippy/fmt 标注 scope + 修 app 两处 lint（复审 R3） | R3 | P2 | **done** | 两个 crate 均 fmt 干净、clippy 0 warning |
| T-011-18 | `mode.rs` 注释与实现对齐（复审 R4） | R4 | P2 | **done** | 注释改为"撤销只回写字段，是否需补提交未验证" |
| T-020-00 | 补齐执行骨架 `policy.md` / `test_ledger.md` | — | P1 | **done** | 门禁数值按项目实际填写；接缝登记 10 条 |
| T-020-01 | 前端冒烟：浏览器 + mock 后端（替代被禁的真机 E2E） | P2-02 相关 | P1 | **done** | 37/37 断言通过；`docs/qa/ui-smoke-spec.md` + 3 张截图 |
| T-020-02 | 建 CI（GitHub Actions）并核对真实运行结果 | P2-02 | P2 | **done** | run 40098051184 → conclusion=success，13 步全 success（约 240s） |
| T-020-03 | 命令改 `async fn` + `spawn_blocking`（根因：默认占 UI 线程） | P3-11 根因 | **P1** | **done** | `evidence/v0.1.3.json`；bin 单测 4→7；前端 37/37 复跑通过 |
| T-020-04 | 修正 policy 覆盖率事实 + 门禁改为可验证形式 | P3-08 | P2 | **done** | 实测 86.56%；分支覆盖在 MSVC 不可用 → 改行覆盖 ≥95% + Err 分支约定 |
| T-020-05 | release 下日志落盘 + panic hook + 失败常驻（调研 N1） | N1 | **P1** | **done** | `app/src-tauri/src/logging.rs`（按天滚动/保留 7 天/5MB 上限/失败静默）+ panic hook + `last_error` 回放；bin 单测 7→12 |
| T-020-06 | WebView2 安装模式改 skip（调研 N2） | N2 | P2 | **done** | 默认 `DownloadBootstrapper` 需联网，离线机器装不上 |
| T-020-07 | a11y 4 处修正（label/aria-valuetext/alert 常驻/tabs role） | N3 | P2 | **done** | 47 条浏览器断言含 6 条可访问性；真机读屏播报仍待验证 |
| T-020-08 | 修 `bundle.icon` 缺 `.ico`，让 msi/nsis 真能构建（关闭 P2-08） | P2-08 | P2 | **done** | CI run 40101956611 package job success；msi 1 990 656 B + nsis 1 362 587 B，已上传 Release |
| T-020-09 | CI 三 job（verify/coverage/package）+ rust-cache + 固定工具链 | P2-02 | P2 | **done** | run 40101956611 三个 job 全 success |
| T-020-10 | 温度寄存器自行测定（CPU 0x043E / GPU 0x044F） | P1-04 | **P1** | **done** | r=+0.857 / +0.989；探测工具入库；基线文档 §4.1 |
| T-020-11 | 修强冷位导致档位识别返回 None | 真机发现 | **P1** | **done** | `from_field1` 屏蔽 bit6；单测 0x40/0x50/0xE0；真机 751=0x40 显示均衡 |
| T-020-12 | 真实 GUI 端到端验证（启动→EC→UI→截图→停止） | 用户要求 | P1 | **done** | 截图 + 内存 62.6 MB + 日志落盘 |
| T-030-01 | 强冷开关（0x751 bit6，读改写+校验+回滚+撤销） | 用户要求 | P1 | **done** | 5 单测 + 真机往返 3/3 |
| T-030-02 | 固件偶发丢写 → 有界重试 | 真机发现 | P1 | **done** | 3 次「写+读回」；2 单测覆盖丢 1 次/丢 3 次 |
| T-030-03 | 自定义配置（保存/应用/删除，坏文件降级） | 用户要求 | P1 | **done** | 6 单测 + 浏览器断言 |
| T-030-04 | 寄存器浏览器（只读，前后端双校验，CSV 导出） | 用户要求 | P1 | **done** | 2 Rust 边界单测 + 4 浏览器边界用例 |
| T-030-05 | 传感器表（当前/最小/平均/最大）+ 采样频率可见 | 用户要求 | P1 | **done** | 浏览器断言 + 真实截图 |
| T-030-06 | 位域解释 + 「档位由机器决定」文案 | 用户要求 | P1 | **done** | 真实截图（751=0x40 → 均衡 + 强冷开） |
| T-030-07 | 布局修复（4 面板被裁） | 真机发现 | P1 | **done** | 右栏改竖排+整栏滚动；真实截图确认 |
| T-030-08 | 系统实际生效 PL（MSR 0x610） | 用户要求 | — | **n/a（明确不做）** | 用户态无权限，需内核驱动；HWiNFO 自带驱动 |
| T-040-01 | 显示固件基线 PL（0x783-0x786，只读） | 用户要求 | P1 | **done** | 真机 75/130/200/133；受控实验排除"生效值"假设 |
| T-040-02 | 全量差分证明写默认 PL 无 EC 连带变化 | 用户质疑 | **P1** | **done** | 4096×2 扫描差分；结论写进界面与基线文档 |
| T-040-03 | 风扇曲线只读展示（6 块 + 候选配对图） | 用户要求 | P1 | **done** | core fancurve 6 单测 + 浏览器渲染断言 |
| T-040-04 | 风扇曲线写入 | — | 🔴 高 | **todo（需先评审）** | 写错风扇可能不转→过热；须先与官方 UserFanTables 核对 |

---

## 4. 待用户决策（阻塞项）

> 2026-10-24：Q-01、Q-02 已拍板。

| # | 决策 | 阻塞什么 | 状态 | 结论 |
|---|---|---|---|---|
| Q-01 | v0.1.1 之后走哪条线 | M3–M6 的排序 | **已拍板** | 温度读数（v0.2.0）优先；顺序固定为 M2 → M3 → M4 → M5 → M6，不插入 |
| Q-02 | 证据链补法 | T-011-06 的具体形态 | **已拍板** | 提交 `docs/protocol/ec-baseline.md`（脱敏协议基线），原始 `ec-scan.json` 继续不入库 |
| Q-03 | `tools/ec_stress.py` 去留 | 策略一致性 | 待拍板 | 不阻塞 v0.1.1，延后到 v0.2.1 |
| Q-04 | 是否实现自动更新（需签名密钥 + 托管） | M6 | 待拍板 | 最早 v0.4.0 决策 |
| Q-05 | 托盘常驻 / 开机自启 | M6 | 待拍板 | 最早 v0.4.0 决策 |

### 4.1 因决策产生的执行约束

1. **v0.2.0 温度探测必须用户在场**：低速只读（≤50 次/秒）、空闲态、禁止施加任何 CPU/GPU 负载、单次会话、异常即停。
2. 温度寄存器在未与外部工具交叉核对前，`AddrStatus` 只能是 `CommunityUnverified`，且**禁止任何写入路径**引用它。

---

## 5. 硬性不变量（不可绕过）

1. EC 访问必须有代码内强制限速（默认 ≤50 次/秒，硬上限 100 次/秒）—— `docs/SPEC.md` §5.1
2. 禁止 CPU/GPU 满载压力测试；禁止真机 UI 自动化 E2E
3. 写 EC 必须：先 `snapshot` → 写入 → 读回校验 → 失败自动回滚
4. 读失败必须返回 `Err`，绝不用 0 代替
5. 新寄存器必须标 `AddrStatus`（Verified / CommunityUnverified / NotApplicable）
6. 不打包、不分发任何官方二进制 / 驱动 / 反编译产物
7. 应用运行期需要管理员权限，不得静默提权；未放宽 CSP、未引入远程内容

---

## 6. 相关文件

| 用途 | 路径 |
|---|---|
| 改进计划与问题清单 | `docs/计划书/下一步改进指南.md` |
| 规格基线 | `docs/SPEC.md` |
| 协议权威 | `docs/protocol/ec-access.md` |
| 验证台账（避免重复验证） | `docs/VERIFICATION-LEDGER.md` |
| 历史任务台账与事件记录 | `workflow_status.md` |
| 需求追踪（历史编号 R1–R23） | `docs/REQUIREMENTS-TRACEABILITY.md` |
| 周报（长期台账） | `tasks/weekly.md` |
| 已归档的任务记录 | `docs/archive/plans/20261024/`（含 `archive-manifest.md`） |
| 门禁 | `policy.md` |
| 接缝与测试台账 | `test_ledger.md` |
| 前端冒烟规格 | `docs/qa/ui-smoke-spec.md` |
