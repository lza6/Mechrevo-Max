2026-10-25 | 建立 OpenWolf 兼容记录层；权威任务仍以 project_specs.md 为准。
2026-10-25 | 定向修复：外部查询超时、未知值、电池存在性、EC 锁外采样、曲线失败退避。
2026-10-25 | 独立 reviewer 指出电池温度单位错误；已按十分之一开尔文修复并新增 fixture，等待复验。
2026-10-25 | 独立 Chromium + mock IPC 验证 21 条；真机仅低频只读健康检查，无写入。
2026-10-25 | 历史单轮审计已复制校验后归档，manifest 可回滚；其它未闭环文档保留。
2026-10-25 | v0.5.1 已发布：CI 三 job success，Release 三资产已 API 复核；独立审查 Approve。
2026-10-25 | v0.5.2 可靠性复审完成：EC 部分失败可见、配置原子保存/恢复/互斥、Job 进程组超时、错误代次；独立 reviewer Approve。
2026-10-25 | 真实 cmd+ping 用例确认继承 pipe 的后代会使直接 kill 不够；最终显式 TerminateJobObject，真实 4/4 通过。
2026-10-25 | v0.5.2 发布完成：CI 三 job 全绿，Release 三资产已 API 复核；独立 reviewer Approve。
| 21:30 | Q-06/Q-07 拍板入档；T-040-04 评审方案 §19 编制完成（三方案对比+TDD F-1~F-10+验收 V-1~V-6）；VERIFICATION-LEDGER E2E 标注纠正 | decisions.md/project_specs/指南§19/台账 | 完成 | ~15k |
| 22:40 | T-040-04 离线阶段：V-1 重扫0 diff + core 写入会话(write_curve/validate/RPM监视) + 13测试全绿 + 提交 83bf78a | fancurve.rs/transport.rs/evidence | 完成 | ~35k |
| 23:59 | T-040-04 完整闭环：真机 V-1/V-1b/V-2b/F-10 全过；审查3轮 Approve；v0.6.0 推送 tag+CI run 40332841600 | fancurve/main.rs/app.js/evidence | 完成 | ~60k |
| 00:40 | 终局审计：3 处文档不一致修复+ADR 0002+GUI E2E+文档清理归档+社区调研 | a4585e3/68fc828 | 完成 | ~25k |
