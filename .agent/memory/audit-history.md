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
