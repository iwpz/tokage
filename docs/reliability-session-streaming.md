# m2g-r1：运行中任务的会话消息与流式输出

日期：2026-10-19。m3a 暂不开始，先修复 m2g 实机中发现的会话查看缺口。

## 问题与修复

此前任务派发只保存了 attempt 身份，任务提示和 CLI 响应没有进入该 attempt 的 chat_logs；文本仅累积在任务执行器的内存中。会话页只在选择变化时读取一次数据库，即使有新消息也不会更新。因此左侧存在 RUNNING 对话，右侧却没有发送内容和响应。

现在任务派发保留实际发送的任务提示，并创建各自 turn 的流式响应。文本、思考、工具调用及结果复用主对话的事件归并规则，通过独立的任务消息 store 立即驱动右侧渲染；SQLite 快照每 250ms 合并保存，最终状态立即排队保存，按 turn 串行写入，旧快照不能覆盖新结果。保存失败保留内存消息并显示提示，可通过刷新重试。

会话页每秒串行刷新 attempt 状态和持久化记录，运行中的本地事件快照优先于较旧的数据库内容。切换 session、project、scope 或 attempt 后，旧请求的结果不能覆盖新选择；读取失败保留已有消息并提示。尚无文本时明确显示等待 CLI 输出，思考与工具过程可展开查看。

数据库写入使用显式 attemptId，并核对该 task attempt 属于同一主会话、provider 与 model；拒绝把消息移动到其他 attempt。任务消息不会写回主会话的续聊历史，不能串用为下一次用户输入。保存转录也不改写任务的执行 / gate / 交付状态，防止较迟的 streaming 保存把 DONE 改回 RUNNING。

取消和中断保留已有输出，关闭该 turn 的转录写入；迟到成功 / 文本不能复活它。后续继续、修复、审批恢复的新 turn 使用独立记录。消息中的“响应结束”只表示 CLI 结束，不代表任务已通过 gate 或已经交付。

本次真实额度错误还暴露了旧错误入口没有保存 attempt 终态的问题，左侧可能一直显示 RUNNING。现在错误 / 超时在暴露 BLOCKED 前保存 FAILED / TIMEOUT，再次检查取消与 turn 所有权；无终止事件的异常退出同样落盘。新增回归验证取消发生在保存等待期间时，迟到回调不能把 CANCELLED 改回 BLOCKED。

## 验证

- 前端回归覆盖并行 turn 隔离、连续文本与思考、工具活动、取消后的部分输出与迟到事件、慢保存与最终写入顺序、失败重试，以及停止旧读取 / 串行轮询。
- SQLite 回归覆盖并行任务消息的准确归属、重复保存幂等、跨会话 / 跨 attempt 拒绝、主对话恢复隔离，以及不改变既有执行终态。
- 桌面验收入口增加 `--verify-session-streaming`：真实自然语言规划产生两个并行 RUNNING 任务，依次在实际会话树中选择它们，要求右侧用户提示可见，响应 DOM 在任务仍 RUNNING 时发生变化；随后独立核对每个 task attempt 的提示 / 响应记录，再完成原有门禁与交付。

真实桌面命令（固定原定模型，不回退）：

```sh
python3 scripts/run-live-desktop.py CLAUDE_CODE --scenario natural \
  --version-prefix 2.1.296 --cli-path /absolute/path/to/claude-2.1.296 \
  --verify-session-streaming --timeout 1500
```

全量检查首轮确实失败：旧 FakeAdapter 宣称从 stdin 接收提示，却可能在读取前退出，协议回放偶发 Broken pipe。现在替身先完整消费输入，再回放原样协议；保留生产对输入未送达的拒绝与原有断言，不通过忽略该测试取得通过。

首轮实机生成了 `npm run test`，被只允许 `npm test` 的验收驱动拒绝，尚未开始任务。驱动与独立 oracle 仅补充这一个等价 package script 写法；原始生成计划不改写，附加参数、任意命令和缺失全量验收仍拒绝。失败现场与重跑分别归档。

最终检查结果见 [检查记录](reliability-evidence/session-streaming/validation.md)，两项真实流式检查见 [实机证据](reliability-evidence/session-streaming/desktop/streaming-checks.json)。

## 边界

历史任务此前没有保存的提示和响应不会凭空补造；会话页会说明没有已保存记录。此次修复针对新的任务运行，以及修复后已经保存的部分输出。流式显示受 CLI 实际事件影响：若 CLI 只有思考 / 工具活动，界面显示过程，不伪造模型回复；若 CLI 本身缓冲文本，Tokage 也不能提前生成文本。

突然强杀仍可能丢失尚未提交的转录快照；此处不是费用回执或请求 exactly-once 保证。m2 的原始历史证据保持其原源码指纹，不因本次修复而重写；m3a 的真实代码库选择与连续需求试用尚未开始。

## 本轮实机结论

Haiku 的两个真实并行 RUNNING 任务均通过实际会话树选择与右侧 DOM 更新检查；各自提示和部分响应准确落盘，后续额度错误 / 取消的文本保留。随后真实 CLI 返回 session limit，应用将失败任务阻塞、取消另一任务并跳过下游，没有产生交付。整个桌面验收退出 1，正常交付复验未通过；不自动重试、切换模型或以流式通过冒充交付通过。

实机构建指纹保存在 `desktop-build-source.json`。之后补充的 attempt 终态保存由本机生产编排回归与 SQLite 状态保护回归验证，没有在已耗尽额度的账户上再次发送请求；归档中原有 RUNNING 状态不手工改成 FAILED。最终源码与全量检查另由 `environment.json` 和 `local/summary.json` 记录。

m3a 未开始。完整正常交付复验保留为未勾选项，待额度恢复后继续。
