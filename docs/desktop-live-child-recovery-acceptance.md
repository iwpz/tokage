# 直聊存活子进程恢复验收（dr3）

2026-11-13，针对 dr2 未捕获的存活子进程窗口，使用 `external-ui-v1` 验证直聊恢复。测试使用独立 fixture 和提交 `aafcf9ed48539fdc7d1549945567e7a439d28096` 的隔离桌面构建，普通 UI 请求 `CODEX_CLI / gpt-6-luna / medium` 执行只读 `sleep 90`；CLI 版本 0.161.0。模型实际身份的独立 observedModel 和美元费用均未报告，保持未知。

界面显示命令运行中后，核对隔离应用可执行文件和 PID，仅对其父进程发送 SIGKILL。登记的 Codex 子进程 PID 51841 在强杀后仍存活。重新启动应用并通过正常 UI 点击核对时，界面显示 `Native operation process still active`，保留未解决状态。子进程自然退出后确认其 PID 和进程组均不存在，再次核对成功，界面显示中断已确认并提示检查工作区。

同一会话始终只有一个 turn，最终状态为 unknown，未自动重发。fixture 的 HEAD 为 `fd10b679e9bf5e87705fb56b8b58aa73c407e919` 且工作区干净。记录和各阶段 Accessibility 快照保存在 `/Users/iwpz/Documents/tokage-validation/recovery-luna-aafcf9e-e70qsr2j/live-child-recovery/`，其中 `sha256.json` 可核对证据。

本次验证的是直聊中登记的原生子进程，并不证明编排任务、gate 或所有后代进程的同一情形。构建使用标准 Tauri 独立数据目录与 incognito WebView，CLI HOME 未隔离；这不是原发布二进制或前端设置持久化验收。没有更改产品源码、直接修改存储或自动接受确认；连续真实需求试用保持 9/20。
