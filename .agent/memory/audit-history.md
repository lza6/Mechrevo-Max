# 审计历史

## 2026-10-24 · v0.1.2 独立复审与修复
- reviewer 子代理独立跑验证并给出 CONDITIONAL PASS：R1（限速锁未覆盖 IOCTL）、R2（闸门实例级）、R3（台账超范围断言）、R4（注释矛盾）
- 全部修复并补回归测试（`run_never_allows_overlapping_access` 断言闸门内峰值并发=1）
- 复审未审查范围：`docs/计划书/*`、`.agent/*`、`tasks/*`、HTML 报告、`tools/publish_release.py`

## 2026-10-24 · 全量只读复审 + v0.1.1 护栏补丁
- 范围：全部跟踪文件（65→69），排除 `target/`、`graft/`、`reverse/evidence/*.json`
- 方法：结构扫描 → 只读验证 → 文档/代码交叉核对 → 4 方案权衡 → 原子任务 → 修复 → 独立审查
- 产出 27 条问题（P0×2、P1×5、P2×10、P3×10），全部带 `file:line`
- 关闭：P0-01、P0-02、P1-01、P1-02、P1-03、P1-05、P2-01、P2-04、P2-05、P2-09
- 未关闭：P1-04（温度，v0.2.0）、P2-02/03/06/07/08/10、P3 全组
- 报告：`docs/计划书/下一步改进指南.md`、`reports/change-report-2026-10-24.html`

## 已审计模块（勿重复审计，除非改动）
- `core/`：ec（transport/windows/throttle/mod）、control、mode、sensors、namespace、error、lib
- `app/`：main.rs（IPC 层）、ipc_contract.rs、ui（index/style/app.js）
- `docs/`：SPEC / ARCHITECTURE / DEPLOYMENT / TROUBLESHOOTING / CONTRIBUTING / 台账 / 需求矩阵 / protocol
- `tools/`：ec_health / ec_scan / ec_stress / publish_release（限速与发布逻辑）

## 2026-10-26 终局审计（v0.6.0 后）
- 范围：全文档一致性、README/SPEC/baseline/DEPLOYMENT、ADR、真实 GUI E2E、计划文档清理、技能沉淀、社区调研
- 发现并修复（P1/P2）：README「不会开放写入」过期（v0.6.0 已上线）；ec-baseline §6 曲线表条目未更新行为级证据；DEPLOYMENT「不写持久化配置」过期（profiles.json 自 v0.3.0 存在）——全部已修，提交 a4585e3
- 新增：docs/adr/0002-fan-curve-write-safety-chain.md；code-review-checklist 补「断言句柄/伪 panic 测试」教训
- 真实 GUI E2E：v0.6.0 exe 启动成功（8 仪表实时、50.2MB）截图 docs/screenshots/e2e-v060-launch.png
- 计划文档清理：18 候选 → 仅归档 tasks/riper5_20261025_224633.md（已闭环会话记录，manifest 68fc828）；其余为活跃路线图/规范保留
- 社区调研（explorer）：z0brk/Mechrevo-Fan-Control（Go，机械革命 14/16）用**不同 EC 地址方案**（Offset 0x91 间接寻址），与本机直连 0x0F00 区不同——确认跨机型 EC 布局差异真实存在，佐证多机型 blocked 于第二台机器的决策；tongfang-fan-controller 为同方系（PF5NU1G），非本机 EC 方案。无可直接采纳的代码，backlog：无新增高价值项
- 结论：无 P0/P1 残留，本轮收敛

## 2026-10-27 复核审计（本轮）
- 性质：v0.6.0 后复核，非全量重复审计（2026-10-26 终局审计刚收敛，无 P0/P1 残留）
- 真实运行验证全部通过：core lib 88 单测 / app 44 / IPC 契约 8 / doctest 1 / 前端 qa_browser 33 断言 0 失败 / 两 crate clippy 0 警告 / 无冲突标记残留
- doctest 首跑失败为环境问题（choco rust 的 rustdoc.exe 缺失），按台账用 RUSTDOC 指向 rustup 1.95.0 工具链后通过——该坑已在 tested-scope 有记录，本次复确认仍有效
- 收尾：上轮新增的《前端UI增强计划》文档提交推送 a8c766c；CI run 待推送后核对
- 结论：本轮无新缺陷；仍开放项不变（P2-07、P2-10、P3-09、P3-13、S4/S6 真机 pending、多机型 blocked）

## 2026-10-27 · 第二轮：计划文档闭环清理 + 决策拍板 + v0.6.1
- 抽验：core 88 单测 / 前端 33 断言全绿，工作区无漂移（qa_browser 截图副产物已还原）
- 用户拍板 Q-04（不做自动更新）/ Q-05（只要托盘，纳入 v0.7.0）
- 《下一步改进指南.md》全部任务闭环 → 按 §10.4 流程归档删除（docs/archive/plans/20261027/ + manifest）
- 悬空引用修复：project_specs.md、REQUIREMENTS-TRACEABILITY.md、ADR-0002
