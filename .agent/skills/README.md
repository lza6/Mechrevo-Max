# 项目技能索引

> 每个技能 = 适用场景 + 前置读取 + 步骤 + 验收标准 + 证据要求 + 回滚方式 + 常见坑。
> 与 `~/.claude/skills/`（用户级通用技能）互补：这里的技能都是**本项目专用**的事实与坑。

| 技能 | 什么时候用 |
|---|---|
| `add-new-hardware-feature.md` | 要读/写一个新的 EC 寄存器（替代通用 add-new-feature） |
| `add-new-api.md` | 增删/改名一个 Tauri IPC 命令 |
| `verification-matrix.md` | 任何声称"完成/修复/通过"之前 |
| `release-checklist.md` | 打 tag / 建 Release / 宣布版本 |
| `incident-rollback.md` | 用户报"设置乱了 / 档位不对 / 机器重启过" |
| `code-review-checklist.md` | 提交前自审，或启动独立审查代理 |
| `project-audit-sop.md` | 要把项目当"即将交付产品"重审一遍 |
| `high-concurrency-checklist.md` | 改动触及 EC 访问频率/轮询/并发 |
| `plan-doc-audit.md` | 判断哪些计划/清单类 md 可以归档删除 |

## 技能路由优先级
项目内专用脚本（`tools/`）> 项目已用框架与测试（cargo / Tauri）> 项目已有 MCP/CLI > 通用工具 > 联网安装。

## 优先级规则（多技能同时适用时）
1. 安全类（`verification-matrix`、`high-concurrency-checklist`）**永远先行**
2. 涉及写硬件 → `add-new-hardware-feature` 覆盖通用流程
3. 交付前 → `code-review-checklist` → `verification-matrix` → `release-checklist`
