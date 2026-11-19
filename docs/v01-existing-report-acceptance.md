# V01 桌面验收：已有完整报告复用

2026-11-19（Asia/Tokyo），已有完整报告复用通过。发送下一条消息后，Tokage 在新 Codex 线程只启动一轮请求，旧线程新增零轮。新线程收到登记的原始消息，逐字节一致。

## 桌面操作

沿用 [首轮验收](v01-warm-report-acceptance.md) 的 macOS ARM64 隔离构建 `dd37fd1e39753fd1dacbadf18b9459f8f115a76c` 与 `external-ui-v1`。在同一测试项目通过正常界面新建会话。Codex CLI 0.161.0 使用 GPT-6-Luna Medium、只读直聊。Context Suggest、缓存估价与热报告开关均已确认，最小节省阈值为 15%，报告预算为估算 300 token。Debug Provider、Demo 和 Full Auto 均关闭。应用数据目录独立，WebView 为临时模式，CLI HOME 沿用日常登录环境。

1. 在新会话先发送一条简短检查请求，再用普通消息让 Luna 审阅登记的源码材料并撰写进展报告。第二次回复包含五个规定标题，每节都有正文，属于用户主动请求的普通答复。
2. Tokage 显示预计缓存仍热，建议下一轮带着保留的上下文进入新窗口。点击 `Apply next turn` 后，动作 3 为 `COMPACT / SUGGESTED`。只读记录确认已选中本轮完整报告，来源为 `EXISTING_MODEL_REPORT`。
3. 点击 `Send` 一次。动作 3 变为 `APPLIED`，执行方式为 `fresh`。原生 CLI 的线程记录显示旧线程仍为已完成的两轮，新线程完成一轮。新增的唯一用量条目为 `DIRECT`，额外报告轮和 `CONTEXT` 用量条目均为零。

## 新窗口收到的内容

旧窗报告为 1,590 字节。默认预算按 UTF-8 字节估算，新窗收到 1,200 字节的已有报告摘录，末尾带 `[truncated excerpt]`。新线程首条原生请求（2,601 字节）与保留简报、分隔符和原始问题的拼接逐字节匹配。原始问题本身完整，新窗回复完成。

新窗调用的实际美元费用为未知，记录保留未结算（`carrier_cost_unknown`）。启动前执行隔离检查，完成后复核两个线程、数据记录及进程组。隔离克隆干净，已登记的模型进程组均已退出。离线复跑上下文与报告相关的 58 项测试，通过 Node 22.16.0 完成。默认 Node 20.12.2 下 Vitest 启动失败，重跑只切换离线测试的 Node 可执行文件。

[只读校验结果与哈希](reliability-evidence/context/v01-20261119/existing-report-verification.json)。原始桌面观察、预登记材料及复核脚本保留在结果记录的外部验收目录。
