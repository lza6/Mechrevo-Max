# 变更日志

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

---

## [0.1.3] — 2026-10-24

**执行线程修正版。** 独立调研（源码级核对）发现：Tauri v2 的 `#[tauri::command]` **默认在
WebView2 的 UI（主）线程上同步执行**，而 `get_status` 一轮约 14 次 IOCTL、受「两次访问不重叠」
闸门约束 ≈ 280 ms —— 等于每 2 秒卡住窗口消息循环 280 ms。这是 P3-11（「280 ms 持锁」）的**真正根因**，
只削 IOCTL 次数治不了。

### 修复

- **6 个 IPC 命令全部改为 `async fn` + `tauri::async_runtime::spawn_blocking`**：
  命令本体只做参数解析，EC 访问搬进 blocking 线程池，UI 线程不再被阻塞。
  - `AppState` 改为 `Arc<AppState>` 共享（`State<'_, Arc<AppState>>`），因为借用型
    `State<'_, T>` 不能搬进 `'static` 的 blocking 任务。
  - `get_status` 改为返回 `Result<StatusPayload, String>`（「含引用的 async 命令必须返回
    `Result`」是 Tauri 的编译期约束），但**永远返回 `Ok`**：EC 不可用仍用结构化载荷表达，
    **前端看到的载荷形状与命令名/参数完全不变**，`ipc_contract.rs` 与 `IPC` 表零改动。
- **`undo_power` / `undo_mode` 先判断有无记录、再碰硬件**：无记录时给准确文案
  （「没有可撤销的功耗改动」），而不是先报「EC 不可用」；顺带让这两条路径可离线单测。

### 测试

- IPC 命令层单测 **4 → 7**：新增「锁中毒时 `status_snapshot` 返回 restart 提示且不带任何读数」、
  「无功耗记录时的准确错误」、「无档位记录时的准确错误」。
- 前端 37 条浏览器断言重构后**复跑仍 37/37 通过**，`pageerror`/`console.error` 各 0。
- 两个 crate 的 `clippy --all-targets` 0 warning、`fmt --check` 干净；release 构建成功。

### 文档事实更正

- `policy.md` §1 原写「`cargo-llvm-cov` 未安装、无基线」——**已过期**。实测工具已装（0.9.1），
  `core` 行覆盖 **86.56%**（排除 `ec/windows.rs` 后 **97.1%**）。同时把「100% 分支」改成
  「行覆盖 ≥95% + 每个 `Result::Err` 分支都要有测试」，因为 Windows/MSVC 下分支覆盖不可测。
- 新增接缝 S11（命令执行线程）到 `test_ledger.md`。

### 已知限制

- UI 线程不再被阻塞属**静态确认 + 合理推断**（源码链路完整：`tauri-macros` 默认 Blocking、
  阻塞分支不 spawn、官方 doc 明写 async 才不阻塞主线程）；**改前/改后「拖窗口是否掉帧」的对照
  观察仍需真机由用户完成**。

---

## [0.1.4] — 2026-10-24

把第 3 轮调研暴露的 3 个 P1/P2 缺口一次做掉：**release 下失败不可见**、**离线机器装不上**、**读屏念不懂**。

### 修复

- **N1（P1）日志落盘 + panic hook**：新增 `app/src-tauri/src/logging.rs`。
  release 是 `windows_subsystem = "windows"`，没有 stderr —— 此前 `RestoreGuard` 的
  「自动回滚失败，硬件可能停在中间状态」以及 Tauri 自身 20 处 `log::error!` **全部被丢弃**。
  现在统一写入 `%LOCALAPPDATA%\Mechrevo-Max\logspp-<天>.log`（绝对路径，因为提权实例
  的 cwd 可能是 `System32`），按天滚动、保留 7 天、单次运行上限 5 MB，日志失败静默降级不影响业务；
  同时装 `panic::set_hook`，panic 也会落盘。
- **N1（P1）失败原因常驻界面**：`AppState` 新增 `last_error`，失败时记录、成功时清空，
  由 `get_status` **回放**（不用一次性 `emit` —— 监听未注册或窗口销毁时事件会直接丢）。
  前端新增常驻错误条：此前失败只闪一下状态栏，2 秒后就被轮询冲掉，用户回头看不出发生过什么。
- **N2（P2）离线可用**：`bundle.windows.webviewInstallMode = skip`。默认是
  `DownloadBootstrapper`（**需联网**），对一个"离线工具"是隐性自毁：没有 WebView2 的机器装不上。
  改为依赖机器已有运行时（Win11 内置、Win10 随 Edge 推送），并在 `docs/DEPLOYMENT.md` 写明前提。
- **N3（P2）可访问性四处**：
  ① 滑块 `<label>` 不再包住 `<output>`（原来可访问名会随数值漂移，读屏念「PL1 长时 45 45 45」），
  改成 `label for=` + `aria-describedby`；② 滑块补 `aria-valuetext`（念「75 瓦，长时功耗墙 PL1」
  而不是裸「75」）；③ 告警区外挂常驻 `.sr-only` `aria-live="assertive"` 区
  （`role="alert"` + `hidden` 切换的播报不稳定）；④ 功耗 tab 补 `role="tablist"/"tab"/"tabpanel"`
  与 `aria-controls`。档位原始寄存器也改成 `aria-label` 说人话。

### 工程

- **CI 硬化**：`Swatinem/rust-cache`（替代只按 Cargo.lock 哈希的手写 cache）、
  `permissions: contents: read`、`concurrency` 取消旧运行、新增 **coverage job**
  （`cargo llvm-cov` 上报，暂不阻断）、新增 **package job**（tag 或手动触发时真跑
  `npx tauri build` 产出 msi/nsis 并上传 artifact —— 这是关闭 P2-08「安装包从未构建」的唯一手段）。
- 新增 `rust-toolchain.toml` 固定 1.95.0，避免「昨天绿今天红」。

### 测试

- IPC 命令层单测 **7 → 12**（新增 3 条日志模块：落盘、只删本模块过期文件、保留新文件；
  2 条失败可见性：`note_result` 记录/清空、`unavailable` 载荷不带陈旧错误）。
- 前端断言 **37 → 47**：新增 6 条可访问性（`aria-valuetext`、`label for` 不包裹、
  `aria-describedby`、寄存器 `aria-label`、tabs 语义、常驻 live 区播报）+ 2 条持久错误条 + 1 条失败态。
  **47/47 通过，`pageerror`/`console.error` 各 0**。
- core 46 + 1 doctest、契约 8、两个 crate clippy 0 warning、fmt 干净、release 构建成功。

---

## [0.1.5] — 2026-10-24

**打包配置修复。** CI 的 package job 第一次真跑 `npx tauri build`，立刻暴露了
「MSI/NSIS 从未构建验证」（P2-08）的真实原因：

```
failed to bundle project: `Couldn't find a .ico icon`
```

`bundle.icon` 里只列了两个 PNG，**从未包含 `icons/icon.ico`** —— 也就是说这份配置
在 Windows 上根本不可能打出安装包。讽刺的是 `docs/DEPLOYMENT.md` §2.2 一直写着
「必须存在 `icons/icon.ico`」：**文档是对的，配置是错的**，而没有人真跑过一次打包。

### 修复

- `bundle.icon` 补上 `icons/icon128.png`、`icons/icon256.png`、`icons/icon.ico`
- 发布清单与 `docs/DEPLOYMENT.md` 增加一条：**打包必须在 CI 上真跑一次**（现在由 tag 触发的
  package job 保证），而不是只看配置里写了 `targets`

### 说明

- 本版**无业务代码改动**；exe 行为与 v0.1.4 一致，改动只在打包资源列表与版本号。
- v0.1.4 的 exe 仍是有效的绿色版产物；v0.1.5 的意义是让 msi/nsis 真的能生成。

---

## [0.2.0] — 2026-10-24

**温度读数上线 —— 而且是自己测出来的，不是抄来的。**

### 新增

- **CPU / GPU 温度读数**（界面 + `Sensors` + IPC）
  - CPU `0x043E`、GPU `0x044F`。社区两份参考实现（roj234 的 `ec_api.py` 44 个 `REG_`、
    LumaDesk 的 `ec_protocol.py`）**都没有温度寄存器**，因此本机自行测定。
  - 判定方法（不靠"数值看起来像温度"）：
    - 用 **ACPI 热区**（`\Thermal Zone Information(*)\Temperature` 的 `\_sb.ectz`）做 CPU 参照，
      9 组配对样本上 `0x043E` 与参照**相关 r=+0.857**、绝对偏差 ≈+2 °C；
      并在「单线程升温 40 s → 冷却」实验中同步上升 +5.5 °C、随后回落。
    - 用 **`nvidia-smi`** 做 GPU 参照（NVAPI 不经过 EC，可并发而不抢通道），
      80 组并发样本上 `0x044F` **相关 r=+0.989**、偏差 ≈+1 °C。
  - 界面新增两张温度卡：`<60 °C` 正常、`60–79` 偏高、`≥80` 过热，并给行动建议。
- 新增探测工具（只读、强制限速）：
  - `tools/ec_temp_probe.py`：区间扫描 / 候选时间序列采样，50 次/秒、硬上限 100 次/秒
  - `tools/ec_temp_correlate.py`：三阶段（静置 → 单线程升温 → 冷却）+ ACPI 参照的分阶段相关性实验

### 修复（真机状态暴露的 bug）

- **强冷位让档位识别失效**：真机实测读到 `0x0751 = 0x40`（bit6=强冷开、档位=均衡），
  而 `from_field1` 直接按整字节匹配 → 返回 `None` → 界面不高亮任何档位。
  现在解析前先 `& MODE_MASK (0xBF)` 屏蔽 bit6；`0x40/0x50/0xE0` 均能正确识别。

### 实测记录（顺带发现的两条工程事实）

- **ACPI 温度轮询会抢 EC 通道**：并发跑 `MSAcpi_ThermalZoneTemperature`（WMI）或
  `Get-Counter`（性能计数器）时，直连 EC 的单次读取从 ~20 ms 恶化到 ~1.5 s（**慢 75 倍**）。
  因此任何"EC 采样 + ACPI 参照"的实验都必须**分阶段**做，不能并发。
- WMI 的 `MSAcpi_ThermalZoneTemperature` 在本机读数是**冻结**的（4 分钟内恒为 66.05，
  期间温度实际变化 5 °C），`Get-Counter` 才是活数据 —— 以后做参照别用前者。

### 测试

- 核心单测 **46 → 49**（CPU/GPU 温度读取、温度读失败必须报错、强冷位不破坏档位识别）
- 前端浏览器断言 **38/38**（含温度三档判定、无数据降级、5 张卡不溢出、A11y 复验），0 error

---

## [0.3.0] — 2026-10-24

**界面按「专业监控台」重做，并把黑匣子拆开。** 这一版几乎没有新增硬件知识，
全部是把**已经能读到的**东西正确地展示出来，加两个用户真正需要的开关。

### 新增（能力）

- **强冷模式开关**：`0x0751` 的 bit6。与档位共用同一字节，因此**读改写**（只动 bit6，
  不破坏档位编码），写后读回校验，失败自动回滚，可单独撤销。
- **自定义配置（Profile）**：把「PL1 + PL2 + PL4 + 是否强冷」存成一条，一键套用。
  存 `%LOCALAPPDATA%\Mechrevo-Max\profiles.json`；最多 32 条；同名覆盖；
  文件损坏时改名 `.broken` 保留现场并降级为空列表（不让程序起不来）。
  应用配置时若是强冷那步失败，**把已写入的功耗墙一起回滚**，不留半套状态。
- **寄存器浏览器**：填 `0x751 0x730-0x732` 这类输入即可只读任意 EC 地址，结果可导出 CSV。
  前端与后端都做前置校验（单次 ≤64 个地址、地址 ≤0x0FFF），非法输入**不会发出 IPC**。

### 改进（专业感 / 透明度）

- **传感器表**（HWiNFO 风格）：CPU/GPU 温度、CPU/GPU 风扇、供电，每行显示
  **当前 / 最小 / 平均 / 最大** 四列 + 状态（正常 / 偏高 / 过热 / 静止 / 运转中 / 高转速）。
- **采样信息**：顶栏显示「间隔 2.0s · 实测 x.xs · N 次」，面板里显示"本次启动已采样 N 次 · 上次 x 秒前"，
  并提供「清零统计」。—— 之前用户完全不知道数据多快刷新、刷新了几次。
- **位域解释**：把 `751=0x40` 直接翻译成「档位编码 0x00 → 均衡 / bit6 强冷 开」，
  并明确写「档位由机器固件/EC 决定，本程序仅读取；启动时不会写入」——
  回答"我都没设置过，怎么就有模式了"这个疑问。
- **功耗墙说明**：明确标注这是**默认功耗寄存器**，实际生效还受固件/BIOS/系统策略约束；
  「系统当前实际生效值」需要读 MSR（0x610），用户态读不到（见已知限制）。
- 布局改为 4 个面板（传感器 / 功耗墙 / 自定义配置 / 寄存器浏览器），
  读数区从卡片改成表格，横向不溢出（有 `scrollWidth` 断言）。

### 测试

- 核心单测 **58**（+5 强冷开关：读位、只动 bit6、保留档位位、卡住回滚、可撤销；+4 Profile 校验）
- IPC 命令层 **20**（+6 配置持久化：往返、缺文件、坏 JSON 保留现场、同名覆盖/删除、超限、非法值；+2 寄存器边界）
- 契约测试 8（命令集扩到 **13 个**，双向比对通过）
- 前端浏览器断言 **43**（温度四列统计、位域解码、强冷写入、配置增删改、寄存器区间展开与越界拦截、EC 三态、布局不溢出），0 error

### 已知限制（新增两条，都跟"黑匣子"有关）

- **看不到"系统实际生效的 PL1/PL2"**：那需要读 Intel MSR 0x610（RAPL），用户态读不到。
  HWiNFO 是靠自带内核驱动读的；本项目不分发驱动，所以只能显示 EC 里的默认功耗寄存器。
  这是**有意的取舍**，不是遗漏。
- **风扇 PWM 百分比暂未展示**：`0x075B`/`0x075C` 已知是 CPU/GPU 风扇占空比（社区实现与本机 delta 一致），
  但"原始字节 → 百分比"的换算关系尚未在本机标定，因此没有硬塞进界面。

---

## [0.4.0] — 2026-10-24

**回答"界面没有真实的 PL1/PL2"：把能读的都读出来了，读不到的如实标注；风扇曲线先做只读。**

### 新增（真实数据，不是估算）

- **固件基线功耗（只读）**：`0x783`–`0x786` 实测为 **75 / 130 / 200 / 133**，与"EC 默认功耗"
  （`0x730`–`0x732` = 75/75/200）**不是一回事**。界面上两排并排显示，并说明区别。
- **风扇曲线只读展示**：EC `0x0F00`–`0x0F5F` 的 6 个 16 字节表，自动分类成
  「温度点 / 占空比」并给出**候选配对**曲线图（温度 → 占空比%）。全程只读，
  界面上明确写"本程序不会写这些表"。

### 关键更正的三个事实

1. **`0x783`–`0x786` 不是"当前生效 PL"**。做了一组决定性实验：
   把 `0x730` 从 75 改成 50 并写提交寄存器 `0x767` → `0x783` 不变；
   依次切办公/狂暴档再切回 → `0x783` 仍不变。结论：它是**固件/BIOS 基线**，不随应用写入变化。
   （此前本项目把它标成 `NotApplicable`「读出恒为 0」，现已更正：它有非零值，只是含义不是"生效值"。）
2. **写默认功耗寄存器在 EC 里没有任何可见的连带变化**：做了一次「全量 4096 地址扫描 →
   写 `0x730`=50 + 提交 → 再全量扫描」的差分，除 `0x730` 自身外**没有任何寄存器跟随**
   （其它变化都是风扇/计数器的自然漂移）。因此「写入是否真的约束了 CPU 功耗」**目前无法从 EC 证明** ——
   界面上这句话也如实写着，不再暗示"改了就生效"。
3. **官方 profile 里 PL 默认是不启用的**：`Mode1_Profile1.json` 显示
   `CPU.PL1_Enabled = False`、`PL2_Enabled = False`，且 `FAN.FanSwitchSpeedEnabled = 0`
   （对应 `0x787 = 0x81` = 关闭）。这解释了为什么"默认进去就有模式、但功耗没被改过"。

### 新增能力

- 命令 `get_fan_curves`（只读，14 个命令）；payload 增 `firmware_power`。
- 核心新增 `fancurve` 模块（6 个块读取 + 特征分类 + 候选配对）与
  `PowerController::read_firmware_baseline`，均有单测。

### 测试

- 核心单测 **60 → 68**（曲线模块 6 条、固件基线 2 条）
- 前端断言 **12 条**聚焦本轮新增（固件基线显示、曲线 6 块与 2 张图、只读声明、布局 5 面板）
- 契约测试 8（命令 14 个）；两 crate clippy 0 warning / fmt 干净

### 仍然读不到（写进界面，不再让用户猜）

- **系统实际生效的 PL**：需读 Intel MSR 0x610，用户态无权限；HWiNFO 靠自带内核驱动。
- **风扇曲线的官方数值**：官方 profile 只有表名（`M1T1`），曲线本体在 `UserFanTables/PH6PG*`
  资源里，本机备份没有 → 无法逐点交叉核对，所以只标注为"候选配对"。
- **写曲线的能力**：本轮**不做**。写错风扇可能不转导致过热，必须等只读展示被确认无害
  且与官方表核对后再评审。

---

## [0.5.0] — 2026-10-25

**按官方那套「侧栏 + 环形仪表 + 分区页面」重做界面，并在数据面上全面超过官方。**

### 界面

- 7 个分区：概览 / 系统监控 / 设备信息 / 电池 / 性能 / 风扇曲线 / 高级（侧栏导航，URL hash 路由）
- **概览**：8 个环形仪表（CPU 占用/温度、GPU 占用/温度/功耗、内存、电池、SSD 温度）+ 状态 chips
- **系统监控**：10 行传感器表，每行「当前/最小/平均/最大/状态」
- **设备信息**：CPU（型号/核心线程/占用/温度/风扇）、GPU（型号/占用/温度/功耗/核心与显存频率/显存）、
  内存、SSD（型号/容量/健康/温度/上限/磨损）、系统、本程序
- **电池**：电量与状态（充电/放电/接电）、满充容量、电压、EC 充电上限；健康页单独一栏
- **风扇曲线**：**自动加载**（不再需要点按钮），中文坐标轴（温度 °C → 转速 %），
  带网格与面积填充；6 张表按「温度点 / 转速档」分类列出
- 视觉：深空黑 + 青绿仪表、背景波纹、环形仪表阈值变色（正常/偏高/过热）

### 数据面（全部免驱，只读）

新增采集（`app/src-tauri/src/sysinfo.rs`，分三档刷新频率，全部不经过 EC）：

| 数据 | 手段 | 频率 |
|---|---|---|
| CPU 使用率 | `GetSystemTimes` 差分 | 每次轮询 |
| 内存占用 | `GlobalMemoryStatusEx` | 每次轮询 |
| GPU 使用率/显存占用/温度/核心与显存频率/功耗/显存用量 | `nvidia-smi` | >=3 s |
| 电池（接电/充电/放电/剩余/满充/电压/循环/温度） | WMI `BatteryStatus` / `BatteryFullChargedCapacity` / `BatteryCycleCount` / `BatteryTemperature` | >=60 s |
| SSD（型号/容量/健康/温度/温度上限/磨损） | `Get-PhysicalDisk` + `Get-StorageReliabilityCounter` | >=60 s |
| 静态（CPU 型号/核心线程/OS） | `Win32_Processor` / `Win32_OperatingSystem` | 启动一次 |

**比官方多给的**：GPU 功耗与显存实时用量、GPU 核心/显存频率、SSD 温度上限与磨损、
电池满充容量与电压、EC 充电上限、固件基线功耗、40 个 EC 寄存器浏览与导出。

**比官方更诚实的地方**：电池设计容量/循环次数/温度在本机 WMI **不暴露**，
界面显示「本机不提供」而不是编数字（官方的电池页只有电量和"剩余时间 --"）。

### 修复

- 风扇曲线不再需要手动点「读取曲线」：启动时自动读一次，进入该页时按需刷新（60 s 节流）。
- 仪表文字顺序改为「数值 + 单位」在上、名称在下（原来读成「18 % CPU 占用」）。

### 测试

- IPC 命令层 **20 -> 25**（新增 5 条 sysinfo：内存合理性、GPU 结构、慢查询解析、CPU 使用率区间、坏 JSON 不 panic）
- 前端 **44 条**逐页断言（7 个页面、8 个仪表、10 行传感器、电池"本机不提供"、曲线自动加载、寄存器边界），0 error
- 核心 68 单测 + 1 doctest；契约 8（14 命令）；两 crate clippy 0 warning / fmt 干净

### 已知限制

- **真窗口外观**：本机在多次启停后窗口状态异常（截到的是 15x15 的窗口、或另一会话桌面），
  因此完整外观用浏览器逐页截图验证；真窗口只确认「新界面已渲染、EC 已连接、采样正常」。
- 电池设计容量/循环/温度：本机 WMI 不提供。
- 风扇曲线写入仍未实现（等只读版被确认无害 + 与官方表核对）。

---

## [Unreleased]

### 工程化（2026-10-24，无代码改动，故未单独发版）

- 新增 `policy.md`：门禁数值按本项目实际填写（覆盖率现状、无服务端/无数据库的适用性排除、审查门禁、证据门禁）
- 新增 `test_ledger.md`：10 条接缝登记 + 测试状态 + "改这里就重跑那里"的映射
- 新增 `.github/workflows/ci.yml`：win-latest 上跑 core/app 的 fmt、clippy(-D warnings)、单测、契约、`node --check` 与合规自检；
  **不跑**真机测试与 GUI E2E（安全红线），不设覆盖率门禁（无基线）
- 新增 `docs/qa/ui-smoke-spec.md`：把"真机 UI 自动化"降级为"浏览器 + mock 后端"的 37 条断言规格，
  补齐此前完全缺失的前端渲染/交互验证；新增 3 张截图（ready / EC 不可用 / 需要管理员权限）
- 已知边界：本仓库不引入 npm/`package.json`（保持前端无构建步骤），因此冒烟不提供"一条命令复跑"，
  执行走 Playwright 环境；这是设计取舍，已写入规格文件

### 规划中
- CPU / GPU 温度读数（待确认寄存器）
- 自动更新机制
- 风扇曲线 / 电压 / MUX（高风险，需先评审）

---

## [0.1.2] — 2026-10-24

**独立复审后的修正版。** v0.1.1 宣称的两条核心保证（"限速不可绕过"、"不会并发冲进硬件"）
经独立审查判定**只成立一半**，本版把实现补齐；同时修正证据台账里超出实际范围的断言。

### 修复（v0.1.1 的能力声明超出实现）

- **限速器的锁没有覆盖实际访问**：`AccessThrottle::wait()` 在返回时就释放了锁，
  真正的 `DeviceIoControl` 在锁外执行 —— 若单次 IOCTL 耗时超过 20 ms，两次访问仍会重叠。
  改为 `AccessThrottle::run(|| ...)`：**整个访问过程持锁**，从等待间隔一直持到访问返回；
  间隔从"上一次访问结束"开始计时（更保守）。
- **限速状态是 per-instance**：原实现把闸门作为 `WindowsAcpiTransport` 字段，
  而每条 IPC 命令都 `WindowsAcpiTransport::new()` → 每条命令的第一次访问都是零间隔，
  任何新代码路径建个新 transport 就绕过了。改为**进程级单例** `ec::throttle::global()`，
  全进程共享同一份状态（新增 `global_throttle_is_shared_across_transports` 断言同一实例）。
- **补上真正能发现重叠的测试**：`run_never_allows_overlapping_access`
  用"比间隔还慢的模拟 IOCTL"跑 4 线程 × 5 次，断言闸门内并发峰值恒为 1。
  v0.1.1 的同名测试只断言总耗时，无法发现重叠（已删除该假信号）。
- **`mode.rs` 注释与实现矛盾**：注释称"撤销时再写一次提交触发即可"，但 `undo` 只回写两个
  档位字段、不写 `0x767`。改为与实现一致的表述，并明确标注"撤销后固件是否立即重新应用
  尚未在真机确认"（`docs/protocol/ec-baseline.md` §6）。

### 修复（工程质量）

- `app/src-tauri` 的 `cargo fmt --check` 此前**失败**（9 处），`cargo clippy --all-targets`
  有 3 个警告（`manual_clamp` ×2、`err().expect()` ×1）→ 全部修掉，两个 crate 现在都是
  fmt 干净 + clippy 0 warning。
- 证据台账此前把 `clippy` / `rustfmt` 记成无条件通过，实际只跑过 `core`；
  现在两处都明确标注作用范围并补上 `app` 的实测结果。

### 优化

- `get_status` 合并 `0x751` 的重复读取（`current()` + `raw_fields()` 各读一次 → 只读一次），
  每轮轮询少一次 EC 访问，即少 20 ms 延迟。

### 测试

- 核心单测 **44 → 46**（新增闸门重叠检测与进程级单例断言）。
- 实际执行：`cargo test` 46 passed + 1 doctest；`cargo test --bin mechrevo-max` 4 passed；
  `cargo test --test ipc_contract` 8 passed；两个 crate 的 `clippy --all-targets` 均 0 warning；
  两个 crate 的 `fmt --check` 均 exit 0；`cargo build --release` 成功
  （`mechrevo-max.exe` 4 994 048 B，sha256 `c85a29ea…f33363`）。

### 复审遗留（未在本版处理，已记录）

- `get_status` 单轮约 14 次 IOCTL ≈ 280 ms，全程持有 EC 互斥 → 用户点"切档"最多排队约 300 ms
  （可用性可接受，但未做真实窗口帧率测量，标"待验证"）。
- `RestoreGuard` 在 release 下 `Drop` 失败只 `eprintln!`，而 release 无 stderr 去处；
  应改为写日志文件或前端事件（当前 app 尚未使用守卫，影响面仅限真机测试）。
- 契约测试的**参数名**仍是手写表，源码解析只覆盖命令名。

---

## [0.1.1] — 2026-10-24

护栏补丁版：**不新增硬件功能**，只补齐"安全、可信、可回归"的地基。
全部改动可在无硬件条件下验证（mock 单测 + 静态检查 + 源码级契约解析）。

### 修复（安全）

- **真机测试在断言失败时不回滚硬件**（P0）—— 新增 `RestoreGuard`（RAII），
  panic 展开路径也会完成还原；`core/tests/hardware.rs` 的两个写测试全部改用守卫。
- **EC 访问未串行化**（P1）—— `AppState` 新增 EC 访问互斥，所有 IPC 命令
  （读写/撤销）都必须先取锁；前端在"忙"期间暂停 2 秒轮询，并给 `refresh`
  加在飞标记防止请求重叠。
- **Rust 侧无访问限速护栏**（P1）—— 新增 `AccessThrottle`，由
  `WindowsAcpiTransport` 在**唯一 IOCTL 出口**处调用：默认 20 ms（50 次/秒），
  硬下限 10 ms（100 次/秒），调用方无法绕过。
- **`undo_power` / `undo_mode` 失败时丢弃回滚记录**（P1）—— 改为只有真正
  撤销成功才出栈，失败时保留记录并提示"可重试"。
- **锁中毒导致 `get_status` panic**（P2）—— 改为返回结构化状态 + `restart`
  提示；前端新增"需要重启程序"分支。
- **撤销档位时误写自清零的命令寄存器**（P2）—— 提交寄存器 `0x0767` 移出快照集合。

### 修复（工程与文档）

- **证据链断裂**（P0）：新增 `docs/protocol/ec-baseline.md` —— 脱敏入库版协议基线
  （7 项交叉核对地址 + 传感器/档位编码 + 全量扫描统计 + 复现命令 + 未验证项清单）；
  原始 `reverse/evidence/*.json` 继续不入库。`README` / `ec-access.md` /
  `workflow_status.md` / `CONTRIBUTING.md` 的引用一并改正。
- **IPC 契约测试存在人工同步盲区**（P2）：前端新增 `IPC` 契约表，
  `ipc_contract.rs` 用 `include_str!` 解析 `app/ui/app.js` 源码，
  双向比对命令集，并断言"不存在直接传字符串命令名的 invoke 调用"。
- 修正 4 处文档过期：需求矩阵 R10/R11（实际已闭环）、`workflow_status.md`
  的"下一步"（B1 早已解决）、工具数 12→16、`ec_health.ps1`（不存在）。
- 发布清单新增"文档一致性自检"与合规自动检查命令。
- 删除 3 个死依赖（`serde_json`、`tracing`、`tempfile`）；删除两个空目录
  （`protocol/`、`test/`）；`graft/` 加入 `.gitignore`。

### 可访问性

- 新增 `:focus-visible` 焦点样式（此前键盘用户看不到焦点）；状态栏加
  `role="status" aria-live="polite"`；告警区加 `role="alert"`；
  支持 `prefers-reduced-motion`。

### 测试

- 核心库单测 **33 → 44**（新增：节流夹取/间隔/并发串行、回滚守卫
  drop/disarm/panic 路径、提交寄存器不入快照）。
- IPC 契约测试 **5 → 8**（新增：源码契约表双向比对、禁止裸字符串命令名、
  手写表与源码一致性）。
- 新增二进制单测 **4 个**（锁中毒提示、撤销栈容错、EC 访问串行化）。
- 实际执行：`cargo test` 44 passed + 1 doctest；`cargo clippy --all-targets`
  0 warning；`cargo fmt --check` 干净；`cargo test --bin mechrevo-max` 4 passed；
  `cargo test --test ipc_contract` 8 passed；`node --check app/ui/app.js` 通过。

### 已知限制（与 0.1.0 相同）

- 温度读数仍未实现（v0.2.0，需一次用户在场的只读探测）
- 仅在本机（旷世16 Pro GM6PQ0X）验证；跨机型地址不通用
- 真机集成测试与 UI 自动化 E2E 按安全策略不执行

---

## [0.1.0] — 2026-10-17

首个可用版本。核心链路已在真机完整验证。

### 新增

**核心库 `mechrevo-core`**
- EC 访问层
  - `EcTransport` trait 抽象，真机实现 `WindowsAcpiTransport`（IOCTL `0x9C40A488` / `0x9C40A48C`）
  - `MockTransport` 供离线测试，可注入「写了不生效」「读取失败」等故障
  - `Ec<T>`：写后读回校验、位操作、`RollbackPoint` 快照与还原
- 运行档位切换（`ModeController`）
  - 均衡 / 办公 / 狂暴三档，组合 `0x751` + `0x7AB`，提交寄存器 `0x0767`
  - 正确识别提交寄存器「写后自清零」特性，不做读回校验
- 功耗墙控制（`PowerController`）
  - PL1 / PL2 / PL4 读写，范围校验（PL1/PL2: 5–200W，PL4: 0–255W）
  - 写入前存回滚点，失败自动回滚已写入项
- 传感器读数（`Sensors`）
  - CPU / GPU 风扇转速、电源来源、项目 ID
- 寄存器地址表（`namespace`）
  - 每项标注可信度：`Verified` / `CommunityUnverified` / `NotApplicable`

**桌面应用（Tauri v2）**
- 运行档位一键切换，当前档位高亮 + 原始寄存器值显示
- 实时读数（CPU/GPU 风扇、供电状态），2 秒轮询
- 默认功耗墙滑块（游戏/办公分别配置）
- 一键撤销（档位切换与功耗改动均可回退）
- EC 不可用时给出可执行的修复指引
- 权限不足时提供「以管理员身份重启」按钮
- 启动自检：UI 元素 id 与 JS 映射不一致时立刻报告
- 滑块编辑锁：防止轮询覆盖用户正在拖动的值

**工具（Python，16 个）**
- EC 读取 / 扫描 / 验证 / 健康自检 / 档位实验 / 稳定性测试
- ACPI 表导出与检索、AML 解析（EC 操作区、设备定义）
- 驱动版本对比、官方注册表解析

**诊断脚本（PowerShell）**
- `hw_health.ps1` 硬件健康快照
- `diagnose_reboot.ps1` 重启原因分析
- 驱动枚举、运行态采集、UIA 树导出

**文档**
- `docs/SPEC.md` 规格基线
- `docs/ARCHITECTURE.md` 架构与扩展指引
- `docs/DEPLOYMENT.md` 构建/发布/回滚
- `docs/TROUBLESHOOTING.md` 排障
- `docs/CONTRIBUTING.md` 贡献指南
- `docs/VERIFICATION-LEDGER.md` 验证台账
- `docs/protocol/ec-access.md` 协议权威来源
- `docs/REQUIREMENTS-TRACEABILITY.md` 需求追踪矩阵

### 安全

- **EC 访问强制限速**（默认 ≤50 次/秒，硬上限 100 次/秒，代码内强制）
- 写入一律读回校验，不信任驱动返回值
- 所有写入可回滚
- 不联网、不收集任何用户数据
- 不打包、不分发任何官方二进制 / 驱动 / 反编译产物

### 已修复（相对官方控制台的缺陷）

| 官方缺陷 | 本项目的做法 |
|---|---|
| 内存占用 178 MB | 单进程，运行内存约 52 MB |
| 主程序 .NET Native AOT 无法审计 | 全 Rust 源码 + 33 单测 |
| MQTT 通信密码明文硬编码 | 无进程间通信，无凭据 |
| 通信失败被 `catch {}` 完全静默 | 失败一律报错并显示到界面 |
| 装完 EC 通道不可用（PnP 未扫描） | 文档显式说明修复步骤，界面给出指引 |
| 能力表数据自相矛盾 | 每项能力标注验证状态 |

### 已知限制

- 仅支持 Windows x64
- 需要管理员权限
- 需先安装官方控制台以获取 `UWACPIDriver.sys`（本项目不分发该驱动）
- 温度读数尚未实现
- 仅在本机（MECHREVO 旷世16 Pro GM6PQ0X，BIOS N.1.06MRO16）验证
- 跨机型地址表不通用，新机型需实机重新验证

### 重要事件记录

2026-10-17 02:11 本机发生一次「无转储硬复位」（Event 41 Kernel-Power）。
开发过程中的 CPU 满载压力测试与 EC 泛洪（约 620 次/秒）都可能是触发器。
**已据此实施 EC 访问限速，并禁止一切硬件压力测试。**
同模式事件在 2026-10-13 与 2026-10-16 也曾出现，均早于本次工作，
说明是机器既有的偶发问题。详见 `workflow_status.md`。
