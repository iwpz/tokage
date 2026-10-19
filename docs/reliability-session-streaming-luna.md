# m2g-r1：GPT6 Luna 复验

日期：2026-10-19。按用户要求，后续测试从 Haiku 改为 Codex CLI 0.162.0 / `gpt-6-luna`。本机登录探测为 `Logged in using ChatGPT`；不切换模型、不改用户账户或全局配置。本次扩充调试验收入口，不修改历史 m2 范围认证。

## 桌面结果

- [x] 固定模型自然语言规划、实际计划审阅与并行任务派发。
- [x] 两个 RUNNING task 在实际会话页分别可见发送内容、响应过程持续更新，转录保存无错误。
- [ ] 完整正常交付：失败。没有 verified.json，没有合并交付，不能以流式通过替代交付通过。

命令：

```sh
python3 scripts/run-live-desktop.py CODEX_CLI --scenario natural \
  --version-prefix 0.162.0 \
  --cli-path /Users/iwpz/.nvm/versions/node/v22.16.0/bin/codex \
  --verify-session-streaming --timeout 1500
```

Codex 适配器只在已有 session command 授权时启用 workspace-write；其 OWN_SANDBOX 能力未纳入 Tokage 的 OS_SANDBOX Full Auto 资格。会话的 Full Auto 授权不会使其自动取得写权限。实际格式化任务尝试写文件，系统返回 operation not permitted，进入待审批阻塞；另一个任务未形成有效改动，真实门禁失败并进入修复，验收驱动随后取消。没有通过禁用沙箱、增加伪造的已批准命令或扩大权限来使本轮通过。

这说明当前组合的桌面自主交付存在权限适配缺口，不能沿用 Claude / OpenCode 的既有通过结论。Codex 的事件接口按自身事件粒度更新，未声称提供逐 token 输出。

## 新发现与修复

实机中待审批任务的 CLI 已结束，但 attempt 仍为 RUNNING。原先错误 / 超时入口的终态修复没有覆盖「pending approvals + done」和「pending approvals + chat-ended」两个入口。

现在先将结束且没有完成任务的该次 attempt 保存为 FAILED，并保留 pipeline 的 BLOCKED 和待审批记录；后续审批恢复仍是独立 turn。保存后再次核对取消状态与 turn 归属，避免迟到保存把 CANCELLED 改回 BLOCKED。新增两种终止方式的保存断言与取消竞态回归。归档 SQLite 保留修复前的真实 RUNNING，不手改历史记录；这个补充修复只经本地回归，没有再次发送已知会被只读权限阻断的完整桌面请求。

## 证据

[复验目录](reliability-evidence/session-streaming-luna/) 保留配置、规划与转录、状态事件、SQLite 一致性备份、环境、源码和二进制指纹。`facts.json` 区分实机构建和其后的终态修复。原 Haiku 额度失败证据保持不变。

该小型 invoice 仓库只用于 m2g-r1 回归，不计入 m3a 的真实 Tokage 需求样本。

## Luna 底层角色链

首轮真实角色链失败：规划提示只要求记住 marker，而测试要求回复包含 marker，模型回复未包含该值。失败日志保留。将提示明确为“回复包含读到的 marker”，不改变断言、不注入期望输出，再次运行。

重跑通过：只读规划读取文件 → 执行编辑并产生真实 Python 断言输出 → 同一原生会话续聊 → 只读审查 → 原生会话可读。执行轮使用该测试已有的显式 `session_allowed=["python3"]`，不是桌面自然语言计划自动获得的授权，因此不能替代完整桌面交付验证。

最终本地统一检查见 `local/summary.json`，真实角色链日志见 `role-chain-retry.log`。这些结果分别记账，失败桌面链路保持未完成。
