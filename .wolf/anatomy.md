# Anatomy（文件导航，优先按描述查阅）

## 权威入口

- `project_specs.md`：任务状态权威源；仍需核对证据，不能盲信历史 done。
- `AGENTS.md`：仓库规则与 EC 安全约束。
- `docs/SPEC.md`：功能与非目标、§5.1 安全红线、采样降级边界。
- `workflow_status.md`：本轮执行、验证与审查记录（只追加）。
- `test_ledger.md`：接缝、复测状态。
- `evidence/`：实际命令与输出摘要，非所有任务都已完成。
- `docs/adr/0001-sampling-decoupled-from-ec.md`：外部采样与 EC 解耦的决策。
- `.agent/skills/`、` .agent/memory/`：既有技能和验证/问题/决策记忆。
- `.wolf/`：OpenWolf 兼容协议、文件索引、跨会话学习、bug 记录。

## 代码

- `core/src/ec/`：EC 传输、进程级限速、Windows 驱动。修改须复测并发/边界。
- `core/src/control.rs`：功耗范围与写入回滚、固件基线。
- `core/src/mode.rs`：档位与强冷读改写、偶发丢写重试。
- `core/src/sensors.rs`：温度/风扇/项目/电源读数。
- `core/src/fancurve.rs`：曲线表只读分类，不证明官方配对。
- `app/src-tauri/src/main.rs`：IPC、锁顺序、状态快照、诊断元数据。
- `app/src-tauri/src/sysinfo.rs`：免驱采集；超时、解析、缓存、未知值。
- `app/src-tauri/src/profiles.rs`：配置持久化与坏文件降级。
- `app/src-tauri/src/logging.rs`：release 日志与 panic hook。
- `app/ui/`：7 页原生 HTML/CSS/JS；`app.js` 的 IPC 表是命令契约源之一。
- `tools/qa_browser.cjs`：独立 Chromium + mock IPC 的前端验收，不是硬件 E2E。

## 发布与验证

- `docs/DEPLOYMENT.md`：构建、发布与回滚清单。
- `docs/qa/ui-smoke-spec.md`：浏览器验收边界与本轮断言。
- `.github/workflows/ci.yml`：verify/coverage/tag package。
- `tools/ec_health.py`：限速只读健康检查。
- `docs/screenshots/`：截图证据；浏览器 mock 与真实窗口不得混淆。

## 历史与归档

- `docs/计划书/`：路线图，含未闭环项，禁止为整洁删除。
- `docs/archive/plans/20261025/`：已结束单轮审计的备份与 manifest。
- `docs/REQUIREMENTS-TRACEABILITY.md`：历史需求矩阵，顶部有当前状态校正。
