# 归档清单 · 2026-10-25

> 执行依据：独立文档审计（39 个已跟踪 Markdown）。本目录是**备份，不是删除证据**。

## 归档对象

| 项 | 值 |
|---|---|
| 原路径 | `docs/计划书/闭环审计-20261024.md` |
| 归档路径 | `docs/archive/plans/20261025/闭环审计-20261024.md` |
| 类型 | 已结束的单轮计划文档审计（历史证据） |
| 闭环状态 | 该次审计已结束；其扫描数、未闭环统计属历史快照，**不代表当前项目全部闭环** |
| SHA256（备份） | `7d15a3d50182431c9ce86854f315dbb28bfda469a1e2335c4c27e0ec64b6dd61` |
| 备份校验 | `cmp 原文件 备份文件` 完全一致（`ARCHIVE_COPY_IDENTICAL=1`） |
| 审计证据 | `evidence/audit-2026-10-25-repair.json`；独立 doc-auditor 结论见 `workflow_status.md` |

## 处置

- 备份并核对一致后，原件移除；历史内容保留在本目录，**不是无备份删除**。
- 当前没有其它可删除的计划文档。`下一步改进指南.md` 仍含多机型、产品化、高风险写入、未统一决策项，保留。
- `project_specs.md`、`test_ledger.md`、`docs/VERIFICATION-LEDGER.md` 是活跃权威台账，必须同步而非归档。
- `docs/REQUIREMENTS-TRACEABILITY.md`、`workflow_status.md`、`tasks/weekly.md` 保留为需求/历史证据。

## 回滚

从本目录复制回原路径，或恢复本次归档提交：

```bash
cp docs/archive/plans/20261025/闭环审计-20261024.md docs/计划书/闭环审计-20261024.md
# 或
git revert <archive-commit-sha>
```

不执行 `git reset --hard`、`git clean`、强制推送。
