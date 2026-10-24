# 架构

> 目标：让任何一个新人在 10 分钟内搞清楚「代码在哪、为什么这样分层、改哪里」。

---

## 1. 全景

```
┌──────────────────────────────────────────────────────────┐
│  app/                            桌面应用（Tauri v2）      │
│                                                          │
│   ui/                            前端：原生 HTML/CSS/JS   │
│     index.html  style.css  app.js       （无打包器）      │
│              │                                           │
│              │  window.__TAURI__.core.invoke(...)        │
│              ▼                                           │
│   src-tauri/src/main.rs          IPC 命令层（薄，~350 行）│
│     get_status / set_mode / set_power_limits             │
│     undo_power / undo_mode / relaunch_elevated           │
└──────────────────────┬───────────────────────────────────┘
                       │  只依赖 core 的公开 API
┌──────────────────────▼───────────────────────────────────┐
│  core/  mechrevo-core          Rust 库（无 UI 依赖）      │
│                                                          │
│   ec/           EC 访问：传输抽象 + Windows 实现 + Mock  │
│   control.rs    功耗墙（PL1/2/4）读写，含回滚             │
│   mode.rs       运行档位（办公/均衡/狂暴），含回滚        │
│   sensors.rs    只读传感器（风扇/电源/项目 ID）           │
│   namespace.rs  寄存器地址表（每项标注可信度）            │
└──────────────────────┬───────────────────────────────────┘
                       │  EcTransport trait（可替换）
┌──────────────────────▼───────────────────────────────────┐
│  \\.\ACPIDriver  →  UWACPIDriver.sys  →  EC 硬件          │
│                    （KMDF PnP 驱动，官方提供）             │
└──────────────────────────────────────────────────────────┘
```

---

## 2. 为什么这样分层

### 2.1 核心库不依赖 UI

`mechrevo-core` 是纯 Rust 库，不依赖 Tauri、不含任何 UI 概念。

**收益**：
- 可以用 `MockTransport` **完全离线**做单元测试（33 个单测，无需硬件）
- 未来换 GUI 框架（或做 CLI）不用动业务逻辑
- 同一套逻辑可以被脚本、服务、其他程序复用

### 2.2 传输层抽象（`EcTransport` trait）

```rust
pub trait EcTransport: Send + Sync {
    fn read_ec(&self, addr: u16) -> Result<u8>;
    fn write_ec(&self, addr: u16, value: u8) -> Result<()>;
}
```

只有两个方法。所有批量读写、校验、位操作都在上层 `Ec< T >` 组合出来。

**实现**：
| 实现 | 用途 |
|---|---|
| `WindowsAcpiTransport` | 真机：IOCTL 到 `\\.\ACPIDriver` |
| `MockTransport` | 测试：内存模拟，可注入「写了不生效」「读取失败」等故障 |

**关键设计**：`MockTransport` 刻意支持模拟**官方驱动的坑** ——
`with_stuck(addr)` 让某地址「写了不生效」，用来验证读回校验真的能抓到问题。

### 2.3 薄 IPC 层

`main.rs` 只做三件事：解析参数 → 调 core → 转成 JSON。不含业务逻辑。

**为什么**：业务逻辑放 core 才能被离线测试覆盖。
IPC 层只保留无法测试的部分（Tauri 命令注册本身）。

---

## 3. 关键设计决策

| 决策 | 理由 |
|---|---|
| **读写失败必须返回 `Err`，绝不用 0 代替** | 官方驱动会把失败伪装成成功，用 0 冒充会让用户以为读到了真值 |
| **写后强制读回校验** | 同上。`write_verified` 是默认路径，`write_unverified` 需要显式说明理由 |
| **每次写入前存 `RollbackPoint`** | 硬件写入不可撤销，必须能回退 |
| **批量写入失败自动回滚已写项** | 避免留下「半改」状态 |
| **会写硬件的代码路径必须持有 `RestoreGuard`** | Rust 没有 `finally`：断言失败会 panic 展开并跳过后续语句，只有 `Drop` 会执行。守卫让"panic 也回滚"成立 |
| **寄存器地址标注 `AddrStatus`** | 跨机型地址不通用，必须区分「本机已验证」与「社区未验证」 |
| **提交寄存器不做读回校验，且不进快照** | `0x0767` 写入后硬件自动清零：校验会误报失败；没有"原始值"可还原，撤销时去写它会执行一次语义不明的命令 |
| **EC 访问强制限速，且只在传输层实现一次** | 见 `docs/SPEC.md` §5.1。放在 `WindowsAcpiTransport` 的唯一 IOCTL 出口，才能保证新增调用路径无法绕过 |
| **EC 访问必须串行化** | EC 硬件不支持并发。串行化落在 IPC 层的 `AppState.ec` 互斥上；前端在"忙"期间暂停轮询，避免无谓排队与读数抖动 |

---

## 4. 目录职责

```
core/src/
  ec/
    mod.rs          Ec<T>：读写、回滚、位操作；RollbackPoint；RestoreGuard（RAII 回滚）
    throttle.rs     AccessThrottle：访问间隔下限（默认 50 次/秒，硬下限 10 ms）
    transport.rs    EcTransport trait + MockTransport
    windows.rs      WindowsAcpiTransport（IOCTL 封装，含 unsafe）+ 在唯一出口处限速
  control.rs        PowerController：PL1/2/4 读写 + 校验 + 回滚
  mode.rs           ModeController：档位切换 + 提交寄存器处理
  sensors.rs        Sensors：只读读数
  namespace.rs      寄存器常量表（Reg + AddrStatus）
  error.rs          Error 枚举（每个变体都说明失败原因）
  lib.rs            公开 API 与 crate 文档

core/tests/hardware.rs   真机测试（#[ignore]，按需运行；写测试必须挂 RestoreGuard）

app/src-tauri/
  src/main.rs       IPC 命令 + EC 访问串行化（AppState.ec 互斥）+ 窗口适配 + DPI 声明
  tests/ipc_contract.rs   前后端契约：手写表 + 解析 app/ui/app.js 的 IPC 契约表
  build.rs          构建脚本（不自定义清单，见文件内注释）
  tauri.conf.json   窗口/CSP/权限配置
  capabilities/     Tauri v2 权限声明

app/ui/             原生前端（无构建步骤）
  index.html        结构
  style.css         设计令牌 + 布局 + 焦点可见性/reduced-motion
  app.js            IPC 契约表 + 状态管理 + 轮询（忙时暂停）

tools/              逆向与验证工具（Python）
  ec_*.py           EC 读取/扫描/验证/健康自检/压测（全局限速）
  acpi_*.py         ACPI 表导出与检索
  aml_*.py          AML 解析（操作区、EC 字段映射）
  compare_drivers.py 驱动版本对比
  parse_reg.py      官方注册表解析

scripts/            PowerShell 诊断脚本
docs/               规格、协议、部署、排障
reverse/            逆向证据（不入库）
```

---

## 5. 数据流（以「切换档位」为例）

```
用户点「狂暴」
   │
   ▼  app.js: doSwitchMode('turbo')
invoke('set_mode', { mode: 'turbo' })
   │
   ▼  main.rs: set_mode()
OperatingMode::from_key("turbo") → OperatingMode::Turbo
ModeController::switch(Turbo)
   │
   ├─ 1. ec.snapshot([0x751, 0x7AB, 0x0767]) → RollbackPoint
   │
   ├─ 2. write_verified(0x751, 0x10)    ← 写 + 读回校验
   ├─ 3. write_verified(0x7AB, 0x10)
   ├─ 4. write_unverified(0x0767, 0x01) ← 自清零，不校验
   │      （任一步失败 → ec.restore(&rollback) 回滚全部）
   │
   ▼  返回 ModeChange { mode, rollback }
存入 AppState.undo.mode（供「撤销」使用）
   │
   ▼  前端收到 Ok("已切换到狂暴档")
setStatus(...) → refresh() → 轮询读回 0x751=0x10 确认
```

---

## 6. 测试策略

| 层级 | 位置 | 需要硬件 | 频率 |
|---|---|---|---|
| 单元测试 | `core/src/**` 的 `#[cfg(test)]` | ❌ | 每次改动 |
| 契约测试 | `app/src-tauri/tests/ipc_contract.rs` | ❌ | 每次改动 |
| 静态检查 | clippy / fmt | ❌ | 每次改动 |
| 真机测试 | `core/tests/hardware.rs`（`#[ignore]`） | ✅ | 少量、谨慎 |
| 健康自检 | `tools/ec_health.py` | ✅ | 按需 |

**刻意不做**：UI 自动化 E2E、压力测试 —— 见 `docs/SPEC.md` §5.1。

---

## 7. 扩展指引

### 新增一个硬件功能（例如电池充电上限）

1. **确认寄存器**：用 `tools/ec_read.py` 读、`ec_scan.py --delta` 找动态值，
   **必须实机验证**，不能只信社区文档
2. **登记到 `namespace.rs`**：带 `AddrStatus` 与 `note`（说明来源与验证方式）
3. **在 `control.rs` 或新模块实现**：写入必须走 `write_verified` + 先 `snapshot`
4. **补单元测试**：用 `MockTransport`，包含「写了不生效」的失败路径
5. **暴露给 IPC**：`main.rs` 加命令 → 同步更新 `ipc_contract.rs` 的两个常量表
6. **前端接入**：加 UI + 状态反馈 + 撤销路径
7. **更新文档**：`docs/protocol/ec-access.md` 加地址条目

### 风险分级（决定要不要先问用户）

| 级别 | 例子 | 做法 |
|---|---|---|
| 🟢 低 | 只读、灯光、档位 | 直接做 |
| 🟡 中 | 功耗墙、电池上限 | 做，但必须带回滚 |
| 🔴 高 | 风扇曲线、电压、MUX | **先单独评审 + 用户逐项确认** |
