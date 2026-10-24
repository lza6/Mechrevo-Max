# Skill: 验证矩阵

## 适用场景
任何声称"完成/修复/通过"之前。

## 前置读取
1. `docs/VERIFICATION-LEDGER.md` —— 哪些可信任、哪些改过必须重跑
2. `docs/DEPLOYMENT.md` §7 —— 发布检查清单
3. `docs/SPEC.md` §5.1 —— 安全红线（限速/禁压测/禁真机 E2E）

## 步骤（按风险选，不机械全跑）
| 风险 | 必跑 |
|---|---|
| 只改文档 | 引用文件是否存在（逐条 `ls`） |
| 只改 `core/` 逻辑 | `cargo test` + `clippy --all-targets` + `fmt --check` |
| 改 IPC/前端 | 上述 + `cargo test --bin mechrevo-max` + `cargo test --test ipc_contract` + `node --check app/ui/app.js` |
| 改真实硬件路径 | 上述 + 评审 `git diff` 确认每处写都有 snapshot/校验/回滚 + 一次低频只读 `tools/ec_health.py` |
| 发版 | 全部 + `cargo build --release` + `docs/DEPLOYMENT.md` §7 全清单 |

## 验收标准
每条验证都要能贴出**命令 + 真实输出摘要**；未执行的必须写明原因。

## 证据要求
写进 `evidence/<task_id>.json`：`cmd`、`exit_code`、`raw_output_tail`；禁止填写猜测输出。

## 回滚方式
验证本身不应改动工作区；若跑了 `cargo build`，回滚 = 无需处理（`target/` 已 ignore）。

## 常见坑
- **测试不要断言"某台机器的具体硬件事实"**（2026-10-25 连红两次的教训）：CI 是虚拟机，
  可能没有真电池（`BatteryStatus` 类存在但容量为 0）、`Get-PhysicalDisk` 的 FriendlyName 可能为空。
  解析逻辑用**固定 JSON fixture** 测；真机只做"跑得通、不 panic"的 smoke。
- 只读系统信息（CPU/GPU/电池/SSD）全部免驱、不经 EC，改这层不需要动 EC 限速逻辑，但要注意别在轮询里拉起进程（缓存 ≥3s/60s）。

- `cargo test --test ipc_contract` **不编译** `main.rs`；必须另跑 `cargo check --all-targets` 或 `cargo test --bin mechrevo-max`
- clippy 输出被缓存时会只打印 `Finished`；以"是否有 `warning:` 行"判定，不要以耗时判定
