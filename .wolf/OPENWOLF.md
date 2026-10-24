# OpenWolf 协议适配（本仓库）

本仓库没有原 OpenWolf 文件体系，现建立**兼容记录层**，不替代既有工作流。

- 权威任务：`project_specs.md`
- 执行骨架：`workflow_status.md`、`policy.md`、`test_ledger.md`、`evidence/`
- 规则：`AGENTS.md`、`.agent/skills/`
- 记忆：`.agent/memory/`、本目录下的 `cerebrum.md`、`anatomy.md`、`memory.md`、`buglog.json`

## 开工顺序

1. `AGENTS.md` 与 `project_specs.md`
2. `.agent/memory/verified-scope.md`、`known-issues.md`
3. `docs/VERIFICATION-LEDGER.md`、`docs/SPEC.md`（§5.1 安全红线）
4. 相关协议基线、技能、本目录记忆

读取后先判断是否过时；不要因历史文档写着“完成”就跳过实际验证。

## 留痕

- 重要行动后追加 `memory.md` 单行记录；bug、测试失败或修复错误后追加 `buglog.json`。
- 新增/删除/重命名文件后更新 `anatomy.md`。
- 任何“通过”必须有真实命令、退出码和输出；不把 mock 或静态检查称为真实硬件验证。
- 禁止 CPU/GPU 满载压测、EC 泛洪、真机 UI 自动化和 force push。
