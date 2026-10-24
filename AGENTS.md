# AGENTS.md —— 本仓库的代理工作规则

> 适用于任何 AI / 新同事接手本仓库。**读完这份再动代码。**
> 与 `docs/SPEC.md` 冲突时以 SPEC 为准；与 `docs/CONTRIBUTING.md` 重复处不赘述。

---

## 0. 开工前必读（顺序固定，别跳）

1. `project_specs.md` —— 任务状态权威源（里程碑 / 当前迭代 / 待决策项）
2. `.agent/memory/verified-scope.md` —— **哪些验证可信任、不必重跑**
3. `.agent/memory/known-issues.md` —— 已知问题与未关闭项
4. `docs/VERIFICATION-LEDGER.md` —— 每项验证的状态、"最后改动"日期
5. `docs/SPEC.md` —— 做什么 / **明确不做什么** / §5.1 安全红线
6. `docs/ARCHITECTURE.md` §7 —— 新增硬件功能的固定流程
7. `docs/protocol/ec-baseline.md` —— 已入库的协议基线（脱敏、可复核）
8. `.agent/skills/` —— 按任务类型取对应技能

读完先判断**是否过时**：过时就更新再干活；没过时就跳过重复劳动。

## 1. 三条安全硬约束（违反即停）

1. **EC 访问必须限速**：限速在 `WindowsAcpiTransport::ioctl` 内部执行（默认 50 次/秒，硬下限 100 次/秒）。**不得**绕过、不得为提高刷新率调高频率。
2. **禁止硬件压测**：不做 CPU/GPU 满载压测，不做 EC 泛洪。本项目的"压测"= 长时**低速**轮询 / 写入与复原往复 / 设备掉线恢复 / 边界拦截。
3. **禁止真机 UI 自动化 E2E**（用户明确指示）：验证以 mock 单测 + 静态检查 + 源码级契约为主；真机只做低频**只读**自检。

> 背景：2026-10-17 本机发生过一次无转储硬复位（Event 41 Kernel-Power），开发期的高频 EC 读取与满载压测都是可疑触发因素。

## 2. 编码前

- 先读代码再动手；**优先复用**仓库已有实现（`core/src/` 的分层与 `tools/` 的脚本），禁止重复造轮子。
- 新功能先改 `docs/SPEC.md`（规格基线），再改代码。
- 新寄存器必须**实机验证**，不得只信社区文档（反例：社区称 `0x0783` 是"当前 PL"，本机恒为 0）。
- 明确写出：目标 / 范围 / 关键假设 / 验证方式。存在多种解读时全部摆出来，不沉默选择。

## 3. 写代码时必须遵守的不变量

| 不变量 | 保证方式 |
|---|---|
| 读失败必须 `Err`，不得返回 0 | `Error::Ioctl` |
| 写入前必须 `snapshot` | `Ec::snapshot` |
| 写入必须读回校验 | `Ec::write_verified`（`write_unverified` 需注释说明理由） |
| 失败必须自动回滚 | `control.rs` / `mode.rs` 的模式 |
| 会写硬件的路径必须 panic 也回滚 | `Ec::guard` + `RestoreGuard` |
| EC 访问必须串行化 | `AppState.ec` 互斥（锁顺序：`ec` → `undo`） |
| 新寄存器必须标 `AddrStatus` | `namespace.rs` |
| 公开 API 必须有文档注释 | `#![warn(missing_docs)]` |
| IPC 命令名必须走前端 `IPC` 表 | `ipc_contract.rs` 会解析 `app/ui/app.js` 并双向比对 |

**不要做**：为单次使用造抽象；添加未要求的可配置性；用 `unwrap()/expect()` 处理可恢复错误（测试除外）；绕过 `EcTransport` 直接调 IOCTL；大幅重构没坏的东西。

## 4. 验收与证据（反伪闭环）

1. 未实际运行 ≠ 通过。声称"测试通过/构建成功/修复完成"必须附**本次实际执行的命令 + 真实输出**。
2. 结论必须打标签：`已验证`（实际运行观察）/ `静态确认`（读代码）/ `合理推断` / `待验证`。
3. 验证分层按风险选，不机械全跑：只改文档 → 查引用；只改 core → `test`+`clippy`+`fmt`；改 IPC/前端 → 追加 `--bin` + `ipc_contract` + `node --check`；发版 → `docs/DEPLOYMENT.md` §7 全清单。
4. **禁止**通过删除测试、跳过测试、放宽断言、mock 真实业务逻辑、注释功能来"让验证通过"。
5. 每完成一个任务写 `evidence/<task_id>.json`，`raw_output_tail` 必须是真实输出，不得改写成整洁摘要。

## 5. 审查

- 每个节点进入 Review 必须有审查记录；`Approve` 只代表"未发现阻塞性问题"，不代表"代码完美"。
- 审查角色只读、不继承实现者的推理作为结论依据；第 3 轮仍无实质进展时停下并报告阻塞。
- 审查清单见 `.agent/skills/code-review-checklist.md`。

## 6. 留痕与回滚

- 每个写操作、每次发布都要能回滚：代码用独立提交（可 `git revert`），硬件用快照 + `RestoreGuard`，版本用上一 tag 的绿版 exe。
- 改动的文件、行为变化、影响范围、验证方式、回滚方式，写进 `workflow_status.md` 或 `evidence/`。
- 不可逆操作（删数据、发布、force push）需留备份或明确授权；**禁止 `git push --force`**。

## 7. 文档同步（同一次提交内完成）

改了代码就同步：`README.md`、`docs/SPEC.md`、`docs/ARCHITECTURE.md`、`docs/protocol/`、`docs/VERIFICATION-LEDGER.md`、`project_specs.md`、`.agent/memory/change-index.md`。禁止"文档写理想状态、代码是另一状态"。

## 8. 计划文档的纪律

- 计划/改进指南类文档每轮结束要更新状态；**含未决策项或未完成里程碑的不得删除**（见 `.agent/skills/plan-doc-audit.md`）。
- 删除前必须归档到 `docs/archive/plans/YYYYMMDD/` 并生成 `archive-manifest.md`。
