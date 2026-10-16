# 排障指南

> 按「现象 → 诊断 → 解决」组织。先跑 `tools/ec_health.py` 再看对应章节。

---

## 0. 第一步：跑健康自检

```bash
python -X utf8 -I tools/ec_health.py
```

它会检查关键寄存器合理性、风扇是否实时响应、连续读取稳定性，并直接给出结论。
**限速只读（50 次/秒），对硬件安全。**

---

## 1. 界面显示「EC 未连接」

### 现象
顶栏状态点是红的，提示「EC 通道不可用」。

### 诊断

```powershell
# 驱动在跑吗？
Get-Service UWACPIDriver

# 设备枚举出来了吗？
Get-PnpDevice | Where-Object { $_.InstanceId -like '*INOU*' }
```

### 原因与解决

| 诊断结果 | 原因 | 解决 |
|---|---|---|
| 服务 `Stopped` | 驱动未启动 | `sc.exe start UWACPIDriver` |
| 服务 `Running` 但**设备不存在** | **PnP 未枚举该设备**（最常见的坑） | `pnputil.exe /scan-devices` |
| 服务、设备都正常，但应用报权限不足 | 应用未以管理员运行 | 点界面上的「以管理员身份重启」按钮 |
| `\\.\ACPIDriver` 打不开且 errno=2 | 设备对象未创建 | 依次执行上面两条命令后**重启应用** |
| 驱动文件不存在 | 从未安装官方控制台 | 需先安装机械革命官方控制台以获取 `UWACPIDriver.sys` |

### 为什么会有「设备不枚举」这个坑

`UWACPIDriver` 是 **KMDF PnP 驱动**，绑定硬件 ID `ACPI\INOU0000`。
该设备定义在 BIOS 的 DSDT 里，但 **Windows 启动时不会自动枚举它**。
官方控制台的安装流程也没有触发这次扫描 —— 所以驱动加载了却没有设备可绑，
`EvtDriverDeviceAdd` 从不执行，设备对象 `\DosDevices\ACPIDriver` 因此不存在。

**这是官方控制台的一个落地缺陷**，也是我们必须在文档里显式说明的原因。

---

## 2. 读数不刷新 / 显示「—」

| 现象 | 原因 | 解决 |
|---|---|---|
| 所有读数都是「—」 | EC 不可用 | 见第 1 节 |
| 风扇转速恒为 0 | 风扇未转（低负载时正常）或寄存器不可读 | 加负载后再看；用 `ec_health.py` 的「动态采样」段确认 |
| 数值卡住不动 | 轮询停止了 | 看状态栏是否有报错；重启应用 |

---

## 3. 切换档位/应用功耗没反应

| 现象 | 原因 | 解决 |
|---|---|---|
| 点档位按钮无变化 | EC 不可用，或写入被驱动静默丢弃 | 看状态栏报错；跑 `ec_health.py` 确认档位字段 |
| 显示「写入校验失败」 | **正常的保护行为** —— 写入后读回不一致，已自动回滚 | 说明该寄存器不可写或值非法；不要重试同一操作 |
| 应用后数值又跳回 | 轮询读回了硬件真实值（说明写入未生效） | 正常情况下硬件接受了才会保持；持续如此说明写不进去 |

> **重要**：官方驱动存在把失败**伪造成成功**的代码路径，所以本应用**不信任**
> `DeviceIoControl` 的返回值，一律写后读回校验。看到「校验失败」是保护机制在工作。

---

## 4. 界面显示错乱 / 右侧内容被裁掉

**原因**：DPI 感知未生效。在 150% 缩放的屏幕上，窗口会被放大 1.5 倍，超出屏幕。

**诊断**：

```powershell
# 查看系统缩放
Get-ItemProperty "HKCU:\Control Panel\Desktop" | Select-Object LogPixels
```

**解决**：本应用已在启动时调用 `SetProcessDpiAwarenessContext`。
若仍出现，说明该版本缺失此调用 —— 检查 `src/main.rs` 的 `enable_dpi_awareness()`。

---

## 5. 构建失败

| 错误信息 | 原因 | 解决 |
|---|---|---|
| `RC.EXE failed to compile specified resource file` | `rc.exe` 选中了旧版本 | 把新 SDK 加进 PATH（见 `docs/DEPLOYMENT.md` §2.1） |
| `` icons/icon.ico` not found `` | 缺 ICO 文件 | 项目已含 `icons/icon.ico`；若丢失需重新生成 |
| `unknown field requestedExecutionLevel` | 用了旧版 Tauri 配置字段 | Tauri 2.12 的 `bundle.windows` 不再支持该字段 |
| 改了 UI 但界面没变 | 前端资源未重嵌入 | `touch src/main.rs && cargo build` |

---

## 6. 应用启动后立刻消失

```powershell
# 前台运行看报错
cd app/src-tauri
.\target\release\mechrevo-max.exe
```

若 bash 下直接运行报 `Permission denied`，是因为可执行文件带提权清单 ——
用 PowerShell 的 `Start-Process` 启动。

---

## 7. ⚠️ 机器意外重启过

**这跟本应用无关，但必须知道。**

若你的机器发生过「无转储的硬复位」（开机后事件日志有 `Event 41 Kernel-Power`，
且**没有** BugCheck / WHEA），排查方向：

```powershell
# 用项目自带的脚本一次性查清
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/diagnose_reboot.ps1
```

脚本会列出：意外关机时间点、是否有蓝屏、硬件错误、热区事件、转储文件。

常见原因（按概率）：
1. **散热**（积灰、硅脂老化）—— 最常见，尤其高负载时
2. **BIOS 缺陷** —— 检查厂商是否有 BIOS 更新
3. **供电**（适配器功率不足、电池老化）
4. **内存不稳** —— 跑 `mdsched.exe` 内存诊断

> **不要**用 CPU/GPU 满载压力测试去复现 —— 那会真的把机器推到硬复位。
> 详见 `docs/SPEC.md` §5.1。

---

## 8. 收集诊断信息

需要报问题时，跑这些并把输出附上：

```powershell
# 硬件健康
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/hw_health.ps1

# 重启历史（若有异常重启）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/diagnose_reboot.ps1

# EC 状态
python -X utf8 -I tools/ec_health.py

# 驱动与设备
powershell -NoProfile -Command "Get-Service UWACPIDriver; Get-PnpDevice | Where-Object { \$_.InstanceId -like '*INOU*' }"

# 版本
(Get-Item app/src-tauri/target/release/mechrevo-max.exe).VersionInfo
```

**注意**：`reverse/evidence/*.json` 含本机特征值（项目 ID 等），
分享前请自行判断是否脱敏。
