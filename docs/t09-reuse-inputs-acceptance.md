# T09 · 共享复用决策的实际输入与来源

日期：2026-11-01。起点提交 `87a2d24`，写入范围为共享上下文/调度、原生 Codex 用量解析、App/CLI 配置传递、相关离线测试及本文档。完成实现与离线验证；没有新增模型请求、账号操作或普通桌面复用验收，不恢复暂停的 9/20。真实端到端验收仍在 R02，Claude 不可用仍在 B01。

## 生产输入

`execution::choose_context` 使用唯一 `reuse_inputs` resolver。目标模型的价格、上下文窗口、有效容量、缓存 TTL/倍率及长窗口价格来自冻结的有效知识包；模型身份未知/歧义、fast/tier 未核价或动态价格表未支持时拒绝复用。长窗口倍率按 reuse/fresh 各自每轮上下文独立计算。容量包含预期轮数内的增长。

App 冻结当前 cacheAware/minSaving；CLI `run create` 提供同名语义的公开选项。旧配置缺字段时保持原序列化/hash，采用明确标注的历史默认值（cacheAware=true、minSaving=0.15）；新配置验证有限值及 0..1 范围，0 是有效阈值。contextPolicy=auto 才能执行复用，off/suggest 不自动复用。

brief 使用与实际执行同一个 prompt builder，包含合同、上游报告、失败证据、历史需求与冻结授权阶段。UTF-8 字节数除以 4 是 token 估算；CLI 隐含 framing 仍属于 baseline 先验，不能称 tokenizer 实测。窗口总占用与 baseline/history 分开，B+H 等于该总占用，累计 input/output/cache tokens 不能回填窗口占用。

同域同 size 且可比的已核实观察用于增长/API 次数；没有观察时取知识包 domain/size 画像，再缺失取共享 provider 先验。新任务轮数采用目标画像预测，不能拿旧任务已完成轮数充当未来轮数。baseline 仍是共享 resolver 的知识包/Provider 估算；相关历史上限 30,000、rework_phi=0.3 是具名先验，未作语义相关性测量或费用校准，后续配置/校准留 T12。预期轮数 1..512 和每轮 API 次数上限用于计算边界，NaN/无穷/负参数不批准复用。

每次候选决策持久保留 configHash、packVersion、sourceTurnIds、historyComplete、逐项 sources、冻结选项、参数和决策；costEstimates 未计算时为 null，计算时明确 FORECAST_NOT_BILLING。所有差额都是反事实预测，不能称账单或实测节省。未知原生占用、旧未核实字段、历史不完整、cacheAware 关闭、implicit/none 缓存策略均保守新窗。旧 `findReusableWindow` 缺这些权威输入，已停止凭固定参数批准兼容链复用。

## Codex 原生数据修正

核对 [官方 exec 实现](https://raw.githubusercontent.com/openai/codex/main/codex-rs/exec/src/event_processor_with_jsonl_output.rs) 与 [官方 app-server 类型](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/ThreadTokenUsage.ts)：exec 完成事件输出累计 total，app-server 区分 last/total。exec 保留原始 usage，但不再把累计 input 当 contextTokens；两条链均不再把 command 事件数当 API 调用次数。app-server last 上下文快照保留。Codex output 的累计值/最后 API 值也不能用作每 Tokage turn 的可比输出观察，使用标注的画像预测。

新观察带 contextMetricsVersion=2；历史快照没有该标记时，原始回执保留但不会被静默升级为可信窗口占用。窗口未知时 T06 仍能使用带来源的可见载荷估算做上下文保护；跨任务自动复用不以该估算授权。测试中原先用巨大累计 input 触发窗口阈值的夹具已改为实际大提示词，生产没有新增测试例外。

本项没有标准化费用账本的 Codex 累计差分/逐轮归属；现有 raw token 等价估价不能当作已核实逐轮成本，该缺口明确进入 T11。T10 继续核对 cwd/worktree/权限/线程身份和匹配阶段成本，不把本项参数修正当作身份约束已闭环。

## 离线证据

单元覆盖真实窗口观察、已知零/未知/旧字段、画像回退、模型价格/容量/缓存冷热、长 brief、用户阈值、动态价格及历史不完整。共享调度正例经 reserve_ready 产生复用；公开服务协议场景经 Create/Import/Approve/Start、两任务依赖与原生子进程夹具验证累计用量不冒充占用、实际 prompt 字节预测、保守新窗和持久重读。协议夹具不代表真实模型。

前端全量 647 项、context 50 项（48 单元＋2 协议）、Runtime 91 项（24 单元＋67 协议）、CLI 23 项通过；相关 Codex 原生解析 29 项通过，3 项实机测试忽略。最终 build/TypeScript、App 编译、i18n 与隔离检查通过。首轮并行调度测试超出原有 30 秒等待，保持超时不变后完整相关套件重跑通过；首次编译/类型错误与中止的冗余总验证记录均保留，不算通过。

最终验证数量、命令、失败后修复记录及源码/制品 hash 见 [验证清单](reliability-evidence/t09/verification.json)。前端、Runtime/context 全量及相关 native/CLI 离线检查分开统计，重复集合不相加。没有真实模型节省、桌面复用效果、其他平台或长期可靠性的新增结论。
