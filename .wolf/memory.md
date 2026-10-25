2026-10-25 | 建立 OpenWolf 兼容记录层；权威任务仍以 project_specs.md 为准。
2026-10-25 | 定向修复：外部查询超时、未知值、电池存在性、EC 锁外采样、曲线失败退避。
2026-10-25 | 独立 reviewer 指出电池温度单位错误；已按十分之一开尔文修复并新增 fixture，等待复验。
2026-10-25 | 独立 Chromium + mock IPC 验证 21 条；真机仅低频只读健康检查，无写入。
2026-10-25 | 历史单轮审计已复制校验后归档，manifest 可回滚；其它未闭环文档保留。
2026-10-25 | v0.5.1 已发布：CI 三 job success，Release 三资产已 API 复核；独立审查 Approve。
2026-10-25 | v0.5.2 可靠性复审完成：EC 部分失败可见、配置原子保存/恢复/互斥、Job 进程组超时、错误代次；独立 reviewer Approve。
2026-10-25 | 真实 cmd+ping 用例确认继承 pipe 的后代会使直接 kill 不够；最终显式 TerminateJobObject，真实 4/4 通过。
2026-10-25 | v0.5.2 发布完成：CI 三 job 全绿，Release 三资产已 API 复核；独立 reviewer Approve。
