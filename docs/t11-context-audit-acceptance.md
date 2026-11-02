# T11 · 复用审计与逐轮费用归属

日期：2026-11-01。起点及隔离验收基线 `fb47bd3`。在独立 checkout 实现与验证，排除主工作区同期未提交的 gate、validator、知识包及界面工作；复用已有 Cargo/npm 缓存，不作冷启动结论。没有新增真实模型请求、账号操作或桌面验收，不恢复暂停的 9/20；Claude 不可用继续保留 B01。

## 原始计数与逐轮归属

新增原生 UsageAccounting 事件携带带协议标记的 thread total 快照。Codex exec 保留完成回执，另存其累计 total；app-server 保留 last API 回执，另存 total，两者不混用。依据 [官方 exec 映射](https://raw.githubusercontent.com/openai/codex/main/codex-rs/exec/src/event_processor_with_jsonl_output.rs)、[ThreadTokenUsage](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/ThreadTokenUsage.ts) 及 [TokenUsage 统计代码](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)，input 包含 cached input，reasoning 作为 output 的细分展示；新解析不再将 reasoning 额外叠加。旧回执不改写。

启动前冻结实际 resume source turn 与请求的 thread。新窗计数从经明确作用域证明的 total 归属；续窗必须找到精确来源、相同线程/分配/观察身份、已结束且清理确认的基线，才计算各维 checked 差量，并从 inclusive input 中扣除 cache read，避免双计。原始 usage 与 attributedUsage、attributionBasis、baselineTurn 分别提供，读取不回写计数。

缺 scope/total/基线、旧轮次、只提供 last API、线程改变、累计计数回退、负差量或不兼容来源均保持未知；cache-write 的输入重叠口径未核实，因此非零时也不猜拆分。已核实零差量保留为零数量。原生多调用/长窗口阶梯不明确时继续不给逐轮美元等价估价，fast/tier 未核价也不套普通价格。报告美元、Token 等价估价、credits/premium requests 仍独立，均不等于服务商账单。

全局 App 汇总和图表仅累加可归属数量。旧 App Codex usage_events 没有可靠 thread-total 作用域/基线，保留原始回执和明确 CLI 估价，却不将旧累计 token 或存量 token 等价值当逐轮数量/费用。未知数量另计；图表零值仅代表已归属部分，没有把未知补成零。普通聊天旧存储链当前仍缺该归属证据，不借共享 Runtime 的新事件证明其历史成本。重复幂等键发生 raw/归属冲突时拒绝，不混合不相容口径。

## 审计、费用与窗口历史

任务预约时将候选决策、拒绝原因和 resume intent 冻结到该 turn，后续 repair/reassignment 不覆盖之前 attempt。所有生产原生发送冻结实际 launch；审计从持久快照只读投影，不另写可重复累计的 savings 行。REUSE 区分跨任务经济复用、同任务修复/审批/追加续传、FRESH 策略抑制、派发回退、待完成、原生确认及任务验收。源 thread、目标 thread、window origin 和 turn/attempt 保持独立关联；错误 thread 不能记确认成功。

CONTEXT 保留准备/绑定/失败、承载轮完成、原生动作确认及排队撤销。尚无原生轮次的排队记录也保留，费用与时间缺证据时为 null。原生完成不证明任务通过，也不证明 compact 缩减。任务验收只来自 retained commit/integration 证据。

Dashboard 费用证据新增动作下钻，可按 CONTEXT/REUSE、run/task/attempt/source turn/源目标窗口筛选；CLI `run accounting` 同样返回 contextActions。每行能查看参数与输入来源、冻结基线、带符号预测差额、承载轮费用及其证据状态，并关联原始 receipt。费用属于承载 turn，同一回执可被多个动作引用，不能叠加当作动作增量账单。预测差额不是实测节省；measuredSavingUsd 始终 null，未知费用显示“—”，不记已省金额。

## 验证边界

单元覆盖累计差量/缓存包含关系、已知零、counter reset、基线缺失/身份不同/清理未知、app-server last 与 total 分离、复用确认与任务验收区分、负预测、排队撤销及重放。公开 Create/Import/Approve/Start 和原生子进程 fixture 验证修复差量、冻结快照重读、来源变更新窗、错误线程不能通过门禁；fixture 不代表真实模型。组件验证历史/当前窗和 unknown 费用、负预测显示及实际筛选事件。

Runtime 全量 102 项、最终相关账本/复用复验 37 项（重叠）、App 账本汇总 4 项、前端全量 649 项及最终费用下钻组件 4 项（重叠）通过。CLI 编译、build/TypeScript、i18n（2060 keys）、IPC 与隔离检查通过。原生 Codex 专项结果另列于验证清单，实机相关项保持忽略。

早期组件测试漏标 DOM 环境、中文资源插入位置错误、账本旧断言仍把无作用域的累计量当已知值、新集成测试漏写 mutable Reader，均保留失败记录并在对应最终检查通过后交付。最后只对非法可选缓存写计数增加严格拒绝，并以原生解析专项复验；不放宽超时、审批或生产门禁。

命令、完整数量、失败修复记录与源码/制品哈希见 [验证清单](reliability-evidence/t11/verification.json)。真实正常 UI 的上下文/复用端到端仍留 R02，不声称新桌面、真实美元节省、跨平台或长期计费已验收。下一项 T12 负责配置与已结算样本校准，T12 须排除未知或未归属 Codex 样本，不能以原始累计量回填训练标签。
