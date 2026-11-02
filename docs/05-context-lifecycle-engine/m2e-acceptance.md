# M2.e：真实桌面限定验收（2026-10-30）

M2.e1 自动排队/撤销与 M2.e2 真正 FRESH 新窗在当前 Mac、Codex CLI 0.160.0 上通过限定验收。M2.c2/c3 与 M2.e3 的 Claude 原生路径仍未完成：唯一一次普通请求返回账号受限，未执行 `/compact`。M2 总阶段保持未勾选。

## 候选与隔离

使用 external-ui-v1，在提交 `fbdac3aaa66b4b3a5ad81c5a381cccc8e986a2f2` 的独立克隆构建 release App，通过正常 UI 选择项目、信任、模型和设置、发送消息、展开历史并撤销/采纳。未写 Store/数据库、未注入 UI、未降低规划阈值、未改价格或窗口。克隆结束时受跟踪源码仍干净。原生 CLI 沿用本机已有账号，不复制认证；WebView 为 ephemeral，本次不证明跨重启前端持久化。

外部根目录为 `/Users/iwpz/Documents/tokage-validation/tokage-desktop-test-8xv025o2`。注册、构建身份、设置/操作记录及校验结果见 [证据目录](../reliability-evidence/context/m2e-20261030/verification.json)、[操作记录](../reliability-evidence/context/m2e-20261030/ui-observations.md)、[候选身份](../reliability-evidence/context/m2e-20261030/candidate-build.json)。屏幕观察来自本会话 CUA；未单独归档 PNG。原始 SQLite、完整任务材料和原生输入保留在外部目录，仅提交路径/摘要/哈希，不提交数据库或认证。

## M2.e1：auto 与撤销

普通主会话开启 Auto，最小节省 15%，额外热报告轮关闭。GPT 6-Luna Medium 的真实代码审计材料使 CLI 上报上下文达到 133,488 token。未点击采纳，自动产生 action 1（COMPACT / PLANNER / SUGGESTED）。普通 UI 取消后为 UNDONE，目标和 appliedAt 均为空；下一轮仍使用原线程，不带 compactAction/保留前缀。

下一轮上下文达到 378,201 token，自动产生 action 2。正常发送消费同一记录为 APPLIED，目标 `7c74691d-456a-4225-bc1d-8b8bd6be026b`；本地保留信息进入新原生线程，完成后为 22,651 input / 191 output。该动作的 kind 是 COMPACT、执行 method 为 fresh，不将它冒充 FRESH-kind 验收。当前新窗历史包含传入 action 2，不包含旧窗已撤销 action 1。

## M2.e2：FRESH-kind 与首包

另一个 GPT 5.6-Luna Medium 主会话在 cache-aware 开启时达到 138,970 token，实际仍推荐 COMPACT（action 3）。按预注册边界停止该分支并正常撤销，保留这个未触发 FRESH 的样本。初始可达性估计忽略了 Codex compact ratio 为 0.05；长上下文价格门槛实际为 272,000，观察记录保留并更正这些预测错误，没有为达到结论改包参数。

随后单独预注册正常产品分支：在设置中关闭 cache-aware，其余规划参数不变，仍关闭额外热报告轮。这表示按无缓存估价，不证明供应商缓存实际冷却或未命中。预览成本为 continue 0.136299 / compact 0.124802 / fresh 0.121244，推荐 FRESH。撤销同源抑制仍生效，设置变更未自动复活 action 3；正常显式采纳创建 action 4（FRESH / USER），再正常发送使其 APPLIED。

旧原生线程 `01a18a1c-82e1-7921-a1cf-3aef2a49d448`，新线程 `01a18a25-bfbf-7080-af2e-21fa6df99b37`；目标消息 `e05553d1-c91b-48b8-a2bd-41777b949108`。进展来源 LOCAL_EXTRACTION，包含该窗两条实际来源消息。新窗首条任务严格等于 preparedPayload、分隔符与原用户问题的拼接；6469 字节，SHA-256 `7124a79f17ed75fa8242622503398d6560b6577b2752a7276a85000c4965547a`。承载轮完成，上报 21,527 input / 233 output。证据见 [FRESH 校验](../reliability-evidence/context/m2e-20261030/fresh-verification.json)。本次不证明摘要语义完整、热缓存新报告生成、其他 provider/平台或真实美元节省。

## M2.c / M2.e3：原生阻挡与回退

Claude Code 2.1.288 / Haiku 4.5 20251001 在唯一一次正常请求中返回 `Your account is on hold and can't use Claude Code`。立即停止 Claude，不重试、不改账号或认证、不绕过能力门槛。没有真实 compact_boundary，也没有执行原生命令，安装版本不能替代实测。详见 [Claude 观察](../reliability-evidence/context/m2e-20261030/claude-observation.json)。因此 c2/c3 和 e3 原生部分保持未勾选。

Codex 的 SYNTHESIZED_FRESH_BRIEF 回退已通过真实首包/新线程核对，UI 显示本地抽取及尚未证实实际缩减；该限定回退结果不能关闭整个 e3。M2.d 中已真实观察撤销与当前窗历史，失败恢复/取消等其余矩阵仍只有此前离线覆盖。

## 费用、偏差与独立复核

共请求 8 轮：7 轮 Codex 完成，1 轮 Claude 账号错误。Codex 结构化 tokens 不含供应商美元费用；7 条 usage 的 cost_usd 均为 NULL。知识包等价值合计 0.10894914 仅为估价，不是实际花费。两条 CONTEXT 记录使用 horizon=1 / first_turn_counterfactual_v1；action 2/4 基线分别约 0.05447852 / 0.0562506，实际费用 NULL、settled=false、actualSaving=NULL，原因 carrier_cost_unknown，不将未知记零或宣称节省。无额外模型报告轮。

初始 UI 数字输入意外留下 timeout 0.53；发现后通过普通键盘输入纠正并可见确认 3，再开始 Codex。Claude 的立即账号错误样本保留这一偏差，不据此作超时验收声明。设置返回会重置 composer mode，最终 FRESH 发送前显式重新确认 Just do it 与同一模型。大消息在 App 日志有意截断；原生完整任务正文的字节/哈希均匹配，不能以日志摘录误判传输截断。审计模型的回答仅为工作负载，不作为已验证的代码缺陷。

[只读复核脚本](../reliability-evidence/context/m2e-20261030/verify-result.py) 对一致 SQLite 快照、冻结知识包和本任务原生线程文件核对：4 个动作的状态/绑定、2 个新窗完整首包和来源、原始三个排队场景的独立成本公式重算、同一首轮基线及未知费用语义。结果为 scoped-pass-with-native-account-block。历史登记的 7 个子进程/进程组均已退出，没有残留流式消息或待审批；没有为清理杀进程。哈希清单见 [sha256.json](../reliability-evidence/context/m2e-20261030/sha256.json)。

候选独立构建通过，前端 88 文件 / 580 测试通过，i18n 1924 键通过。验收后仅修正中英文设置中的陈旧“原生压缩尚未开放”文案，改为依 CLI/版本启用；该两处文案通过主工作区生产构建、i18n 和隔离检查，未重跑付费模型。上述真实运行证据的源码身份始终是 fbdac3a，不将新文案声称为另一轮真机验收。
