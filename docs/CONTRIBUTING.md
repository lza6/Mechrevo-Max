# 贡献指南

> 面向「想加功能 / 修 bug / 从零接手」的人。**先读完这份再动代码。**

---

## 0. 三分钟上手

```bash
# 1) 跑离线测试（不需要硬件，不需要管理员）
cd core
cargo test                 # 应为 33 passed
cargo clippy --all-targets # 应为 0 warning

# 2) 看规格与架构
#    docs/SPEC.md         —— 做什么、不做什么、验收标准
#    docs/ARCHITECTURE.md —— 代码在哪、为什么这样分层
#    docs/DEPLOYMENT.md   —— 构建踩坑与发布流程
```

**不需要硬件就能做完整开发与测试** —— 协议逻辑用 `MockTransport` 覆盖。

---

## 1. ⚠️ 动手前必读：两条硬性安全禁令

### 1.1 EC 访问必须限速

EC 是 8051 级单片机，**同时负责风扇与温控**。以数百次/秒泛洪会令其停摆：

```
EC 停摆 → 风扇/温控失控 → 平台硬复位（无 Windows 转储，Event 41）
```

**本项目开发过程中真实发生过一次这样的意外关机。**

规则：
- 所有 EC 访问代码**必须**限速（`MIN_INTERVAL_S` 硬上限 = 100 次/秒）
- 已有工具的限速在 `Ec` 类**内部**强制执行，不要绕过
- 新增工具请复用 `tools/ec_stress.py` 里的 `clamp_interval()` 模式

### 1.2 禁止硬件压力测试

**不要**跑 CPU/GPU 满载压力测试。笔记本全核满载逼近温度墙，同样会触发硬复位。
本项目的「压测」定义是：长时**低速**轮询、写入/复原往复、设备掉线恢复、边界值拦截。

详见 `docs/SPEC.md` §5.1。

---

## 2. 开发流程

### 2.1 改代码前

1. 读 `docs/SPEC.md` —— 确认这个改动在规格范围内（尤其确认它**不是**「明确不做」的项）
2. 读 `docs/ARCHITECTURE.md` §7 —— 按「新增一个硬件功能」的清单走
3. 若涉及新的 EC 寄存器：**必须实机验证**，不能只信社区文档
   （跨机型地址不通用，已有实例：社区说 `0x0783` 是当前 PL，本机读出恒为 0）

### 2.2 写代码

**必须遵守的不变量**：

| 不变量 | 怎么保证 |
|---|---|
| 读失败必须 `Err`，不得返回 0 | `Error::Ioctl` |
| 写入必须先 `snapshot` | `Ec::snapshot` |
| 写入必须读回校验 | `Ec::write_verified`（`write_unverified` 需注释说明理由） |
| 失败必须自动回滚 | 见 `control.rs` / `mode.rs` 的模式 |
| 新寄存器必须标 `AddrStatus` | `namespace.rs` |
| 公开 API 必须有文档注释 | `#![warn(missing_docs)]` |

**不要做**：
- ❌ 为单次使用创建抽象
- ❌ 添加未要求的可配置性
- ❌ 用 `unwrap()` / `expect()` 处理可恢复错误（测试里可以）
- ❌ 绕过 `EcTransport` 直接调 IOCTL（会破坏可测试性）

### 2.3 提交前

```bash
# core
cd core
cargo fmt
cargo clippy --all-targets   # 必须 0 warning
cargo test                   # 必须全过

# app（改了 IPC 或 UI 时）
cd ../app/src-tauri
cargo test --test ipc_contract
cargo build --release

# 前端语法
node --check ../ui/app.js
```

### 2.4 提交信息

```
<type>: <描述>

<可选正文>
```

`type`: `feat` / `fix` / `refactor` / `docs` / `test` / `chore` / `perf`

---

## 3. 测试要求

| 改动类型 | 必须补的测试 |
|---|---|
| 新增 core 功能 | 单元测试（`MockTransport`），**必须含失败路径** |
| 新增 IPC 命令 | 同步更新 `ipc_contract.rs` 的两个常量表 |
| 修 bug | 一个能复现该 bug 的测试 |
| 改协议地址 | 更新 `docs/protocol/ec-access.md`，标注验证方式 |

**测试质量要求**：

- 用 `MockTransport::with_stuck()` 验证「写了不生效」能被抓到
- 用 `MockTransport::with_failing_read()` 验证失败会报错而不是静默返回 0
- 断言要具体（断言值，不是只断言 `is_ok()`）
- 覆盖边界：最小值、最大值、越界

---

## 4. 前端约定

- **无构建步骤**：直接改 `app/ui/*`，不要引入打包器
- **改完必须重嵌入**：`touch src/main.rs && cargo build`（否则跑的是旧资源，见 `docs/DEPLOYMENT.md` §2.3）
- **IPC 参数名用 camelCase**：Tauri v2 会把 Rust 的 `snake_case` 转成 camelCase，
  写错会**静默失败**（`ipc_contract.rs` 会抓到）
- **不要用 `console.log`**：用状态栏 `setStatus()` 给用户反馈
- **错误要可见**：所有 `invoke()` 都要 `try/catch` 并把错误显示到界面

### 设计约定

- 颜色/间距用 `style.css` 顶部的 CSS 变量，不要硬编码
- 有 hover / focus / active 状态
- 交互元素要有禁用态与加载态

---

## 5. 文档要求

改了这些必须同步更新：

| 改动 | 更新哪个文档 |
|---|---|
| 新增/修改协议地址 | `docs/protocol/ec-access.md` |
| 新增功能或改变范围 | `docs/SPEC.md` |
| 改动模块结构 | `docs/ARCHITECTURE.md` |
| 改构建/发布流程 | `docs/DEPLOYMENT.md` |
| 发现新的排障场景 | `docs/TROUBLESHOOTING.md` |

---

## 6. 合规红线

**不得提交、不得分发**：

- ❌ 机械革命官方控制台的任何二进制文件
- ❌ `UWACPIDriver.sys` 或其他官方驱动
- ❌ 官方程序的反编译输出
- ❌ 任何含本机特征值的实测数据（`reverse/evidence/*.json` 已在 `.gitignore`）
- ✅ 但**应该**提交可复核的脱敏结论 → `docs/protocol/ec-baseline.md`（只放地址表、统计、交叉核对来源，不放原始数据）

本仓库只放**自研代码**与**协议分析结论**。协议知识来源于社区公开逆向成果
（见 `docs/protocol/ec-access.md` §8），本项目在其基础上独立实现并在真机重新验证。

---

## 7. 风险分级：什么时候必须先问

| 级别 | 例子 | 做法 |
|---|---|---|
| 🟢 低 | 只读读数、灯光、档位切换 | 直接做 |
| 🟡 中 | 功耗墙、电池充电上限 | 做，但必须带回滚与校验 |
| 🔴 高 | **风扇曲线、电压偏移、MUX 切换** | **先评审 + 用户逐项确认再动** |

高风险项的共同点：写错可能导致硬件损坏（风扇不转→过热）、
不可逆（改 UEFI 变量需重启）、或让机器无法正常启动。

---

## 8. 遇到问题

1. 先跑 `docs/TROUBLESHOOTING.md` 里的诊断
2. EC 相关先跑 `python -X utf8 -I tools/ec_health.py`
3. 机器重启过：跑 `scripts/diagnose_reboot.ps1`（**不是**本应用造成的，但要知道怎么查）
