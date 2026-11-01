# T07 · 任务窗建议、队列与历史

日期：2026-11-01。完成产品实现与离线回归；未运行真实模型或 external-ui-v1 桌面验收，9/20 不推进。

## 产品入口

Pipeline 的任务卡片显示该窗最近动作状态，展开任务详情可查看当前窗、当前建议、规划依据和动作历史。聊天中的紧凑任务看板也可展开查看；修改动作从 Pipeline 详情进行。建议与历史复用 `ContextDecisionDetails` 的成本、参数来源和 cooldown 语义，沿用 T06 的 `Prepared` 记录与 runtime 事务，没有前端调度器或第二套任务状态机。

预览调用只读 `runtime_task_context`，使用当前任务最后一次 attempt 和 T06 的同一 resolver，不调用模型，不变更 cooldown、队列或授权。响应绑定 run/task/source/config/approval/revision；切任务、切窗、配置变化后旧异步响应不能重新显示或启用按钮。读取错误保留正常恢复入口，不自动重发。

## 排队与撤销

- suggest 可显式接受当前 COMPACT/FRESH 建议；auto 的执行前准备也显示在历史中。off 不提供动作。
- `QueueTaskContext` 在当前任务的 `contextActions` 中保存一条 PREPARED，复用 T06 的来源、摘要、preview、prefix、新窗要求和绑定字段。它只接受这一次动作，标记 `userAccepted`；运行的 suggest/auto 策略、冻结计划和模型分配不变。排队本身不派发模型，随后通过正常任务重试或追加消息发送。
- 服务端要求 Run/Task 均 Blocked、原生与 Git/gate 副作用已清理、无活跃 owner worker、无待定审批/追加消息、任务未完成且未偏离合同、最后一次 source 与分配匹配、授权摘要和 configHash 精确一致。UI 提交的 recommendedAction 必须仍与 resolver 相同，不能以旧预览接受另一动作。
- `CancelTaskContext` 只撤销指定 task/source 的未绑定动作，保留 CANCELLED 历史。下一次同 source/config/approval 的自动动作被抑制；硬阈值仍会阻止不安全续聊。重新接受建议可以重新排队。不同配置、授权或窗口不继承旧撤销。
- 已运行的自动准备使用正常“停止运行”入口；它可能与绑定竞争。服务端串行记录取消和绑定，先取消则不绑定，先绑定则保留 BOUND 与原生进程证据，不承诺撤回已发送请求。
- 计划修订保留仍在计划内任务的历史，并撤销尚未绑定的接受；停止运行也撤销未绑定动作。重新检查时推荐动作、source digest、配置、授权或新窗要求变化均拒绝消费旧动作。失去来源工作树/线程也明确拒绝，不能把未执行的队列记成 BOUND。
- 已接受动作在同一发送事务中绑定真实 target turn。绑定前运行失败记录 FAILED 与原因，不让上次显式接受继续存活。重复 commandId 不重新排队、撤销或发送。

公开 schema-v1 命令仍可通过 CLI `run submit --file` 使用，外层字段与 T06 相同，命令 payload 为：

```json
{
  "type": "QueueTaskContext",
  "payload": {
    "task_id": "T1",
    "source_turn": "CURRENT_LAST_TASK_TURN",
    "confirmation_digest": "CURRENT_APPROVED_DIGEST",
    "config_hash": "CURRENT_CONFIG_HASH",
    "recommended_action": "FRESH"
  }
}
```

撤销使用 `{"type":"CancelTaskContext","payload":{"task_id":"T1","source_turn":"CURRENT_LAST_TASK_TURN"}}`。先读当前 run/预览，旧命令失败后先读取结果；不要通过修改数据库制造动作。CLI 编译兼容已检查；本轮新增队列的运行回归走公开 runtime service 命令，不宣称新增 CLI 子命令或真实模型通过。

## 历史与验证边界

历史只合并同任务、同模型身份、同 windowOrigin/thread 的动作，并显示进入该新窗的 FRESH 动作。其他任务、其他窗及旧缺失 origin 的记录不会按相同 thread 字符串混合。过期的未绑定队列保留可撤销入口；任务队列与承载轮的同一记录去重。来源/目标 turn、策略、规划依据、承载方式与失败原因均可追溯。

状态分别表示：PREPARED 排队/准备；BOUND 绑定发送；CANCELLED 撤销；FAILED 动作失败。承载轮是否完成、失败或停止另列，要求 cleanupConfirmed 才显示完成。`nativeConfirmed` 单独显示；目前产品仍不启用任务窗原生 compact，因此真实记录保持“未确认”。COMPACT 按 provider 可能使用新窗保留摘要或同窗保留指令，界面明确显示承载方式。任何承载轮完成均不证明实际上下文缩减、原生 compact 完成或实付节省。

离线覆盖：建议预览无派发、精确队列参数、撤销与重新接受、旧配置/来源/推荐动作拒绝、硬阈值和 cooldown、停止与绑定前失败、异步旧响应隔离、当前窗历史与跨任务隔离、持久 Reader 重载、命令幂等、原始工作树保留及新 thread 确认。协议 fixture 仅存在于 tests，使用临时工作区和正常 runtime 判断，不发模型请求。

检查命令、通过数量、失败记录及最终源码哈希见 [验证清单](reliability-evidence/t07/verification.json)。真实桌面普通 UI 观察、原生 Claude 及多平台验证仍属 V01/V02/R02；T08 继续负责 Composer 的显式目标任务选择。
