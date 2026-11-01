# T06 · 任务窗共用上下文规划与执行

日期：2026-11-01。本轮为实现及离线协议验证，不是新一轮真实模型、桌面 UI 或多平台验收。

## 入口和范围

保留跨任务 `execution::choose_context`，不重建已完成任务的复用选择。新增 `task_context` 接入同任务的修复续聊和公开 `AppendTaskMessage` 命令：任务必须未完成、处于 Blocked、原生/Git/gate 副作用已经清理、原计划授权有效，source 的任务/角色/模型分配必须匹配。活跃 worker、审批中、DONE、改派后旧 source、过期授权、空白/超限消息明确拒绝；不抢占正在生成的模型。边界是 Tokage 自有轮次之间，不干预 CLI 内部工具/API 循环。Composer 定向入口及动作可见历史留 T07/T08。

App 与 CLI 使用同一 schema-v1 runtime；CLI 可经现有 `run submit --file REQUEST.json` 提交正常产品命令（会等待 worker，不是只读查询）。命令文件结构：

```json
{
  "schemaVersion": 1,
  "commandId": "new-unique-id",
  "runId": "RUN_ID",
  "expectedRevision": 123,
  "command": {
    "type": "AppendTaskMessage",
    "payload": {
      "task_id": "T1",
      "confirmation_digest": "CURRENT_APPROVED_PLAN_DIGEST",
      "message": "保留确切的追加文本和空白"
    }
  }
}
```

先通过 `run show` 取得当前 revision / approvedDigest，命令沿用被冻结范围、命令确认、权限、预算、任务 gate 和最终审查，不批准新计划或自动交付。失败后先查询已记录结果；相同 commandId 和完全相同输入幂等，改变输入需新命令。取消未消费追加消息会清空 pending；已绑定后取消保留原轮证据，不自动重放。

## 共用规划及输入来源

主会话和任务窗共用 `tokage_context::inputs::resolve_inputs_with_history`、`plan_context_action` 以及同一 draft 校验内核。主会话 wrapper 继续限定普通主会话 scope；任务 wrapper 必须带 executor / taskId / revision，跨任务或旧 scope 不可复用载荷，调用方不能替换原生命令或 prefix。

任务输入来自当前冻结知识包、当前任务目标与内嵌 contracts、范围、未解决错误、已有报告和 gate 证据。上下文优先使用 CLI contextTokens；缺失时使用已保存 prompt 字节与报告字节 / 4 的可见内容估计及共享 provider prior，不把累计输入 token 当成上下文测量。新轮保存可见 prompt 字节及 window origin，旧缺失或截断历史标为不完整，不伪造 cooldown 已结束。`TaskRun.context` 保留原有跨任务候选选择；实际同任务承载以 `turn.contextPreparation`、`windowOrigin` 与观测 thread 为准，候选不等于已实际复用，完整成本/身份对齐留 T10。

cacheAware=true、minSaving=0.15 是本轮与共享默认值一致的明确 fallback；其余来源和预测保留在 preview.sources / params。全量用户参数冻结、跨任务复用的原有默认输入及校准仍属 T09/T12，不在此宣称完成。价格与差额仍是估计，不等于账单或实测节省。

## 准备、绑定和发送

1. 队列接收前及 reservation 事务内只读预检；硬阈值/cooldown 冲突不产生模型调用或部分 reservation。消息尚未发送时保留 pending；重复追加、错误任务和授权变化不能消费它。
2. 工作区连续性检查后，重建当前材料，持久保存 PREPARED：source / target turn、source revision/digest、configHash、planDigest、policy、preview、prefix 和是否要求新窗。追加文字另存并逐字放在保留 prefix 之后。
3. 发送前再次验证活跃授权、取消状态、来源 digest 和目标 attempt，然后与 native intent 在同一事务中记录 BOUND、绑定时间及 window origin，并清除 pending。新窗不携带旧窗的 scoped-command grants。取消发生在绑定之前不消费动作；绑定后可能已发送的轮次保留证据，不推断免费或未执行。
4. 审批续传不重新准备或再次绑定动作；source 的原生上下文已经承载过 prefix / 用户消息，不通过批准命令重发它们。
5. SYNTHESIZED_FRESH_BRIEF 明确取消 resume、保留同一任务工作树与原始验收基线，并要求返回的原生 thread 与 source 不同。没有 thread 或返回旧 thread 时拒绝后续接受，动作记 FAILED，不把请求新窗当成实际切窗成功。

cooldown 使用同窗已完成且确认清理的轮次与持久绑定锚点；失败不推进计数、preview 不推进计数，新窗承载轮本身不充当后续两轮。秒级时间相同的情况采用共享 replay 的保守规则。硬阈值不会绕过 cooldown；suggest 只保留建议、不自动执行，达到硬阈值时拒绝继续发送。

`off` 不规划或执行上下文动作，保留原有正常修复/续聊语义。COMPACT 可能按 provider 路由准备同窗指令或新窗 brief；指令承载完成不证明实际缩减。任务窗尚无原生 compact overhead 的独立费用归因，本轮不启用原生 `/compact`：候选能力、未核验安装版本、未知能力都不能变成 native confirmation。同窗指令不能安全跨硬阈值时明确阻塞。真实原生路径继续由 V01/V02 验证。

## 验证及后续

离线验证覆盖共享 planner 同输入一致、scope/retention 校验、目标/合约/错误保留、过期 config 绑定拒绝、cooldown/硬阈值、未知历史、off、suggest、审批续传不消费、追加消息取消、公开 runtime 和 CLI 提交幂等、真实子进程协议参数与 thread 变化、保留原工作树及旧原仓库不变。

协议 fixture 验证了：首轮失败后同线程追加；第二轮报告长上下文后准备新窗并发送保留 brief；明确移除 resume 且观察到新 thread；故意返回旧 thread 时拒绝；off 模式没有上下文动作。全部 fixture 在临时数据与工作树中，不改变生产判断、不发真实模型请求、不访问账号数据。

验证：context + runtime 全量 122 项通过，最终受影响路径另 18 项通过；CLI 原有全量 21 项及新增公开 submit-followup 场景通过，最终 task-context / CLI 场景再次通过；前端全量 625 项、App Rust 编译、TypeScript、i18n 和隔离检查通过。

实际命令、失败记录、源码哈希见 [验证清单](reliability-evidence/t06/verification.json)。T07 的建议/撤销/可见历史、T08 的 Composer 选择目标、T09–T12 的完整参数/身份/审计校准及真实模型/桌面场景保持待办；9/20 不推进。
