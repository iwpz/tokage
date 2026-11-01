# T12 · 统一上下文配置与有来源的估计校准

日期：2026-11-01。起点 `f710d3b`。在独立 checkout 实施，排除主工作区并行进行的 gate、validator、知识包和界面改动；没有读取/修改个人业务数据、调用真实模型、安装 Claude、操作账号或恢复暂停的 9/20。

## 统一参数与保留材料

共享 `ContextTuning` 默认 brief=3000、report=300、rework_phi=0.3、最低校准样本=5。App Settings 与 CLI create 使用同一语义：brief 256..32000、report 32..brief、phi 有限且 0..1、最低样本 3..100；零 phi/minSaving 不当作缺省。App settings v8 为缺失/非法 tuning 补默认，并保留合法自定义值；运行冻结 tuning、当前合格本地覆盖及来源，旧配置缺字段时读取不改变 hash。既有审批不随新统计重估，更新有效配置仍走正常编辑/重新授权规则。Runtime UI 预览也取冻结配置，而非随后改变的桌面设置。

主会话与任务窗共用 resolver。报告请求使用配置上限；只对可选报告摘录做 UTF-8 安全裁剪并标注 truncated，保留原始 retained 数据供精确来源校验。必要目标、合同与未解决错误不裁剪；保留前缀超限时动作 unavailable/阻断，不静默丢失要求。旧待发意图须通过当前 resolver 重新核验，失效时撤销并重新准备，不自动扩大范围。

这里的 token 预算是可见 UTF-8 字节/4 估算，并非 tokenizer 实测或服务商 max-output 硬限制。预算约束 retained context prefix/可选报告，不截断原始任务 brief 或用户后续消息；真实 CLI framing、完整任务提示词和消息可以使总输入更大。报告生成请求是提示约束，超出返回内容只在后续传递时有界摘录。FRESH 成本预测使用配置的 brief/report 预算包络，rework_phi 保持显式先验，不由未经控制的节省差额反推因果成本。既有 30,000 相关历史上限、Provider baseline 和 domain/size 使用画像仍是有标签的先验，未伪称新测量。

## 仅使用合格结算证据

旧 `learn_usage_from_attempt` 会用缺失调用数/增长默认值，甚至以 equiv_cost_usd 填实际费用。本项停用该回写，并停止将旧无来源 usage/cost learned_stats 装入新有效策略；历史数据保留，不做破坏性迁移，质量后验继续使用 T03 的核验投影。

新账本在共享 Runtime 的正常状态事务中写入 `settings` 的版本键 `context.calibration.v1`，与 accepted task 状态同事务提交。这里只说本地原生回执终结且任务通过验收，不代表服务商对账结算或已付款。合格样本要求：executor 原生 Completed/ended/cleanup、任务 DONE、门禁成功且清理确认、commit/integration 证据、非人工接手/无合同偏差、明确 launch、非续窗、Structured 且可归属 token、正的 CLI 报告美元、已报告且匹配的模型，以及 T12 冻结的价格签名/系数。续窗美元作用域未证明，不能凭 token 差量推断美元差量。

缺费用、零美元不可用作价格倍率、估价、未结算、未验收、legacy price、身份变化、未知阶梯/fast/tier 价格和不可归属 Codex 计数均排除；不使用 credits、Token 等价估价或原始累计量伪造观测标签。CLI 报告美元也不等于账单，校准只改善本地 Token 等价值预测。

样本键为 run/turn/口径版本，重复进展/读取不重复学习；已结算证据冲突拒绝。按 Provider/规范模型/基础价格/缓存倍率与政策/阶梯签名分组，价格签名改变不沿用旧系数。用 0.2..5 内有效比率的中位数，达到阈值后才生成本地覆盖；异常值排除，样本不足保持知识包先验。拟合时除去该轮已应用的系数，避免学习自身修正并累乘；应用覆盖也幂等。每个账本最多 10,000 有效与 10,000 排除记录，达容量时明确报告且不悄悄计新样本；损坏/越界数据返回错误，不重置为伪正常。

覆盖进入新 App/CLI 冻结策略，再作用于共享 matching、预算和上下文价格预测；origin layer、PriceBasis.calibrationFactor/calibrationKey、冻结配置均可追溯。普通主会话 resolver 使用同一份本地校准，排队与准备再次用相同解析器核验。旧兼容分配器不获得一套不带证据的学习回写。样本不足/读取错误/容量边界在设置中可见，可手动刷新；CLI `run calibration --minimum-samples N` 只读查询。无桌面能力时明确 unavailable，不以零记录冒充已读取本地数据。

## 验证与限制

覆盖默认迁移/显式零与自定义值、App 冻结设置、CLI flags/非法值、UTF-8 报告裁剪且不改原始 retention、超限不切合同、幂等持久化/重开、阈值/异常/未结算/估价/续窗排除、反反馈、价格签名变化及设置组件。公开原生协议 fixture 验证 Codex token 完成且美元未知时仅落排除记录，重读不变；不代表真实账号校准通过。

早期共享 Cargo 输出目录与并行开发覆盖产物，导致 App 读到旧依赖类型；该目录结果不作最终验收。最终使用独立 Cargo target、dev/test debug=0、incremental=0，前端最多两个 worker、Rust 两 job/测试线程。完整回归还发现大型公开 submit future 在长交付恢复场景栈溢出；改为正常生产入口堆分配异步状态，未增大测试栈/超时，原失败场景与 future 尺寸回归通过。保留编译夹具字段错误、AppHandle 接线错误、共享缓存失配、栈失败及全部修复后记录。

最终命令、数量、源码和制品 hash 见 [验证清单](reliability-evidence/t12/verification.json)。当前没有新的真实模型/桌面校准、账单或节省结论；V01/V02/R02 等真实验收、B01 Claude 条件及暂停的 9/20 保持原状。T01–T12 开发主线完成不代表全项目验收或发布完成。
