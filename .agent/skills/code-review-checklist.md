# Skill: 代码审查清单（本项目）

## 适用场景
任何代码改动提交前，或启动独立审查代理时。

## 前置读取
`docs/CONTRIBUTING.md` §2.2 不变量、`docs/ARCHITECTURE.md` §3 关键决策。

## 必查项（有罪推定）
1. **失败是否被吞**：读失败返回 0？`let _ = ...` 丢掉 Result？
2. **写路径完整性**：snapshot → write_verified → 失败回滚，三者是否齐全？
3. **锁**：`AppState.ec`（EC 访问）与 `AppState.undo`（撤销栈）获取顺序是否处处一致（ec → undo）？持锁期间有没有长耗时操作或再次取锁？
4. **限速**：新代码有没有绕过 `WindowsAcpiTransport::ioctl` 直接调 `DeviceIoControl`？
5. **panic 安全**：会写硬件的路径是否持有 `RestoreGuard`？
6. **错误可行动**：错误文案是否告诉用户下一步（驱动命令 / 提权 / 重启）？
7. **IPC 契约**：新命令是否同时进了 `app.js` 的 `IPC` 表与 `ipc_contract.rs` 两张表？有没有裸字符串命令名？
8. **测试**：是否只断言 `is_ok()`？是否覆盖 stuck / 读失败 / 边界？是否依赖 sleep 时序？
9. **文档同步**：受影响文档是否同一次提交内更新？
10. **安全**：无硬编码密钥；CSP 未放宽；未引入远程资源；无 `unwrap()` 在可恢复路径。

## 严重程度
阻塞（安全/数据/逻辑/竞态/验证伪造）> 需要改进 > 强烈建议 > 备注。

## 验收标准
输出 `Summary / Critical Issues / Required Changes / Suggestions / Verdict`，每条带 `file:line` 与复现方式；`Approve` 仅代表"未发现阻塞性问题"。

## 证据要求
审查结论写入 `workflow_status.md` 的审查区块；修复后必须复验并记录轮次。

## 回滚方式
审查不改代码（只读角色）；修复问题用独立提交。

## 常见坑
- 不要用"实现者自己的判断"替代独立审查
- 不要重复已解决的项；关注改进幅度
