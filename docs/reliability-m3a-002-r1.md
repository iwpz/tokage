# m3a-002-r1：内部通知被误显示为 UNKNOWN/default

- [x] 定位计划启动提示来源：`PlanReview` 在确认计划后调用 `postSystemMessage`，没有发起模型请求。
- [x] 核对既有证据：`reliability-evidence/m3a/m3a-002/attempt-2/tokage.db` 中，`9a292dad-4ab8-4f17-9ca0-1551a65184fb|UNKNOWN|` 仅包含该启动提示，状态 IDLE、无任务 ID、模型为空、无对应用量事件。实际任务记录为 CODEX_CLI / gpt-6-luna / DONE。
- [x] 给新内部通知记录明确的 `origin: tokage`。存储使用 SYSTEM 消息角色、TOKAGE_INTERNAL 归档来源、空模型、IDLE 状态及零模型回复轮数；保存重试与重新加载保留消息和来源。不借用任何真实模型身份。
- [x] Session 页面显示“Tokage 内部通知 / 无模型记录”，不显示 Executor 标签。历史 UNKNOWN 且无模型、无任务、无 CLI 会话身份的主会话记录显示“未归属消息（历史记录）”，内容仍可读。该兼容显示不认定所有旧消息均为内部通知，也不隐藏未知渠道的真实任务。
- [x] 更新桌面验收脚本允许的精确内部通知归档身份，保持旧证据可验证。既有证据数据库与清单不改写。
- [x] 验证：前端全量 333 项、tokage-storage 全量 37 项、TypeScript、i18n、生产构建、Python 脚本编译及差异空白检查通过。

本节点为 m3a-002 的缺陷修复，不计为第 3 个真实需求。本次未运行额外付费模型请求：问题及回归覆盖的是本地通知生成、持久化、重载和显示；既有真实运行证据用于核对渠道身份。未重新执行桌面端到端验收。

存储沿用现有 attempts 表保存主会话消息的结构；内部通知归档的兼容 role 字段仍为 EXECUTOR，但 UI 不将其显示为执行者，消息角色与来源已明确区分。
