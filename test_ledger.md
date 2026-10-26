# Test Ledger —— 接缝登记与测试状态

> 规则：任何代码改动**触达**某个接缝，就把该接缝状态改为 `pending`，强制重跑对应验证；
> 单元测试通过**不能**替代接缝测试；验证通过后恢复 `locked`。
> 状态：`locked`（已验证且未改动）/ `pending`（已改动待复测）/ `unverified`（从未验证）/ `n/a`（不适用，需理由）

---

## 1. Seam Registry（接缝登记）

| # | 接缝 | 模块 A | 模块 B | 契约描述 | 最后验证 | 状态 | 关联证据 |
|---|---|---|---|---|---|---|---|
| S1 | IPC 命令集 | `app/ui/app.js`（`IPC` 表） | `app/src-tauri/src/main.rs`（`invoke_handler!`） | 命令名双向一致；禁止裸字符串 `invoke`；**返回结构另由浏览器场景验证，不由本契约测试自动保证** | 2026-10-24 | **locked** | `cargo test --test ipc_contract` → 8 passed（`evidence/v0.1.2.json`） |
| S2 | IPC 参数名 | 前端调用参数（camelCase） | Rust 命令参数（snake_case） | Tauri v2 自动转 camelCase，写错静默失败 | 2026-10-24 | **locked** | `parameter_names_match_with_camel_case_conversion` / `no_snake_case_leaks_to_contract` |
| S3 | EC 访问闸门 | `WindowsAcpiTransport::ioctl` | `ec::throttle::global()` | 间隔 ≥20 ms **且**两次访问不重叠；进程级共享 | 2026-10-24 | **locked** | `run_never_allows_overlapping_access`、`global_throttle_is_shared_across_transports` |
| S4 | EC 写后读回 | `Ec::write_verified` | 驱动 `DeviceIoControl` 返回语义 | 不信返回值，一律读回比对；不符即 `Err` | 2026-10-24（mock） | **pending**（真机未复测） | `detects_write_verify_mismatch`；真机 `--ignored` 未跑 |
| S5 | 寄存器地址表 | `core/src/namespace.rs` | `docs/protocol/ec-access.md` / `ec-baseline.md` | 地址、含义、`AddrStatus`、实测值一致 | 2026-10-24 | **locked**（人工核对） | 两文档 7 项地址与常量逐项比对一致 |
| S6 | 撤销 ↔ 硬件状态 | `ModeChange::undo` / `ChangeRecord::undo` | EC 寄存器实际行为 | 撤销后寄存器回到快照值 | 2026-10-24（mock） | **pending**（"撤销后固件是否重新应用"未验证） | `switches_between_all_modes`（mock）；见 `ec-baseline.md` §6 |
| S7 | 前端 DOM ↔ JS 映射 | `app/ui/index.html` 的 id | `app/ui/app.js` 的 `ui` 映射 | 当前 7 页 DOM id 与 ui 映射一致；缺失时首屏报错 | 2026-10-27 | **locked** | `tools/qa_browser.cjs` 40 断言（含响应式 3 条 + EC 异常三分支）；`evidence/T-070-01.json` |
| S8 | 前端交互 ↔ 后端命令 | 按钮/滑块事件 | `invoke(IPC.xxx)` 调用与返回值 | 成功/失败/禁用/脏检查/危险确认状态正确 | 2026-10-25 | **locked** | `tools/qa_browser.cjs` 21 断言，含取消不发 IPC、确认才发 IPC |
| S9 | 发布产物 ↔ 源码 | `tag` 指向的提交 | Release 上的 `mechrevo-max.exe` | 产物由该提交构建；大小/哈希可核对 | 2026-10-25 | **locked** | v0.5.2：exe 5 446 144 B，sha256 `8935faff…1a65`；CI run 40155344884 三 job success |
| S10 | 版本号 | `core/Cargo.toml` / `app/Cargo.toml` / `tauri.conf.json` | 发布 tag | 三处一致 + 与 tag 一致 | 2026-10-25 | **locked** | 三处均 0.5.2；tag v0.5.2；CI run 40155344884 全 success |
| S11 | 命令执行线程 | `#[tauri::command]` 生成代码 | WebView2 UI 线程 + EC 互斥 | 阻塞 EC 操作在 blocking 池；外部采样不得持 EC 锁，也不得在 UI 线程执行 | 2026-10-25 | **locked** | `main.rs::status_snapshot` 锁外采样 + 独立 reviewer Approve + app 单测 |

## 2. Test Status

| 模块 / 节点 | 测试类型 | 状态 | 最后通过 | 关联改动 | 备注 |
|---|---|---|---|---|---|
| `core::ec::transport`（Mock） | 单元 ×4 | passed | 2026-10-24 | — | stuck / 读失败返回 Err |
| `core::ec::throttle` | 单元 ×8 | passed | 2026-10-24 | v0.1.2 R1/R2 | 含重叠检测、进程级单例 |
| `core::ec`（Ec + RestoreGuard + read_block） | 单元 ×13 | passed | 2026-10-25 | 越界不环绕 + 边界读取 | 含 `read_block_rejects_out_of_range_without_wrapping` |
| `core::control` | 单元 ×6 | passed | 2026-10-24 | — | 越界前置拦截、卡住自动回滚 |
| `core::mode` | 单元 ×8 | passed | 2026-10-24 | v0.1.1 P2-09 | 提交寄存器不入快照 |
| `core::sensors` | 单元 ×6 | passed | 2026-10-24 | — | 未知编码不猜测 |
| `core::namespace` | 单元 ×3 | passed | 2026-10-24 | — | 地址不重叠、Verified 标注 |
| doctest | 文档 | passed | 2026-10-24 | — | crate 示例可编译 |
| `app::main`（IPC 层 + sysinfo + profiles + logging） | 单元 ×44 | passed | 2026-10-25 | 外部查询边界、无电池、GPU 无效行、温度换算、错误代次、配置锁 | `cargo test --bin mechrevo-max` |
| IPC 契约 | 集成 ×8 | passed | 2026-10-24 | v0.1.1 P2-01 | 含源码解析 |
| 前端渲染与交互 | **独立 Chromium + mock IPC** | passed | 2026-10-25 | 本轮定向审计修复 | `node tools/qa_browser.cjs` → 28/28，`errors=[]` |
| 真机硬件 | 集成 ×7 | **skipped** | 2026-10-17 | — | `#[ignore]`；按 `docs/SPEC.md` §5.1 不执行 |
| GUI E2E（真实窗口） | E2E | **blocked** | — | — | 用户明确禁用真机 UI 自动化 |
| 覆盖率 | 行覆盖（core） | passed（历史基线） | 2026-10-24 | — | 行覆盖 86.56%，排除 `ec/windows.rs` 后 97.1%；本轮未重跑，app 采样路径未统计 |

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

| S12 | 托盘 ↔ 窗口生命周期 | `main.rs::setup_tray`/`on_window_event` | WebView2 主窗口 | 关闭=隐藏；托盘显示/退出可达 | 2026-10-27（真机 smoke） | **locked**（菜单点击待人工） | `evidence/T-070-01.json` |
