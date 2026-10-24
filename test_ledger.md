# Test Ledger —— 接缝登记与测试状态

> 规则：任何代码改动**触达**某个接缝，就把该接缝状态改为 `pending`，强制重跑对应验证；
> 单元测试通过**不能**替代接缝测试；验证通过后恢复 `locked`。
> 状态：`locked`（已验证且未改动）/ `pending`（已改动待复测）/ `unverified`（从未验证）/ `n/a`（不适用，需理由）

---

## 1. Seam Registry（接缝登记）

| # | 接缝 | 模块 A | 模块 B | 契约描述 | 最后验证 | 状态 | 关联证据 |
|---|---|---|---|---|---|---|---|
| S1 | IPC 命令集 | `app/ui/app.js`（`IPC` 表） | `app/src-tauri/src/main.rs`（`invoke_handler!`） | 命令名双向一致；禁止裸字符串 `invoke` | 2026-10-24 | **locked** | `cargo test --test ipc_contract` → 8 passed（`evidence/v0.1.2.json`） |
| S2 | IPC 参数名 | 前端调用参数（camelCase） | Rust 命令参数（snake_case） | Tauri v2 自动转 camelCase，写错静默失败 | 2026-10-24 | **locked** | `parameter_names_match_with_camel_case_conversion` / `no_snake_case_leaks_to_contract` |
| S3 | EC 访问闸门 | `WindowsAcpiTransport::ioctl` | `ec::throttle::global()` | 间隔 ≥20 ms **且**两次访问不重叠；进程级共享 | 2026-10-24 | **locked** | `run_never_allows_overlapping_access`、`global_throttle_is_shared_across_transports` |
| S4 | EC 写后读回 | `Ec::write_verified` | 驱动 `DeviceIoControl` 返回语义 | 不信返回值，一律读回比对；不符即 `Err` | 2026-10-24（mock） | **pending**（真机未复测） | `detects_write_verify_mismatch`；真机 `--ignored` 未跑 |
| S5 | 寄存器地址表 | `core/src/namespace.rs` | `docs/protocol/ec-access.md` / `ec-baseline.md` | 地址、含义、`AddrStatus`、实测值一致 | 2026-10-24 | **locked**（人工核对） | 两文档 7 项地址与常量逐项比对一致 |
| S6 | 撤销 ↔ 硬件状态 | `ModeChange::undo` / `ChangeRecord::undo` | EC 寄存器实际行为 | 撤销后寄存器回到快照值 | 2026-10-24（mock） | **pending**（"撤销后固件是否重新应用"未验证） | `switches_between_all_modes`（mock）；见 `ec-baseline.md` §6 |
| S7 | 前端 DOM ↔ JS 映射 | `app/ui/index.html` 的 id | `app/ui/app.js` 的 `ui` 映射 | 28 个 id 全匹配，缺失时首屏报错 | 2026-10-24 | **locked** | 浏览器验证（见 §3）+ `findMissingElements` 自检 |
| S8 | 前端交互 ↔ 后端命令 | 按钮/滑块事件 | `invoke(IPC.xxx)` 调用与返回值 | 成功/失败/禁用/脏检查状态正确 | 2026-10-24 | **locked** | 浏览器 + mock 后端验证（见 §3） |
| S9 | 发布产物 ↔ 源码 | `tag` 指向的提交 | Release 上的 `mechrevo-max.exe` | 产物由该提交构建；大小/哈希可核对 | 2026-10-24 | **locked** | v0.1.2：4 994 048 B，sha256 `c85a29ea…f33363`；API 复核 asset 大小一致 |
| S11 | 命令执行线程 | `#[tauri::command]` 生成代码 | WebView2 UI 线程 | 命令**不得**在 UI 线程上做阻塞 EC 访问（默认 `ExecutionContext::Blocking` 会占 UI 线程） | 2026-10-24 | **locked** | 全部命令改 `async fn` + `spawn_blocking`；`cargo check --all-targets` + 7 个 bin 单测 |
| S10 | 版本号 | `core/Cargo.toml` / `app/Cargo.toml` / `tauri.conf.json` | 发布 tag | 三处一致 + 与 tag 一致 | 2026-10-24 | **locked** | 三处均 `0.1.2`；tag `v0.1.2` |

## 2. Test Status

| 模块 / 节点 | 测试类型 | 状态 | 最后通过 | 关联改动 | 备注 |
|---|---|---|---|---|---|
| `core::ec::transport`（Mock） | 单元 ×4 | passed | 2026-10-24 | — | stuck / 读失败返回 Err |
| `core::ec::throttle` | 单元 ×8 | passed | 2026-10-24 | v0.1.2 R1/R2 | 含重叠检测、进程级单例 |
| `core::ec`（Ec + RestoreGuard） | 单元 ×11 | passed | 2026-10-24 | v0.1.1 P0-01 | 含 catch_unwind panic 回滚 |
| `core::control` | 单元 ×6 | passed | 2026-10-24 | — | 越界前置拦截、卡住自动回滚 |
| `core::mode` | 单元 ×8 | passed | 2026-10-24 | v0.1.1 P2-09 | 提交寄存器不入快照 |
| `core::sensors` | 单元 ×6 | passed | 2026-10-24 | — | 未知编码不猜测 |
| `core::namespace` | 单元 ×3 | passed | 2026-10-24 | — | 地址不重叠、Verified 标注 |
| doctest | 文档 | passed | 2026-10-24 | — | crate 示例可编译 |
| `app::main`（IPC 层） | 单元 ×4 | passed | 2026-10-24 | v0.1.1 P1-01/P2-04 | 锁中毒、撤销栈容错、串行化 |
| IPC 契约 | 集成 ×8 | passed | 2026-10-24 | v0.1.1 P2-01 | 含源码解析 |
| 前端渲染与交互 | **浏览器 + mock 后端** | passed | 2026-10-24 | 本轮 | 4 种后端态 + 交互路径 |
| 真机硬件 | 集成 ×7 | **skipped** | 2026-10-17 | — | `#[ignore]`；按 `docs/SPEC.md` §5.1 不执行 |
| GUI E2E（真实窗口） | E2E | **blocked** | — | — | 用户明确禁用真机 UI 自动化 |
| 覆盖率 | — | **unverified** | — | — | 无工具、无基线 |

## 3. 前端验证用的接缝替代方案（为什么用浏览器而不是真机）

真机 UI 自动化被用户明令禁止（2026-10-17），因为反复启停应用会驱动硬件。
完整断言规格与复跑要点：`docs/qa/ui-smoke-spec.md`（37 条，本轮全绿）。

替代方案：把 `app/ui/index.html + style.css + app.js` 放进 Playwright（Chromium），
用 `addInitScript` 注入假的 `window.__TAURI__.core.invoke`，覆盖 ready / driver / elevation / restart 四种后端态，
断言 DOM 文本、`aria-pressed`、`disabled`、`hidden`、状态栏文案与轮询暂停行为。
**这证明前端逻辑与 DOM 契约正确，不证明真实硬件链路** —— 后者见 S4/S6 的 `pending`。

## 4. 触发复测的映射（改这里就必须重跑那里）

| 改动位置 | 必须重跑的验证 |
|---|---|
| `ec/mod.rs`、`ec/windows.rs` | S3、S4 + `cargo test` + `cargo clippy --all-targets` |
| `ec/throttle.rs` | S3（含重叠检测） |
| `mode.rs` / `control.rs` | S6 + `cargo test` |
| `namespace.rs` | S5（协议文档逐项核对） |
| `app.js` 的 `IPC` 表 / `main.rs` 命令 | S1、S2、S8 |
| `index.html` 的 id | S7、S8 |
| 版本号 / 发版 | S9、S10 + `docs/DEPLOYMENT.md` §7 |
