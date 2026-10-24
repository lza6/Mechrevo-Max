# 周报

> 追加式记录，最新在上。

---

## 2026-10-24（下半场）· v0.1.1 护栏补丁落地

### 完成（全部离线可验，零硬件风险）

| 项 | 结果 |
|---|---|
| EC 访问串行化 | `AppState.ec` 互斥；`lock_ec_serializes_access` 断言临界区并发数恒为 1 |
| 限速护栏 | `AccessThrottle` 挂在 `WindowsAcpiTransport::ioctl()` 唯一出口；默认 50 次/秒，硬下限 100 次/秒 |
| RAII 回滚守卫 | `Ec::guard()` + `RestoreGuard`；`guard_restores_after_panic` 用 `catch_unwind` 验证 |
| undo 不丢记录 | 改为"撤销成功才出栈"，失败提示可重试 |
| 证据链 | 新增 `docs/protocol/ec-baseline.md`（脱敏）；改 5 处引用 |
| 契约测试盲区 | 前端 `IPC` 表 + 后端解析源码双向比对；禁裸字符串 `invoke` |
| 文档过期 | 修正 4 处；发布清单加"文档一致性自检"与合规命令 |
| 清理 | 删 3 个死依赖、2 个空目录；`graft/` 入 ignore |
| 可访问性 | `:focus-visible`、`aria-live`、`role=alert`、`prefers-reduced-motion` |
| 沉淀 | `AGENTS.md` + 9 个技能 + 7 个记忆文件 + `evidence/v0.1.1.json` + HTML 变更报告 |

### 验证（实际执行）

`cargo test` 44 passed + 1 doctest · `clippy` 0 warning · `fmt --check` 干净 ·
`cargo test --bin mechrevo-max` 4 passed · `cargo test --test ipc_contract` 8 passed ·
`cargo check --all-targets` 通过 · `node --check` 通过 · `cargo build --release` 成功（exe 4 992 512 B）

### 未执行（及原因）

真机硬件测试（安全策略）· GUI E2E（用户禁用）· 覆盖率（无工具）

### 发布（已完成）

`git push origin main` → `898c4c2..1fe2a14`；tag `v0.1.1` 已推送；
`tools/publish_release.py v0.1.1` 创建 Release 并上传产物；
API 复核 `draft=false`、asset `mechrevo-max.exe` **4 992 512 B**（与本地构建一致）。
注：`curl` 访问 `api.github.com` 返回 `http_code=000`，但 Python urllib + git 凭据可正常访问 —— 该环境的 `curl` 不可用于 GitHub API 验证。

### 计划文档审计

扫描 32 个 md → **0 个可删除**（指南仍含 11 个未闭环任务 + 3 个未决策项）。审计记录：`docs/计划书/闭环审计-20261024.md`

### 下一步

v0.2.0 温度读数（需用户在场的低频只读探测）；Q-03 / Q-04 / Q-05 待拍板。

---

## 2026-10-24（上半场）· 仅文档交付：改进指南

### 完成

- 以**仅文档交付模式**完成全仓库只读复审，产出 `docs/计划书/下一步改进指南.md`（现状基线 / 27 条分级问题 / 4 方案对比 / 6 段版本路线 / 22 项原子任务 / 验收矩阵 / 门禁 / 清理清单 / 待执行验收项 / AI 提示词）。
- 复核 `docs/VERIFICATION-LEDGER.md` 的 4 条关键声明，**全部如实复现**：33 单测通过、clippy 0 warning、fmt 干净、IPC 契约 5 个通过、`node --check` 通过。
- 新建 `project_specs.md` 作为任务追踪权威源（此前由台账兼任，已出现过期漂移）。

### 关键发现（详见指南 §6）

| 级别 | 发现 |
|---|---|
| P0 | 真机测试在断言失败时不会回滚硬件（`core/tests/hardware.rs:106-172`） |
| P0 | 公开仓库证据链断裂：`reverse/` 下 0 个跟踪文件，却有 5 处文档引用它 |
| P1 | EC 访问未串行化，2 秒轮询与写入路径可重叠（`main.rs` 无 EC 互斥） |
| P1 | Rust 侧完全没有限速护栏，`docs/SPEC.md` §5.1 的硬性要求只落在 Python 工具里 |
| P1 | `undo_power` / `undo_mode` 失败时把回滚记录丢弃 |
| P1 | 温度读数缺失（控制台类产品核心信息） |

### 未做（本轮边界）

未改代码、未跑真机测试、未启 GUI、未删文件、未提交/推送、未安装依赖。GitHub Release 页面因本机无网络未能复核。

### 下一步

用户已于 2026-10-24 拍板两项（已回写指南 §7.3 / §17 与 `project_specs.md` §4）：

- **Q-01 → 温度读数优先**：执行顺序固定为 v0.1.1（护栏）→ v0.2.0（温度）→ v0.2.1（工程化）→ v0.3.0（多机型，仍 blocked 于第二台机器）→ v0.4.0（产品化）。
- **Q-02 → 提交脱敏协议基线文档**：`docs/protocol/ec-baseline.md` 入库，原始 `ec-scan.json` 继续不入库（落在 T-011-06）。

仍未拍板：Q-03（`ec_stress.py` 去留）、Q-04（自动更新）、Q-05（托盘/自启）。

下一步进入 **v0.1.1 实施**（护栏补丁，全部离线可验），需切换到执行模式后开工。

---

## 2026-10-24（收尾）· v0.1.2 独立复审修复

### 复审结论
reviewer 子代理独立跑验证后给出 **CONDITIONAL PASS**，指出 v0.1.1 两条核心声明只成立一半：

| 编号 | 问题 | 修复 |
|---|---|---|
| R1（P1） | `AccessThrottle::wait()` 释放锁后 IOCTL 才执行 → 单次 IOCTL 慢于 20ms 时两次访问会重叠；同名测试只断言总耗时，发现不了 | 改为 `run(|| ...)` 整个访问持锁；新增 `run_never_allows_overlapping_access` 断言峰值并发=1 |
| R2（P1） | 闸门是 transport 字段，每条命令 `new()` → 每条命令首次访问零间隔，新路径可绕过 | 改为进程级 `global()` 单例；新增指针相等断言 |
| R3（P2） | 台账把 clippy/fmt 记成无条件通过，实际只跑 `core`；`app` fmt 失败、clippy 3 警告 | 修掉 3 个 lint + `cargo fmt`；台账标注 scope 为两个 crate |
| R4（P2） | `mode.rs` 注释称撤销会写提交寄存器，实际不写 | 注释与实现对齐 + 标注"是否需补提交未验证" |

### 验证（实际执行）
`core` 46 passed + 1 doctest · `bin` 4 passed · 契约 8 passed · **两个 crate** 均 fmt 干净 + clippy 0 warning · release 构建成功（4 994 048 B，sha256 `c85a29ea…f33363`）

### 发布（已完成）
tag `v0.1.2` 已推送；[Release v0.1.2](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.1.2) 已创建并上传
`mechrevo-max.exe`（4 994 048 B）。API 复核：v0.1.0 / v0.1.1 / v0.1.2 三项均 `draft=false`，
asset 大小与各自本地构建一致，`latest` = v0.1.2。

### 未处理（已记入 known-issues P3-11/12/13）
`get_status` ≈280ms 持锁（未测帧率）· 守卫 Drop 失败仅 stderr（release 无去处）· 契约参数名仍手写
