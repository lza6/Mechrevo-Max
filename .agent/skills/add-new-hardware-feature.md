# Skill: 新增硬件功能（本项目专用；替代通用 add-new-feature / add-new-api）

## 适用场景
要读/写一个新的 EC 寄存器或新增一个 IPC 命令。

## 前置读取
`docs/ARCHITECTURE.md` §7、`docs/SPEC.md` §3.3（功能完整性规则）、`docs/protocol/ec-access.md`、`docs/protocol/ec-baseline.md` §6（未验证项）

## 步骤（缺一不可，顺序不可跳）
1. **确认寄存器**：`tools/ec_read.py` 单点读 + `tools/ec_scan.py --delta` 找动态值；**必须实机验证**，不采信社区文档（反例：`0x0783`–`0x0786` 本机恒为 0）
2. **登记** `core/src/namespace.rs`：带 `AddrStatus`（Verified / CommunityUnverified / NotApplicable）与 `note`（来源 + 验证方式）
3. **实现**：只读放 `sensors.rs`；写操作新建模块或放 `control.rs`，必须 `snapshot` → `write_verified` → 失败自动回滚
4. **补单测**（`MockTransport`）：正常路径 + `with_stuck` 失败路径 + `with_failing_read` + 边界值
5. **暴露 IPC**：`main.rs` 加命令（持 `AppState.ec` 锁）→ 前端 `app.js` 加进 `IPC` 表 → 同步 `ipc_contract.rs` 的 `BACKEND_COMMANDS` 与 `FRONTEND_CONTRACT`
6. **前端**：UI + 成功/失败/进行中反馈 + 撤销路径
7. **文档**：`docs/protocol/`（新增地址）、`docs/SPEC.md` §3、`docs/VERIFICATION-LEDGER.md`

## 验收标准
6 项齐全（core 实现 / 单测 / 真机测试标注 / UI 入口 / 撤销路径 / 协议文档），缺一不算完成。

## 证据要求
单测输出；真机读数（若做过）；UI 截图（若改了界面）。

## 回滚方式
寄存器变更可回滚（快照）；代码回滚 = git revert 该提交。

## 常见坑
- 🔴 **高风险写操作**（风扇曲线 / 电压 / MUX）必须先单独评审 + 用户逐项确认
- 强冷位与档位共用 `0x751`：必须读改写，不能整字节覆盖
- `0x0767` 写后自清零：不可读回校验，也不进快照
