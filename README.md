# Mechrevo-Max

机械革命（MECHREVO）笔记本的**自研控制台** —— 官方「电竞控制台」的轻量替代。
适用于机械革命 40 系列等采用同方（Uniwill）模具的机型。

## 为什么要重写

官方 ControlCenter3 的问题（均为本项目实测）：

| 问题 | 实测数据 |
|---|---|
| **内存占用巨大** | 主程序 137 MB + 托盘进程 41 MB = **178 MB** |
| **主程序无法反编译** | `GamingCenter3_Cross.dll` 是 .NET Native AOT，28.7 MB，无 IL |
| **进程间通信硬编码凭据** | 本地 MQTT `localhost:13688`，密码明文写死 `UWPClient_Pwd888881772688_10` |
| **通信链路静默失败** | 托盘进程用 `catch {}` 吞掉所有异常，MQTT 不通时用户完全无感知 |
| **装完 EC 通道不可用** | 驱动是 KMDF PnP 驱动（绑定 `ACPI\INOU0000`），但 Windows 不自动枚举该设备，官方安装流程也不触发扫描 → 驱动加载却无设备可绑 → **所有硬件功能失效** |
| **能力表数据不一致** | 官方能力表里 `LiquidCoolingAutoModeSupport=1` 却同时 `LiquidCoolingSupport=0` |

## 本项目做的

直接通过内核驱动访问 EC（Embedded Controller），**不依赖官方任何组件**。

### 已验证的能力（真机实测）

```
✅ EC 设备访问          \\.\ACPIDriver, IOCTL 0x9C40A488 / 0x9C40A48C
✅ 项目 ID 识别          读到 16，与官方注册表 ProjectID 一致
✅ 风扇转速              CPU 3970 RPM / GPU 75 RPM
✅ 电源状态              适配器/电池
✅ 功耗墙读取            Gaming 75/75/200, Office 45/45/200（与官方出厂值一致）
✅ 功耗墙写入            实测 45 → 50 生效 → 回滚 → 45 完全还原
✅ 运行档位切换          均衡/办公/狂暴三档，狂暴档 GPU 风扇 75→130 RPM 可见变化，实验后完全恢复
✅ 全量 EC 扫描          0x0000-0x0FFF 共 4096 地址，2002 个非零
```

### 安全设计（针对官方驱动的坑）

社区逆向发现官方驱动**存在把失败伪造成成功的代码路径**，`DeviceIoControl` 返回成功不代表硬件真的执行了。
本项目强制：

1. **失败必须报错** —— 绝不用 0 代替读失败
2. **写后读回校验** —— 默认写入方式
3. **改动前存回滚点** —— 任何写入前先 `snapshot`，失败自动回滚
4. **地址标注可信度** —— `Verified` / `CommunityUnverified` / `NotApplicable` 三态，不拿其他机型的地址当本机结论
5. **访问强制限速** —— 限速在唯一 IOCTL 出口处执行（默认 50 次/秒，硬下限 100 次/秒），新增调用路径也无法绕过
6. **回滚守卫** —— 会写硬件的代码路径持有 RAII 守卫，连 panic 展开都会还原（Rust 没有 `finally`）
7. **访问串行化** —— IPC 层用互斥锁保护 EC，忙时前端暂停轮询，避免并发读写交错

## 桌面应用

Tauri v2 桌面程序，界面用原生 HTML/CSS/JS（无打包器，构建链最短）。

![界面](docs/screenshots/main.png)

功能：
- **运行档位**一键切换（办公 / 均衡 / 狂暴），带当前档位高亮与原始寄存器值显示
- **实时读数**：CPU / GPU 风扇转速（2 秒轮询）、供电状态
- **默认功耗墙**调节：PL1 / PL2 / PL4 滑块，游戏与办公分别配置
- **一键撤销**：档位切换与功耗改动都可回退到改动前的快照
- EC 不可用时给出明确的修复指引，权限不足时提供"以管理员身份重启"按钮

### 构建

```bash
cd app/src-tauri
cargo tauri dev      # 开发运行
cargo tauri build    # 打包 msi / nsis
```

> ⚠️ **构建前必须把较新的 Windows SDK 加入 PATH**，否则 `embed-resource` 会选中
> 很旧的 `rc.exe`，在含中文的路径下编译资源失败：
> ```bash
> export PATH="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64:$PATH"
> ```
>
> 另外：应用在启动时通过 `SetProcessDpiAwarenessContext` 声明 DPI 感知。
> 若缺少这一步，进程会被判定为 DPI 不感知，在 150% 缩放的屏幕上窗口被整体放大 1.5 倍，
> 物理尺寸超出屏幕导致右侧内容被裁掉。

## 结构

```
core/                   Rust 核心库 (mechrevo-core)
  src/ec/               EC 访问层（传输抽象 + Windows 实现 + Mock）
  src/control.rs        功耗墙控制（含回滚）
  src/mode.rs           运行档位切换（含回滚）
  src/sensors.rs        传感器读数
  src/namespace.rs      EC 寄存器地址表（带可信度标注）
  tests/hardware.rs     真机集成测试

app/                    桌面 GUI (Tauri v2)
  src-tauri/            后端：把 core 暴露为 IPC 命令
  ui/                   前端：原生 HTML/CSS/JS

tools/                  Python 逆向与分析工具
docs/protocol/          协议文档（ec-baseline.md = 入库的脱敏基线）
reverse/evidence/      逆向原始数据（含本机特征值 → 不入库）
workflow_status.md     工作状态台账
```

## 构建与测试

```bash
# 核心库（离线，用 Mock 传输层）
cd core
cargo test                                    # 44 单测 + 1 doctest
cargo clippy --all-targets -- -D warnings     # 0 warning
cargo test --test hardware -- --ignored --test-threads=1 --nocapture   # 真机测试（谨慎）

# 桌面应用
cd ../app/src-tauri
cargo test --bin mechrevo-max                 # IPC 层单测（锁/串行化/中毒处理）
cargo test --test ipc_contract                # 前后端契约（含解析 app.js 的 IPC 表）
cargo build --release
```

### 前置条件：让 EC 设备就绪

```powershell
sc.exe start UWACPIDriver      # 启动内核驱动
pnputil.exe /scan-devices      # 触发 PnP 扫描（关键，官方漏了这步）
```

详见 [docs/protocol/ec-access.md](docs/protocol/ec-access.md)。

## 合规说明

本项目**不包含、不分发**任何官方二进制文件、驱动或反编译产物。
仓库内只有自研代码与协议分析结论；原始固件/驱动分析数据保留在使用者本机。

协议知识的来源是社区公开的逆向成果（LumaDesk、roj234/mechrevo_ec_api 等），
本项目在此基础上独立实现，并在真机上重新验证。

## 开发状态

见 [workflow_status.md](workflow_status.md)。

## 许可

MIT
