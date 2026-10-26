# M1.b：上下文规划的真实输入

2026-10-26。范围为 M1.b1–b5：当前窗口的用量与估算来源、有效知识包的模型限制/价格/缓存参数、滑动统计，以及只读生产接线。不修改运行授权、模型选择、派发、自动压缩、FRESH 或节省结算。

## 当前调用链与边界

共享 runtime 迁移后，原 `Workspace → planInWindowContext` 调用已移除，设置默认 contextMode=off，界面公开说明自动建议与压缩暂未开放。因此不把旧 M1.a 勾选解释为当前完整建议功能仍可用，也不借 M1.b 恢复执行控制。

开启上下文展示后：Workspace 从 runtime 的 durable turns / transcript（旧会话从最后完成消息）提取不含正文的统计 → `resolveContextInputs` → 正常 `resolve_context_inputs_cmd` → 与匹配引擎相同的有效分层知识包 → `tokage_context::inputs::resolve_inputs` → 实际 `plan_context_action` 只读预览。返回完整参数、逐字段来源、知识包版本、模型 ID、样本数及只读预览。上下文栏消费同一份总量、有效窗口和缓存热度；未知不画成已测 0%。

新 IPC 不启动 CLI、不联网、不写 Store/DB/action 记录，不改变 runtime 的冻结配置；当前有效知识包提供的是只读估价，不能替换运行中的授权或成本记录。参数来源与预览保存在当前视图投影中，切换窗口/知识包后重新解析；迟到响应不能覆盖新窗口。正常关闭上下文展示时不发解析请求。这里不是持久化动作历史，后者仍属 M1.c/M1.f。

## M1.b1：总量与估算

- 优先最后完成 turn 的 `usage.contextTokens`，其次其 `done.contextTokens`；有效 0 保留。CLI 字段表示最后请求的总上下文 B+H，不再次加基线。累计 input/cache 用量可能跨多次 API 调用，禁止拿它充当当前上下文。
- CLI 不给 B/H 分项：B 为明确的 Provider 基线先验，并限制在已知总量以内；H=总量−估计 B。来源分别标记，不能称 B 为实测。
- 没有 contextTokens 时，只估算同窗可见内容：ASCII 字符 0.25 token、其他 Unicode 码点 0.5 token，向上取整；加上基线先验。不可见的系统提示、内部工具内容等仍未知，UI 明示这是估算而非完整测量。
- runtime 按 native thread、Provider、requested model、角色和 task 同时隔离；无 thread ID 时只认该 turn。并行任务和不同审查窗口不累计。旧消息没有可靠逐消息 native window 归属，保守只取最后完成助手消息及相邻用户文本，不把整个旧会话串起来。

## M1.b2–b3：模型限制与成本

CLI 报告实际模型/Provider 时优先使用该身份；未报告才使用明确标记为 REQUESTED_IDENTITY_UNCONFIRMED 的请求身份，不把请求参数当实际型号证明。同窗统计也排除实际模型变化。Provider + CLI model argument 精确匹配优先，其次既有参数匹配器；多候选歧义或未知模型明确返回不可用，不根据名称中的 claude/gemini/1m 猜窗口。`contextWindow/effectiveContext` 和价格取自有效候选（包含 provider offering 覆盖及自定义层/本地覆盖），有效上下文不超过物理窗口。

CLI 自动压缩比例与压缩输出比例来自 Provider context 元数据；缺失使用标注的 0.92/0.06 规划先验。成本读取当前模型 price_per_mtok，超过已声明长上下文阈值时应用对应输入/输出倍率。缓存读写倍率优先模型 cache_rates，再 Provider 定义；缺失保持中性倍率 1，不臆造折扣。这些是定义包估价/等价成本，不是实际扣费。

## M1.b4：缓存语义

Rust/TS 统一 `ephemeral/disk_persistent/implicit/none`；兼容旧 standard 和定义包 explicit_prefix/automatic_prefix 到 ephemeral。未知模式和 none 不因 TTL 非零而被当作热缓存。

TTL 优先 Provider 定义。by_model 使用模型的明确 vendor 元数据选择集中策略先验（不猜模型名）：anthropic→ephemeral/300s，openai→ephemeral/1800s，deepseek→disk_persistent/86400s，google/alibaba→implicit/未知 TTL，其他→none；来源记录为 PACK_MODEL_VENDOR_POLICY_PRIOR。这些沿用既有产品策略，不代表本次做过厂商保留时间或实际命中验证。

所有有界保留时间在 TTL×0.9 边界变冷；持久存储不再等于永久热缓存。implicit 没有可靠命中证据时不宣称热；目前输入可靠性为 0。关闭 cacheAware 后使用 none、TTL=0、中性倍率。上下文栏倒计时和 mock 使用相同边界；实际缓存命中仍由服务商决定。

## M1.b5：统计与默认表

同窗口已完成记录按结束时间排序去重，排除未来记录，统计最近 8 轮。API 次数来自明确 apiCalls，输出来自明确 outputTokens，0 有效；上下文增长使用连续 CLI contextTokens 的非负增量，压缩/重置造成的负差不作为负增长。缺少可用样本时回到集中先验，不从消息条数虚构 API 次数。

剩余轮数无法由已完成轮数直接观测：采用 Provider 总轮数先验减去已完成轮数、最低 1 的预测，并明确标记 FORECAST；不是声称知道何时结束。默认表元组为（B、总轮数先验、每轮 API 次数、每轮增长、每轮输出）：

- Claude/Cursor：（12000，4，6，8000，1000）。
- Codex/Copilot/OpenCode/Reasonix/Kiro：（8000，4，4，6000，1000）。
- Antigravity：（10000，4，6，8000，1000）。
- Aider：（4000，3，1，4000，1000）。
- 其他已定义模型的 Provider：（8000，4，1，6000，1000）。

表是待后续校准的规划启发值；不是厂商测量、自动学习或成功率承诺。`turnsSinceLastCompact` 继续为带来源标记的 0，不伪造已经完成 5 轮；真实动作冷却跟踪仍属于 M1.c。返回 preview 只读，不可据此自动执行动作。

## 验证与剩余工作

离线覆盖 CLI 0/缺失、累计输入不冒充上下文、候选窗口/价格/模型缓存倍率、长上下文价格、未知模型、同窗隔离/重复/未来记录、滑动统计、负增量、TTL 临界点、none/implicit、跨会话迟到响应、未知 UI 与持久缓存过期。

M1.c–M1.g、M2、M3 保持未完成；本节点不宣称已执行压缩、完成节省结算，或完成真实桌面/模型验收。实际动态窗口与压缩建议需要之后按正常外部 UI 协议单独验收。

### 本次验证结果

- 上下文 Rust 库全量 28 项通过；包括新解析器、实际只读规划预览、0 值、价格阶梯和缓存语义。
- 前端全量 77 文件 / 500 项通过；包含实际模型身份优先、不同窗口隔离、估算不冒充 CLI 报告、迟到响应和禁用状态。
- `npm run build`（含 TypeScript）、i18n（1873 键）、桌面 crate `cargo check`、测试隔离、差异检查通过。
- IPC 接线检查：137 个命令，未豁免的无生产调用项为 0。新调用使用统一 IPC 包装，不新增检查豁免。

开发中曾出现 Storage 不存在 with_conn、连接临时借用类型不匹配，以及来源映射推断缺少显式类型的编译错误；均已修复并重新检查。新命令初次直接调用未被接线检查识别，已调整为正常统一 IPC 调用链。中间编译错误不追认为通过。

没有真实模型请求、付费验收、应用重启或桌面 UI 通过声明；没有修改账号配置、用户环境或 `.tokage/`。建议/压缩开关默认值保持 off，关闭时不构造窗口统计或发解析请求。

后续状态：M1.b 的 cooldown=0 占位及历史拼接限制已由 [M1.c 冷却与动作历史](m1c-cooldown.md) 的持久重建/逐消息窗口归属补充；M1.b 本节保留当时验收范围，不据此声称压缩执行完成。
