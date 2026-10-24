# Skill: 新增 / 修改 IPC 命令（Tauri v2）

## 适用场景
需要在前端增删一个后端命令，或修改命令名/参数名。

## 前置读取
`docs/ARCHITECTURE.md` §5（数据流）、`app/src-tauri/tests/ipc_contract.rs`、`app/ui/app.js` 顶部的 `IPC` 表。

## 步骤（四处必须同步，缺一处就会静默失败）
1. `app/src-tauri/src/main.rs`：加 `#[tauri::command]` 函数，写进 `invoke_handler![...]`；**若涉及 EC 访问，函数体必须先取 `lock_ec(&state)`**
2. `app/ui/app.js`：把命令名加进 `IPC` 表，调用处写 `invoke(IPC.xxx)`（**禁止**直接传字符串）
3. `app/src-tauri/tests/ipc_contract.rs`：把命令与参数加进 `BACKEND_COMMANDS` 与 `FRONTEND_CONTRACT`
4. 前端 UI：入口 + 成功/失败/进行中反馈 + `setBusy` 包裹

## 验收标准
`cargo test --test ipc_contract` 8 个全绿；`cargo test --bin mechrevo-max` 全绿；`cargo check --all-targets` 通过（它会真正编译 `main.rs`）。

## 证据要求
两条测试的输出摘要；若有 UI 改动，附界面截图或说明未截图原因。

## 回滚方式
`git revert` 该提交；契约测试会立刻发现半更新状态。

## 常见坑
- Tauri v2 把 Rust 的 `snake_case` 参数名转成 `camelCase`，前端写 `max_watts` 会**反序列化失败且无报错**（`no_snake_case_leaks_to_contract` 就是拦这个）
- `cargo test --test ipc_contract` 不编译 `main.rs`，别以为绿了就代表命令层没问题
- 命令面板里出现"后端注册但前端没人调"会被 `frontend_source_ipc_table_matches_backend` 判定为死命令
