# V01 桌面验收：本地抽取进展

2026-11-19（Asia/Tokyo），本地抽取和新窗交接通过。没有可复用报告时，Tokage 从上一轮问答中提取进展，新线程收到这些摘录和完整的原始消息，并完成回复。

## 桌面操作

沿用 [首轮验收](v01-warm-report-acceptance.md) 的 macOS ARM64 隔离构建 `dd37fd1e39753fd1dacbadf18b9459f8f115a76c` 与 `external-ui-v1`。应用数据目录独立，WebView 为临时模式，CLI HOME 使用日常登录环境。通过正常界面在隔离项目里新建会话，使用 Codex CLI 0.161.0 与 GPT-6-Luna Medium 只读直聊。Context 使用 Suggest，缓存估价开启，额外热报告生成关闭，保留 15% 节省阈值和估算 300 token 的报告预算。Debug Provider、Demo 和 Full Auto 关闭。

1. 在旧线程正常发送两轮，第二轮让 Luna 审阅登记的源码材料并给出两条简短结论。回复里没有可复用的完整报告，也没有生成报告的标记。
2. Tokage 建议下一轮带着保留上下文进入新窗口。点击 `Apply next turn` 后，动作 4 为 `COMPACT / SUGGESTED`。只读记录确认来源为 `LOCAL_EXTRACTION`，并绑定旧线程两条来源消息。
3. 点击 `Send` 一次。动作 4 转为 `APPLIED`，执行方式为 `fresh`。这次发送只启动一次新线程请求，旧线程新增零轮，没有额外报告用量条目。新线程完成回复。

## 首条请求复核

使用已登记的上一轮用户正文和保存的 Luna 回复独立重建本地进展，与排队记录匹配。进展原文为 1,333 字节，默认预算下送出 1,200 字节，包含上一轮请求和助手回复的摘录，末尾有 `[truncated excerpt]` 标记。新线程的原生首条请求为 2,515 字节，与保留简报、分隔符和 281 字节的原始消息逐字节匹配。原始消息中的中文、标点和换行均保留。

实际美元费用未知，结算记录为 `carrier_cost_unknown`。隔离检查与 58 项关联离线测试通过。隔离克隆干净，已登记的模型进程组均已退出。

[只读校验结果与哈希](reliability-evidence/context/v01-20261119/local-extraction-verification.json)。原始桌面观察、预登记材料及复核脚本保留在结果记录的外部验收目录。
