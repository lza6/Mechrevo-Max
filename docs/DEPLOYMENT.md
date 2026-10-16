# 构建、发布与回滚

> 本文档描述如何从源码构建产物、发布到 GitHub Releases、以及出问题时如何回滚。

---

## 1. 环境要求

| 项 | 版本 | 说明 |
|---|---|---|
| Windows | 10 / 11 x64 | 仅支持 Windows（EC 访问依赖 Windows 内核驱动） |
| Rust | 1.80+ | 本项目在 1.95 上验证 |
| Node.js | 18+ | 仅打包 GUI 时需要 |
| Windows SDK | 10.0.22621+ | **必须**，见下方「已知构建坑」 |
| 管理员权限 | 需要 | 运行期访问 `\\.\ACPIDriver` |

### 运行期前置依赖（最终用户机器）

| 依赖 | 来源 | 说明 |
|---|---|---|
| `UWACPIDriver.sys` | 随机械革命官方控制台安装 | 本应用**不打包**该驱动 |
| EC 设备已枚举 | 需手动触发一次 | 见下方「设备就绪」 |
| WebView2 Runtime | Windows 10/11 通常已内置 | 若无则需安装 |

### 设备就绪（**官方安装流程漏掉的一步**）

```powershell
sc.exe start UWACPIDriver
pnputil.exe /scan-devices      # ← 关键：触发 PnP 扫描，否则驱动无设备可绑
```

---

## 2. ⚠️ 已知构建坑（务必先读）

### 2.1 `rc.exe` 版本

`embed-resource` 会通过 Windows SDK 注册表查找 `rc.exe`，**可能选中很旧的版本**
（实测选中 10.0.10011），在含中文的路径下会报：

```
RC.EXE failed to compile specified resource file
```

**解决**：构建前把较新的 SDK 加入 PATH：

```bash
export PATH="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64:$PATH"
```

系统上可用的新版：`10.0.22621.0`、`10.0.26100.0`。

### 2.2 必须存在 `icons/icon.ico`

缺少时 tauri-build 报 `` icons/icon.ico` not found; required for generating a Windows Resource file ``。

### 2.3 前端资源不会自动重嵌入

`tauri::generate_context!` 在**编译期**读取 `frontendDist` 目录。只改 `app/ui/*` 时
cargo 不会重编译 `main.rs`，跑的还是旧资源。

**解决**：改完 UI 后 `touch src/main.rs && cargo build`。

### 2.4 DPI 感知

缺少 DPI 感知声明时，进程会被 Windows 判定为 DPI 不感知，在 150% 缩放下窗口被整体放大 1.5 倍，
物理尺寸超出屏幕、右侧内容被裁且渲染模糊。

本项目在 `src/main.rs` 启动时通过 `SetProcessDpiAwarenessContext` 声明，不依赖构建期清单。

---

## 3. 构建

```bash
# 1) 核心库测试（离线，无需硬件）—— 每次改动必跑
cd core
cargo test
cargo clippy --all-targets      # 要求 0 warning
cargo fmt --check

# 2) 桌面应用
cd ../app/src-tauri
export PATH="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64:$PATH"
cargo test --test ipc_contract   # 前后端契约（编译期校验）
cargo build --release
```

**产物**：`app/src-tauri/target/release/mechrevo-max.exe`

单文件绿色版，约 4.8 MB，无需安装。

### 打包安装包（可选）

```bash
cargo tauri build
# 产物: target/release/bundle/msi/*.msi, bundle/nsis/*.exe
```

---

## 4. 版本与发布

### 4.1 版本号规则

语义化版本 `MAJOR.MINOR.PATCH`，与 `app/src-tauri/Cargo.toml` 的 `version` 一致。

| 变更类型 | 版本位 |
|---|---|
| 不兼容的协议/API 变化 | MAJOR |
| 新增功能（向后兼容） | MINOR |
| Bug 修复 | PATCH |

**发布前必须同步三处版本号**：
1. `app/src-tauri/Cargo.toml` → `version`
2. `app/src-tauri/tauri.conf.json` → `version`
3. `core/Cargo.toml` → `version`（若核心库有变化）

### 4.2 发布流程

```bash
# 1) 确认工作区干净且测试全绿
cd core && cargo test && cd ..
cd app/src-tauri && cargo test --test ipc_contract && cd ../..

# 2) 打 tag（tag 名 = v + 版本号）
git tag -a v0.1.0 -m "v0.1.0: 首个可用版本"

# 3) 推送（用户明确要求时才执行）
git push origin main
git push origin v0.1.0
```

### 4.3 GitHub Release

```bash
gh release create v0.1.0 \
  --title "v0.1.0 — 首个可用版本" \
  --notes-file CHANGELOG.md \
  app/src-tauri/target/release/mechrevo-max.exe
```

---

## 5. 回滚

### 5.1 版本回滚

```bash
# 让用户下载上一个 tag 的 exe 覆盖即可（无安装步骤，无注册表残留）
gh release download v0.1.0 --pattern "mechrevo-max.exe"
```

本应用是绿色单文件，**回滚不涉及数据迁移**（仅 `appsettings`-类配置由用户自行管理）。

### 5.2 硬件状态回滚（重要）

若用户应用了不当的功耗/档位设置，有两条回滚路径：

| 方式 | 操作 |
|---|---|
| 应用内 | 界面「撤销上次」按钮（保留最近一次改动的快照） |
| 命令行 | `python -X utf8 -I tools/ec_health.py` 查看当前值，再用 `ec_mode_exp.py` 恢复 |
| 彻底重置 | 重启机器后 EC 会回到默认档位；或运行机械革命官方控制台恢复默认 |

**注意**：本应用不写入任何持久化配置到磁盘，所有改动都在 EC 寄存器里，
重启后由 BIOS/EC 固件决定初值。

---

## 6. ⚠️ 绝对禁止的操作

以下操作在开发过程中曾导致本机发生「无转储硬复位」（Windows Event 41 Kernel-Power）：

| 禁止 | 原因 |
|---|---|
| **EC 泛洪**（>100 次/秒） | EC 是 8051 级单片机，同时管风扇与温控；泛洪会令其停摆 → 风扇失控 → 硬复位 |
| **CPU/GPU 满载压力测试** | 笔记本全核满载逼近温度墙，同样可能触发硬复位 |

详见 `docs/SPEC.md` §5.1。代码中已通过 `MIN_INTERVAL_S` 强制限速。

---

## 7. 发布检查清单

- [ ] `core`: `cargo test` 全过
- [ ] `core`: `cargo clippy --all-targets` 0 warning
- [ ] `core`: `cargo fmt --check` 干净
- [ ] `app`: `cargo test --test ipc_contract` 全过
- [ ] `app`: `cargo build --release` 成功
- [ ] 三处版本号已同步
- [ ] `CHANGELOG.md` 已更新
- [ ] `README.md` 的功能清单与真实状态一致
- [ ] `docs/protocol/ec-access.md` 与真实协议一致
- [ ] **未打包任何官方二进制/驱动/反编译产物**（合规红线）
- [ ] 低频只读自检通过：`python -X utf8 -I tools/ec_health.py`
