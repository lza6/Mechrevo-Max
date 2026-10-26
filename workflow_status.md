# 机械革命控制台自研 —— 工作状态

> 只记录事实与证据。最后更新：2026-10-24

## 2026-10-24 · 第 3 轮：骨架补齐 + 前端真实验证 + CI

本轮**无代码改动**（前端 37 条断言全绿，未发现缺陷），只补交付骨架与验证覆盖：

| 项 | 结果 | 证据 |
|---|---|---|
| 执行骨架 | 新增 `policy.md`、`test_ledger.md`（此前缺失） | 门禁数值按项目实际；10 条接缝 |
| 前端渲染与交互 | Playwright + 假 `invoke`，**37/37 通过**，`pageerror`/`console.error` 各 0 | `docs/qa/ui-smoke-spec.md`、3 张截图 |
| 真机 UI E2E | 仍禁用（用户 2026-10-17 指示） | 替代方案见上 |
| CI | 新增 `.github/workflows/ci.yml` | 运行结论待 push 后核对 |
| 教训 | 首轮 11 条红全是脚手架假象（同 URL 改 hash 不重载）；**看到红先怀疑脚手架** | 已写入规格文件 |

## 2026-10-24 · v0.1.2 独立复审修复

reviewer 子代理独立验证后给出 **CONDITIONAL PASS**，指出 v0.1.1 两条核心声明只成立一半，
全部已修复并补回归测试：

| 编号 | 问题 | 修复 | 证据 |
|---|---|---|---|
| R1 P1 | `AccessThrottle::wait()` 释放锁后 IOCTL 才执行 → 慢 IOCTL 会重叠 | `run(|| ...)` 整个访问持锁 | `run_never_allows_overlapping_access`（峰值并发=1） |
| R2 P1 | 闸门 per-instance，每命令首访零间隔、可绕过 | 进程级 `global()` 单例 | `global_throttle_is_shared_across_transports` |
| R3 P2 | 台账 clippy/fmt 超范围断言；app fmt 失败 + 3 警告 | 修 lint + `cargo fmt`，标注 scope | 两个 crate 均干净 |
| R4 P2 | `mode.rs` 注释与实现矛盾 | 注释对齐 + 标注未验证 | `mode.rs` 注释 |

验证：`core` 46 + doctest 1；`bin` 4；契约 8；两个 crate fmt/clippy 全绿；
release exe 4 994 048 B（sha256 `c85a29ea…f33363`）。
[Release v0.1.2](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.1.2) 已创建并核验（draft=false，asset 大小与本地一致）。证据：`evidence/v0.1.2.json`。

## 2026-10-24 · v0.1.1 护栏补丁

**范围**：不新增硬件功能，只补安全与可信地基。任务状态权威源为 `project_specs.md`，
改进计划为 `docs/计划书/下一步改进指南.md`，证据为 `evidence/v0.1.1.json`。

| 项 | 结果 | 证据 |
|---|---|---|
| EC 访问串行化 | `AppState.ec` 互斥 + 忙时停轮询 | `lock_ec_serializes_access` 单测 |
| 访问限速 | `AccessThrottle` 挂在唯一 IOCTL 出口（默认 50 次/秒，硬下限 100 次/秒） | `ec::throttle` 6 单测 |
| panic 也回滚 | `Ec::guard()` + `RestoreGuard` | `guard_restores_after_panic` |
| 撤销不丢记录 | 成功才出栈 | `main.rs` `undo_*` |
| 证据链 | `docs/protocol/ec-baseline.md`（脱敏） | 5 处引用已改 |
| 契约测试 | 解析 `app/ui/app.js` 的 `IPC` 表双向比对 | `ipc_contract` 8 单测 |
| 验证 | 核心 44 单测 + 1 doctest；bin 4 单测；契约 8；clippy 0 warning；fmt 干净；release exe 4 992 512 B | `evidence/v0.1.1.json` |

**已发布**：`main` = `1fe2a14`（已推送），tag `v0.1.1` 已推送，
[Release v0.1.1](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.1.1) 已创建并上传
`mechrevo-max.exe`（4 992 512 B，API 复核 `draft=false` 且大小与本地构建一致）。
**未执行**：真机硬件测试（安全策略）、GUI E2E（用户禁用）、覆盖率（无工具）。
**计划文档审计**：32 个 md，0 个可删（见 `docs/计划书/闭环审计-20261024.md`）。

## ⚠️ 重大事件记录：2026-10-17 意外硬复位

**事实**（Windows 事件日志，非推测）：

```
2026-10-17 02:11:16  系统意外关闭           Event 6008
2026-10-17 02:12:21  重新启动
2026-10-17 02:13:52  Kernel-Power 41        未正常关机
无 BugCheck(1001) · 无 WHEA 硬件错误 · 无 1074 正常关机
```

**同模式事件在更早也出现过，均早于本次会话**：
- 2026-10-13 18:05:43
- 2026-10-16 15:27:10

→ 说明这是**机器既有的偶发问题**，不是被本项目的写入破坏的。

**本次会话中我做的两件高风险事**（都是我不该做的）：
1. 让子代理执行 **CPU 满载压力测试**（i9-12900HX 全核 100%）
2. 以 **约 620 次/秒泛洪 EC** 读取 12000 次

两者都可能触发硬复位。**无直接证据锁定是哪一个**，但两者都应禁止。

**已实施的修复**：

| 措施 | 内容 |
|---|---|
| `tools/ec_stress.py` | 强制限速：默认 20 次/秒，**硬上限 100 次/秒**（超出强制夹回） |
| `tools/ec_scan.py` | 强制限速：默认 50 次/秒，**硬上限 100 次/秒** |
| 限速实现方式 | 在 `Ec` 类**内部**强制执行，调用方无法绕过 |
| 删除危险工具 | 移除子代理遗留的 `reverse/gpuload/`（CUDA GPU 满载工具） |
| `.gitignore` | 明确排除 `burn.cu` / `*cpuload*` / `*gpuload*` 等压测工具 |
| `docs/SPEC.md` §5.1 | 新增「EC 访问限速」硬性要求 + 禁止 CPU 满载压测 |
| 新增 `tools/ec_health.py` | EC 健康自检（随时可跑，限速只读） |
| 记忆 | `ec-flooding-hardware-hazard` |

**EC 健康验证（2026-10-17，硬证据）**：

```
[1] 关键寄存器    ✅ 全部合理（项目ID=16, Gaming 75/75/200, Office 45/45/200）
[2] 动态采样      ✅ 风扇 3970→3506 RPM 随负载变化 → EC 在正常采样
[3] 连续读取      ✅ 519 次, 0 失败
                  ✅ 档位字段正确复原 (751=0x00, 7AB=0x00)
结论: EC 工作正常，未被写坏
```

**⚠️ 用户明确指示（2026-10-17）**：
> 「其实不用 E2E 的，我们目前这种状态我们机器并不会自动重启，要不你就 mock 测试等等的吧，
> 反正就是说有点危险」

→ **验证策略调整：以 mock / 离线单元测试为主，不做真机 E2E 与任何压力测试。**
→ 真机操作限于：低频只读自检（`ec_health.py`）、必要的单次写入验证（带回滚）。

---

## 验证策略（2026-10-17 起）

| 层级 | 方式 | 是否跑 |
|---|---|---|
| 1 | **单元测试（mock 传输层）** —— `cargo test` | ✅ 主力 |
| 2 | **静态检查** —— `cargo clippy --all-targets` / `fmt --check` | ✅ 每次改动 |
| 3 | **离线契约测试** —— IPC 参数名/返回结构/错误路径 | ✅ |
| 4 | **低频只读自检** —— `tools/ec_health.py`（20 次/秒，只读） | ⚠️ 按需 |
| 5 | **单次写入验证** —— 带回滚点，一次一个寄存器 | ⚠️ 谨慎、少量 |
| 6 | ~~真机 E2E / UI 自动化~~ | ❌ **禁用**（用户指示） |
| 7 | ~~压力测试（CPU/GPU 满载、EC 泛洪）~~ | ❌ **禁止** |

**理由**：真机 E2E 需要反复启停应用并驱动硬件，压力测试逼近温度墙与 EC 极限，
两者都可能触发本机既有的硬复位问题。改用 mock 覆盖协议逻辑，
真机只做必要的低频只读确认。

## 任务契约

**总目标**：反编译/摸透官方控制台，全新自研替代，推到 https://github.com/lza6/Mechrevo-Max

**用户已确认的路线**：全新自研重写（非在官方基础上魔改）

**用户已确认的首批目标**（全选）：
1. 读数 + 模式切换
2. 功耗与性能调优
3. 风扇曲线自定义
4. 先把官方彻底摸透（含实机 EC）
5. 官方的所有功能都要摸透，所有东西完整落地闭环

## 验收标准

| # | 标准 | 状态 | 证据 |
|---|---|---|---|
| A1 | 摸清官方控制台完整架构与功能清单 | **已完成** | 见「已达成的发现」 |
| A2 | 打通本机 EC 读写通道 | **已完成** | EC 读 8/8 成功，全量扫描 4096 地址 |
| A3 | 产出本机 EC 全寄存器表 | **已完成（基线）** | 脱敏可复核基线：`docs/protocol/ec-baseline.md`；原始数据 `reverse/evidence/ec-scan.json`（不入库）；已验证 7 项关键地址 |
| A4 | 自研控制台读数功能可用 | **已完成（核心库）** | Rust `Sensors` + 真机测试通过 |
| A5 | 模式切换可用 | **已完成（核心库）** | `0x751`/`0x7AB` 三档实测通过，见协议文档第 7 节 |
| A6 | 功耗调优可用 | **已完成（核心库）** | Rust `PowerController`，真机写入+回滚通过 |
| A7 | 风扇曲线可用 | 未开始 | 高风险，需先确认单体 |
| A8 | 代码推送到 Mechrevo-Max | **已完成** | `main` = `ca9b80a`，64 文件，本地/远端一致 |
| A9 | 桌面 GUI (Tauri) | **已完成** | 真机运行验证通过，见下方截图证据 |
| A10 | 创建发行版 (Release) | **已完成** | `v0.1.0`，含 `mechrevo-max.exe` 4.76 MB |
| A11 | 文档补全 | **已完成** | SPEC / ARCHITECTURE / DEPLOYMENT / TROUBLESHOOTING / CONTRIBUTING / VERIFICATION-LEDGER / CHANGELOG |
| A12 | 发布工具 | **已完成** | `tools/publish_release.py`（复用 git 凭据，上传产物） |
| A13 | IPC 契约测试 | **已完成** | 编译期校验 5 个测试，防 Tauri v2 参数名坑 |
| A14 | 未提交官方二进制（合规） | **已完成** | `git add --dry-run` 检查无 `.sys`/`.msix`/`.exe`/`.dll` |

### GUI 验证证据（2026-10-17）

真机运行截图 `docs/screenshots/main.png`，界面同时显示：
- `EC 已连接`（绿色状态点）+ `项目 ID 16`
- CPU 风扇 `3970 RPM`、GPU 风扇 `75 RPM`、供电`适配器`（编码 7）—— **实时 EC 数据**
- 运行档位「均衡」高亮，原始寄存器 `751=0x00 · 7AB=0x00`
- 功耗墙 `PL1=75W / PL2=75W / PL4=200W`，与官方出厂值一致

布局自检：`readouts.clientWidth == readouts.scrollWidth`（674/674），**结构性无溢出**。

## 已达成的发现（已验证）

### 官方控制台架构
- `ControlCenter_5.17.49.19_Mechrevo.exe`（171 MB）= InstallShield 分发包，内含 MSIX
- 主包：`PreinstallKit/GamingCenter3_Cross.UWP_5.17.49.19_x64.msixbundle`
- Identity `ControlCenter3`，Publisher `CN=AISTONE GLOBAL (SUZHOU) LIMITED`
- 组成：UWP 主程序（`GamingCenter3_Cross.dll` 28.7 MB，**.NET Native AOT，无 IL，无法反编译**）
  + 全信任托盘进程 `Win32/SystrayComponent.exe`（**有 IL，已成功反编译 3337 行**）
  + `Win32/PowerToys.KeyboardManagerEngine.exe`（源自微软 PowerToys）

### 已反编译成果（`reverse/disasm/SystrayComponent.decompiled.cs`）
- **MQTT 进程间通信**（硬编码凭据，本机 13688 端口未监听）：
  ```
  broker   = localhost:13688  (明文, MqttSslProtocols.None)
  clientId = UWPClient_10
  username = UWPClient_User_10
  password = UWPClient_Pwd888881772688_10
  ```
- **Topic 表 45 个**（`Customize/SupportControl`、`EC/Status`、`Fan/Control`、`System/CpuInfo`…）
- **`MyRamFan1p5`（60+ 字段）= 官方完整硬件控制功能清单**
- **`ProjectID` 枚举 18 项**、**`CustomizeNumber` 枚举 35 个 OEM 品牌**（Mechrevo = 17）
- `CustomizeNumber` 枚举值（Mechrevo=17）与 `appsettings.json` 的 `customizeTarget: 17` 一致

### 官方能力协商表（`OEM_Backup_20261001_111014/GamingCenter2.reg`，已解析）
本机（旷世16 Pro GM6PQ0X）支持的官方功能：

| 项 | 值 | 含义 |
|---|---|---|
| `ProjectID` | 16 | 本机项目号 |
| `CustomizeTarget` | 17 | Mechrevo 品牌 |
| `BIOS_PROJECT_ID` | `IDX` | BIOS 项目标识 |
| `DGpuDirectConnectionSupport` | 1 | ✅ 支持 MUX/独显直连 |
| `KeyboardSupport` | 1 | ✅ 键盘灯 |
| `NumPadSupport` | 1 | ✅ 小键盘 |
| `ColorCalibrationSupport` | 1 | ✅ 色彩校准 |
| `AcRecoverySwitchSupport` | 1 | ✅ AC 恢复开关 |
| `FanSettingsSupport` | 1 | ✅ 风扇设置 |
| `RamFan1p5Support` | 1 | ✅ RamFan 1.5 协议 |
| `TurboModeSupport` | 1 | ✅ 狂暴模式 |
| `OcSettingsSupport` | 1 | ✅ 超频设置 |
| `SystemMonitorSupport` | 1 | ✅ 系统监控 |
| `LiquidCoolingAutoModeSupport` | 1 | ⚠️ 标 1 但 `LiquidCoolingSupport=0`（疑似官方数据 bug） |
| `LightbarSupport` / `RGBLightbarSupport` | 0 | ❌ 无灯带 |
| `TypeCSupport` | 0 | ❌ 无 Type-C 相关 |
| `IsAMDPlatform` | 0 | Intel 平台 |
| `IsNvGpu` | 1 | NVIDIA GPU |

### 官方功耗/风扇参数（本机出厂值）
| 模式 | PL1 | PL2 | PL4 |
|---|---|---|---|
| Gaming（游戏） | 75 | 75 | 200 |
| Office（办公） | 45 | 45 | 200 |

- EC 版本：`1.23`
- 风扇 PWM 档位表（Office 高级模式 L1–L4，T1–T5 各 5 点）

### 官方用户 Profile 结构（`OEM_Backup/UserPofiles/`）
- `MainOption.json`：当前 `OperatingMode=3`、各模式 Profile 索引、WhisperMode、FanBoost、风扇安全保护通知
- `Mode{1..4}_Profile{1..5}.json`：完整参数
  - CPU：PL1/PL2/PL4 + 各自 `_Enabled`、`PL1_dc`/`PL2_dc`、`PL1_S2`/`PL2_S2`、`OffsetCoreVoltage`、`TccOffset`、AMD 侧 SPL/SPPT/FPPT/AmdTccTarget
  - GPU：`CoreClockOffset`、`MemoryClockOffset`、`TargetTemperature`、`ConfigurableTGPSwitch`/`Target`、`DynamicBoostSwitch`/`DynamicBoost`、WhisperMode 三档 MinFps（30/40/60）、`CustomTGPinGCUforGN21`
  - FAN：`TableName`、`FanSwitchSpeedEnabled`、`FanSwitchSpeed`(300)、`SkipSafetyAbnormalProtection`
  - MEM：`MemoryOverClockSwitch`
- 本机 4 个模式 × 多 Profile：Mode1/2/3 各 2 个，Mode4 有 5 个

### 官方硬件协议（来自社区逆向，已交叉验证）
详见记忆文件 `prior-reverse-engineering-artifacts`。核心：
- 设备 `\\.\ACPIDriver`；EC 读 IOCTL `0x9C40A488`，写 `0x9C40A48C`
- `UWACPIDriver.sys` SHA256 与本机 **完全一致**（`6e360ee4…e61235`）
- 完整 EC 地址表（LumaDesk 20+ 项 + roj234 40+ 项）

## 阻塞项

### B1【已解决 ✅】本机 EC 读写通道

**解决方式（2026-10-16）**：
```powershell
sc.exe start UWACPIDriver      # 1. 启动内核驱动
pnputil.exe /scan-devices      # 2. 触发 PnP 重新扫描 ← 关键一步
```
第 2 步执行后，`ACPI\INOU0000\0` 设备被枚举出来（Name=`ACPIDriver`，Status=OK），
KMDF 驱动随即完成 `EvtDriverDeviceAdd`，创建出 `\DosDevices\ACPIDriver`，
用户态 `CreateFile` 成功。

**重要推论**：设备定义一直在 DSDT 中（见下方「固件层证据」），但 Windows 启动时**没有自动枚举它**。
必须手动触发一次设备扫描。**这很可能就是官方控制台在本机"装完不能用 / 一堆 bug"的根因之一** ——
官方安装流程没有触发这次扫描。这是一个可复现的官方缺陷，值得作为我们项目的卖点记录。

**验证结果**：EC 读 8/8 成功；全量扫描 4096 地址 6.6s；关键寄存器 7 项与官方配置完全吻合。

<details>
<summary>原排查过程（保留作证据）</summary>

**现象**：`\\.\ACPIDriver` `CreateFile` 返回 errno=2（找不到文件）。

**根因（已定位，证据链完整）**：
1. `UWACPIDriver.sys` 是 **KMDF PnP 驱动**（导入 `WdfLdr.SYS`，导入表见驱动分析）
2. 其 INF（`DriverStore/FileRepository/uwacpidriver.inf_amd64_1ed39a93aac84c8d/UWACPIDriver.inf`）声明：
   ```ini
   [Standard.NTamd64]
   %UWACPIDriver.DeviceDesc%=UWACPIDriver_Device, ACPI\INOU0000
   ```
3. 内核对象枚举确认 `\Driver\UWACPIDriver` **已加载**（184 个 Driver 对象之一），但 `\Device` 491 个对象中**无 ACPIDriver 设备**
4. 本机 ACPI 枚举（`HKLM\SYSTEM\CurrentControlSet\Enum\ACPI`）**没有 `INOU0000` 节点**
   - 仅有的 `UNIW0001` 经查明是 **I2C HID 触摸板**（`Service=hidi2c`），非厂商控制接口
5. 结论：BIOS 未暴露 `ACPI\INOU0000`，KMDF 驱动无设备可绑 → `EvtDriverDeviceAdd` 从不执行 → 不创建设备对象 → 用户态无法访问
6. 官方用户态组件 `ACPIDriverDll.dll` / `GPUInfoDLL.dll` / `UEFI_Firmware.dll` **本机根本不存在**
7. 官方 OEM 服务目录 `C:\Program Files\OEM\机械革命电竞控制台\`（安装日志确认曾存在，含 `UWACPIDriver\UWACPIDriver.inf`）**现已被清空**

**已尝试的替代探测**：
| 路径 | 结果 |
|---|---|
| `\\.\ACPIDriver` | ❌ errno=2 |
| `\\.\ACPIDriverDll` | ❌ 用户态 DLL 不存在 |
| `\Driver\UWACPIDriver` | 对象存在但不建设备 |
| 官方 UWP 启动后 | ❌ 13688 端口仍未监听，LocalState 全空 |
| PNP0C14 (WMI 固件接口) | 存在 3 个（`DSARDEV`/`1`/`TESTDEV`），未找到厂商自定义 WMI 类 |
| ACPI WMI 类（root/WMI） | 仅有微软/Intel 标准类，无厂商类 |

**下一步待选方案**：
- 方案 A：重新安装官方控制台完整版（含 OEM 服务），让其重建驱动与设备 —— 需用户同意安装
- 方案 B：逆向官方安装器，取出被删的 `ACPIDriverDll.dll` 等组件
- 方案 C：绕过该驱动，走 ACPI WMI（`ACPI\PNP0C14\DSARDEV`）直接调 ACPI 方法 —— 需确认厂商 ASL 是否暴露
- 方案 D：找新版 `5.56.51.26` 的安装包（本机已装 UWP 部分，但缺失 OEM 服务）

**⚠️ 涉及 BIOS/ACPI 硬件层，且方案 A 需要安装操作 —— 必须用户决策。**

</details>

### 固件层证据（DSDT 分析，已解决后的补充）

| 证据 | 内容 |
|---|---|
| DSDT 大小 | 661,436 字节 |
| OEM 标识 | `OEM=ALASKA` / `TABLE=A M I` → **AMI BIOS** |
| `INOU0000` 出现 | DSDT 中 1 次，位于 `\_SB.INOU` 设备：`Name(_HID,"INOU0000")`、`Name(_UID,0)` |
| `_STA` 方法 | **不存在** → 按 ACPI 规范默认视为 present + enabled |
| 同设备内其他成员 | `Mutex(UWOL)`、`Name(CSTM,0)`、`Name(PODB,80)`、`_CRS`（FIXED_IO 资源 A1FE..A20D）、`T3PC`、`AX22/AX42/AX62/AX92/AXB2`、`AP02/AP03`、`AS72/AS73`、`MX24`、`A324` 等 |
| `UNIW0001` | 位于 `\_SB.PC00.I2C0.TPAD`，`_CID="PNP0C50"` → 确认是 I2C-HID **触摸板**，与 EC 控制无关 |
| DSDT 规模统计 | 428 个 Device、258 个 `_STA`、281 个 `_HID` |

**注意**：`\_SB.INOU` 无 `_STA` 却被 Windows 跳过枚举，机制未完全查清（可能与 `_CRS` 资源声明或 AMI 平台的 ACPI 枚举时序有关）。
经验事实：**`pnputil /scan-devices` 可以把它枚举出来**。

## 本机硬件事实

- 厂商/型号：`MECHREVO` / `Kuangshi16Pro Series GM6PQ0X`（旷世16 Pro）
- BIOS：`N.1.06MRO16`（2024-08-08）
- CPU：Intel i9-12900HX（12 代，`Family 6 Model 151`）
- GPU：NVIDIA RTX 4060 Laptop + Intel UHD + GameViewer 虚拟显示器
- 官方 EC 项目代号：740 → 本机值（未读到，因 B1）
- BIOS 项目 ID：`IDX`
- 已装 ControlCenter3 三份：`5.17.49.10`、`5.17.49.19`（当前用）、`5.56.51.26`（新版，仅 UWP 部分）
- 当前为管理员（`IsInRole Administrator` = True）

## 验证日志

| 日期 | 操作 | 结果 |
|---|---|---|
| 2026-10-16 | `sc start UWACPIDriver` | RUNNING（可逆，`sc stop` 还原） |
| 2026-10-16 | 内核对象枚举 `\Driver` | 找到 `\Driver\UWACPIDriver` |
| 2026-10-16 | 内核对象枚举 `\Device` | 491 个，无 ACPIDriver |
| 2026-10-16 | 启动官方控制台 | GamingCenter3 137 MB + SystrayComponent 41 MB |
| 2026-10-16 | 官方 MQTT 端口 13688 | 未监听 |
| 2026-10-16 | `ilspycmd` 反编译 SystrayComponent.exe | ✅ 3337 行 |
| 2026-10-16 | `ilspycmd` 反编译 GamingCenter3_Cross.dll | ❌ .NET Native AOT |
| 2026-10-16 | 解析 GamingCenter2.reg | ✅ 40 个键 |
| 2026-10-16 | DSDT 分析（661 KB） | ✅ 确认 `INOU0000` 定义存在，无 `_STA` |
| 2026-10-16 | `pnputil /scan-devices` | ✅ `ACPI\INOU0000\0` 设备出现 |
| 2026-10-16 | EC 全量扫描 0x0000-0x0FFF | ✅ 4096 地址 / 2002 非零 / 6.6s |
| 2026-10-16 | **Rust 核心库单测** | ✅ **26 passed + 1 doctest** |
| 2026-10-16 | **Rust 真机集成测试** | ✅ **6 passed**（含真实写入+回滚） |

### 真机测试详细输出（`cargo test --test hardware -- --ignored --nocapture`）

```
✅ EC 设备可访问: \\.\ACPIDriver
项目 ID = 16
传感器快照: SensorSnapshot { project_id: 16, power_source: 7,
             cpu_fan_rpm: 3970, gpu_fan_rpm: 75 }
Gaming: PowerLimits { pl1: 75, pl2: 75, pl4: 200 }
Office: PowerLimits { pl1: 45, pl2: 45, pl4: 200 }
✅ 写通道可用 (回写原值 75 成功)
--- 真实写入测试 ---
改动前 Office: PowerLimits { pl1: 45, pl2: 45, pl4: 200 }
目标   Office: PowerLimits { pl1: 50, pl2: 45, pl4: 200 }
改动后 Office: PowerLimits { pl1: 50, pl2: 45, pl4: 200 }   ← 真实生效
回滚 3 个寄存器
还原后 Office: PowerLimits { pl1: 45, pl2: 45, pl4: 200 }   ← 完全恢复
```

## 工具与产物

| 路径 | 用途 |
|---|---|
| `tools/ec_probe.py` | EC 只读连接探测 |
| `tools/nt_enum.py` | 内核对象命名空间枚举 |
| `tools/parse_reg.py` | 官方 .reg 能力表解析 |
| `tools/strs.py`（在 /tmp） | PE 字符串提取 |
| `scripts/probe_runtime.ps1` | 官方进程运行态采集 |
| 待固化：`tools/peinfo.py`、`tools/peimp.py` | PE 头/导入导出表分析 |

**环境注意**：
- `python`/`7z` 未进 PATH，需全路径（`/c/Python314/python.exe`、`/c/Program Files/7-Zip/7z.exe`）
- `powershell.exe` 未进 PATH，用 `/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe`
- 中文输出乱码：`-I` 隔离模式会忽略 `PYTHONIOENCODING`；改用 `python -X utf8`
- MSIX 内部用 DEFLATE64，Python `zipfile` 读不了，须用 7-Zip
- `ilspycmd` 已装（11.1.0），位置 `~/.dotnet/tools/ilspycmd.exe`

**Tauri / 桌面构建陷阱（都踩过，务必记住）**：

1. **`rc.exe` 必须用新 SDK** —— `embed-resource` 会选中很老的 `rc.exe`（实测 10.0.10011），
   在含中文的路径下编译资源直接失败。构建前 `export PATH=".../Windows Kits/10/bin/10.0.26100.0/x64:$PATH"`。
2. **必须生成 `icons/icon.ico`** —— 只给 PNG 时 tauri-build 会报 "icon.ico not found"。
3. **不要自定义 Windows 清单** —— 手写清单一旦有细微问题（实测 XML 报错）会导致进程无法加载。
   DPI 感知改用运行时 `SetProcessDpiAwarenessContext`。
4. **DPI 感知必须声明** —— 否则在 150% 缩放下窗口被放大 1.5 倍，超出屏幕、右侧被裁且模糊。
5. **窗口尺寸/定位统一用物理像素** —— Tauri 的 `LogicalSize`/`center()` 在 150% 缩放下换算不一致，
   会让窗口跑到屏幕外。改为 `PhysicalSize` + 贴左上角固定边距。
6. **前端资源在 `main.rs` 重编译时才重新嵌入** —— 只改 `app/ui/*` 后需要 `touch src/main.rs` 再 build，
   否则跑的还是旧资源。
7. **本机 DPI = 150%**（DPI 144），屏幕物理 1707×1067，逻辑约 1138×711。
8. **截图工具本身是 DPI 不感知的** —— PowerShell 的 `GetWindowRect` 返回虚拟化坐标，
   会导致截图偏移。诊断 UI 用全屏物理截图，或把数值render 到界面上直接读。

## 合规边界

所有参考项目（LumaDesk、open-revo）都**不分发**官方二进制与反编译输出，只发布自研实现。
**我们的公开仓库也应照做**：反编译产物留在本地（`reverse/`），仓库只放自研代码与协议文档。

## 下一步

> 2026-10-24 更新：B1 早已解决（见上）；v0.1.1 护栏补丁已落地。
> 任务状态的权威来源改为 `project_specs.md`，改进计划见 `docs/计划书/下一步改进指南.md`。

1. **v0.2.0（已拍板，下一站）**：温度寄存器保守探测（只读、≤50 次/秒、不施负载、用户在场）
   → 交叉核对 → 登记 `AddrStatus` → UI 温度卡
2. v0.2.1：CI + 覆盖率基线 + 打包（msi/nsis）验证
3. v0.3.0（blocked：需第二台机型）：寄存器档案外置 + 能力协商 + 未知机型安全降级
4. 可选静态深化：反编译 `5.56.51.26` 版 SystrayComponent 对比差异；解析 `resources.pri`；
   `UserFanTables/PH6PG*` 与官方 profile 交叉比对建立机型映射表

## 2026-10-25 · 终局审计与定向修复（执行中）

### 技能检查记录

- 已读：`AGENTS.md`、`project_specs.md`、`.agent/skills/project-audit-sop.md`、`code-review-checklist.md`、`.agent/memory/known-issues.md`、`verified-scope.md`、`policy.md`、`test_ledger.md`。
- 缺失：`.wolf/OPENWOLF.md` / `cerebrum.md` / `anatomy.md` / `memory.md` / `buglog.json`；`.agent/skills/add-new-feature.md`。
- 工具状态：仓库没有 `.codegraph/`，`code-review-graph` 无法连接；按工具返回改用 Read/Grep/graft。`reviewer` / `doc-auditor` 类型不存在，以独立 `claude` 代理承担同名只读职责。
- 没有执行被禁止的 `scripts/e2e_ui.ps1`、CDP 真机脚本，不做压力测试。

### 任务图

| ID | 工作 | 验收 | 状态 |
|---|---|---|---|
| A-01 | 基线测试与独立审查 | 全量测试/审查清单 | doing（等待独立 reviewer） |
| A-02 | 采样正确性与超时、EC 降级 | fixture + 前端路径 + lint | doing（定向修复完成，最终验证中） |
| A-03 | 文档/计划/记忆/证据闭环 | 审计清单与证据一致 | doing（文档审计代理进行中） |
| A-04 | release 构建、CI、发布 | 构建与真实 CI、Release 可复核 | todo（待 Approve） |

### 已验证事实（按时间）

- `cargo test --offline --bin mechrevo-max sysinfo::tests`：新边界测试加入后，当前修复下 **10/10 passed**。
- `core` 使用明确可用的 `RUSTDOC=~/.rustup/toolchains/1.95.0-x86_64-pc-windows-msvc/bin/rustdoc.exe`：**70 passed + 1 doctest**；两个 crate 的 fmt/clippy 在对应命令中通过。
- `app`：**31 passed**，`ipc_contract`：**8 passed**，严格 clippy 通过。
- 独立 Chromium + 假 IPC 执行 `node tools/qa_browser.cjs`：**17/17 断言**，`errors=[]`；覆盖 EC 不可用保留免驱数据、无电池/未知 GPU/未知内存显示 `—`、曲线失败退避、诊断字段。
- 脚手架修正（不是产品修复）：初始 mock 未提供首屏状态，导致 `waitForFunction` 超时；改为首屏注入有效状态。

### 修复与回滚点

| 文件 | 行为变化 | 回滚 |
|---|---|---|
| `core/src/ec/mod.rs` | `read_block` 拒绝越界和 u16 溢出，不再环绕；+2 单测 | `git checkout <before> -- core/src/ec/mod.rs` |
| `app/src-tauri/src/sysinfo.rs` | 外部命令 15 秒超时；GPU `[N/A]` 返回 None；电池缺字段不伪造 0%；磁盘必需容量；失败缓存/未知态 | 同上 |
| `app/src-tauri/src/main.rs` | 采样移至 EC 锁外，EC 失败/锁中毒仍保留免驱数据；版本与日志路径进 payload | 同上 |
| `app/ui/app.js` | Option 数据前端安全显示；曲线失败 30 秒退避；版本/日志目录接通 | 同上 |
| `tools/qa_browser.cjs` | 可复跑的独立 Chromium 验收，不访问硬件 | 删除文件即可（保留 mock 验收报告） |

### 自反驳

1. **“免驱数据”是否还会被 EC 拖住？** 初次修复仅把采样移到打开 EC 之前，仍持 EC 锁；复核后已移到锁外，锁中毒也返回采样。
2. **使用 Option 是否真的杜绝假 0？** 电池 `percent`、内存两字段均改 Option；GPU 无效行拒绝；无电池不构造对象。前端测试覆盖未知值而非仅 Rust 字段。
3. **当前版本是否完整替代官方？** 否。MUX、风扇曲线写入、系统实际生效 PL 尚未安全实现；不纳入本轮宣称。
4. **独立审查是否已完成？** 否，后台代理未完成前不能写 Approve 或“全部任务完成”。

### 独立审查（已完成）

- 代码 reviewer：先 **Request Changes**，指出 `BatteryTemperature.Temperature` 的单位错误（2982 会显示约 298°C）。
- 修复：按十分之一开尔文转 °C（`/10.0 - 273.15`），范围校验 + fixture；定向 `sysinfo::tests` **11 passed**，直接抽取并执行查询脚本得到合法 JSON，缺失温度 `null`。
- 复验：**Approve**（限当前源码静态复核与已提供的测试证据；真实窗口/硬件行为不在批准范围）。
- 文档 reviewer：扫描 **39 个已跟踪 Markdown**；有陈旧状态需纠正，**没有应删除的计划文档**。
- 归档：`docs/计划书/闭环审计-20261024.md` 已复制至 `docs/archive/plans/20261025/`，`cmp` 一致，manifest 记录 SHA256 与回滚；原件移除。其它未闭环文档保留。
- 结果：A-01 done；A-02 代码修复 done（后续完整验证后发布）；A-03 文档/记忆修复进行中；A-04 待构建/CI/Release。

### 收尾验证与发布（2026-10-25）

- 全量 core：**70 passed + 1 doctest**；严格 clippy / fmt 通过（rustdoc 显式使用 rustup 工具链真实路径，未跳过 doctest）。
- 应用：**32 passed**；契约 **8 passed**；严格 clippy / fmt 通过。
- 采样定向：**11 passed**；独立 Chromium + mock IPC：**21/21，errors=[]**。
- release 0.5.1：`app/src-tauri/target/release/mechrevo-max.exe`，**5,395,456 B**，SHA256 `dc2c0330c4f6e09e5a70e1917d4ae21d15381afb6973b2a3446ea8c6e2970e56`。
- 文档归档 manifest：`docs/archive/plans/20261025/archive-manifest.md`；历史审计保留可回滚副本。
- 报告：`reports/change-report-2026-10-24.html` 第 13 节，5/5 自测（实际逐题核对）；HTML 标签配对通过。
- 发布状态：本地验证和独立 Approve 已完成，开始提交/推送/CI/Release；**CI 未通过前不宣称远端发布验证完成**。

### Release v0.5.1（API 已复核，CI 进行中）

- commit：`c40616c`；tag：`v0.5.1`；已推送。
- [Release v0.5.1](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.5.1)：API 返回 `draft=false`，`mechrevo-max.exe` **5,395,456 B**，与本地 SHA256 `dc2c0330…0e56` 对应。
- CI：已启动，等待三个 job 真实完成；package 产物随后上传，未上传前不宣称完整发布。

### CI 与完整 Release（已完成）

- [CI run 40133863688](https://github.com/lza6/Mechrevo-Max/actions/runs/40133863688)：**build · test · lint、coverage、package 全部 success**。
- Release API 复核三资产：`mechrevo-max.exe` 5,395,456 B、`Mechrevo-Max_0.5.1_x64_en-US.msi` 2,088,960 B、`Mechrevo-Max_0.5.1_x64-setup.exe` 1,441,207 B。
- `main` 已推送；`v0.5.1` tag 已推送；没有 force push。
- 结论：A-04 done（构建/CI/Release）；干净系统安装仍不属于本次 CI 覆盖，保留为独立未验证项。
- 本轮可以停止的条件：P0/P1 修复并复核、测试/契约/浏览器验证、独立 Approve、文档与记忆同步、CI 与 Release 完成。

## 2026-10-25 · 发布后独立可靠性复审（v0.5.2 候选，进行中）

只读实践检查返回 4 项真实缺陷，不因为 v0.5.1 已发布就忽略：

| 项 | 修复与证据 | 状态 |
|---|---|---|
| EC 部分读取失败被 `.ok()` 吞掉 | 字段级 `read_warnings`，有效数据保留；前端警告与下一轮清除已测 | done（待复审） |
| 配置覆盖可能损坏/静默丢非法项 | 临时文件 + sync_all + Windows 原子替换 + `.bak`；NeedsAttention 禁止覆盖；替换失败注入原文件不变 | done（待复审） |
| 外部进程管道/回收边界 | 并发排空、输出上限、失败即终止、外层 future 超时、句柄关闭；真实 helper 4/4 | done（待复审） |
| 任意成功清除其它命令失败 | `(command, message)` 精确归属；A失败→B成功不清错，A重试成功才清除 | done（待复审） |

验证（本轮实际执行）：
- 应用 **44 passed**（含配置锁中毒）；配置 **11 条**（含替换失败、备份恢复）；真实子进程 **4/4**（含 cmd+ping 的继承 pipe）。
- 独立 Chromium + mock IPC **28/28**，含部分 EC 失败、配置损坏提示与显式恢复确认；此前的 mock 标志问题已修正，不以它为产品缺陷。
- 独立 reviewer 第一轮 Request Changes；修复后复验 **Approve**（限定最小范围，不覆盖异常 Windows 环境、真机硬件或真实 GUI）。

边界：
- 超时/输出超限使用显式 `TerminateJobObject` 结束进程组，真实 cmd+ping 的继承 pipe 用例已验证；未覆盖外部安全软件禁止 Job 的环境，失败按 None 返回。
- 配置 `.bak` 保留上一份成功配置；未做真实断电实验；读改写已用 `AppState.profiles` 互斥（profiles → ec）。
- 本轮不开放风扇曲线写入、MUX、系统实际生效 PL、多机型支持。

### 本轮走过的弯路（保留为审计证据）

- 初版超时只杀直接子进程，cmd 被杀但 ping 继承 pipe 仍悬挂；没有满足“有界”。
- 多次 Job 草稿曾重复 spawn、未真正绑定 pipe/超时，已删除，**未进入提交**。
- 最终实现是单个 `Command::spawn` + `OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE)` + `AssignProcessToJobObject` + 超时 `TerminateJobObject`。
- `AssignProcessToJobObject` 失败的根因是缺少 `PROCESS_SET_QUOTA`；补上后真实 4/4 通过。
- 旧成功清除同命令新失败的问题，用 generation 而不是仅按命令名解决。

### v0.5.2 发布进度

- commit `d9ef361`、tag `v0.5.2` 已推送；Release API 复核 `draft=false`，exe 5,446,144 B，与本地构建一致。
- [CI run 40155344884](https://github.com/lza6/Mechrevo-Max/actions/runs/40155344884)：verify / coverage / package **全部 success**。MSI/NSIS 上传 Release 后三资产需再复核。

### v0.5.2 完整 Release 已复核

- `draft=false`；[Release v0.5.2](https://github.com/lza6/Mechrevo-Max/releases/tag/v0.5.2)。
- exe **5,446,144 B**（sha256 `8935faff28a73ce0c2712c0a6daf4f8a2f7fab6a871dd855c82ed763b53f1a65`）、MSI **2,113,536 B**、NSIS **1,450,375 B**。
- CI run **40155344884**：build/test/lint、coverage、package 全 success；MSI/NSIS 来自该 CI artifact 并上传。
- 最终源码提交 `d9ef361`；后续文档提交在 main，未 force push。

## 2026-10-27 · 第二轮：决策拍板 + 计划文档归档 + v0.6.1
| 项 | 结果 | 证据 |
|---|---|---|
| 抽验 | core 88 / app 44 / 契约 8 / 前端 33 断言全绿，工作区无漂移 | 本轮 Bash 输出 |
| Q-04/Q-05 拍板 | Q-04 不做自动更新；Q-05 只要托盘（v0.7.0） | .agent/memory/decisions.md；改进指南 §17（归档副本） |
| 计划文档清理 | 下一步改进指南.md 归档删除（manifest + 可回滚）；悬空引用 ×3 修复 | docs/archive/plans/20261027/ |
| 发版 | v0.6.1 文档版（无二进制，notes 明确声明）；tag+release 已推送；CI success | run 40346143363；Release v0.6.1 |
