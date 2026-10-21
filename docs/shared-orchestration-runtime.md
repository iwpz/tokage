# sh2：共享运行服务实现记录

sh2a–sh2e 已完成；本文件记录已实现边界与验证结果。旧 App/CLI 运行没有切换引擎，接入和退役仍属于 sh3。10/20 真实需求继续暂停。

## sh2a：计划与授权

runtime 使用既有 planning parser/validator、matching allocation 和 orchestration gate-command safety。普通 `ImportPlan` 与模型生成结果走相同规范化路径；任务按 ID 固定顺序，作用域冲突由既有 validator 串行化。计划必须含有效 DAG、范围和可运行验收命令；超出运行范围、路径穿越和不可确认的 shell 组合命令拒绝。任意 glob 包含关系不能由 overlap 推断，包含证明只接受相同 glob 或明确的目录父前缀。

模型目录来自冻结配置；任务锁优先于 executor 锁，锁定 provider/model/path 原样保留，模型输出的 assigned_model/model_params 不取得用户授权。未锁定任务复用匹配引擎，在稳定排序的可用候选中分配。未知显式模型允许保留，价格不会因此推断为零；预算预留属于 sh2c。自定义包逐个核对 SHA256，并用既有安全合并规则构建有效 pack；层内容和 override 同样包含在配置摘要中。

`ApprovePlan` 绑定规范化计划、分配、完整有效配置、工作区/目标、策略版本和用户确认命令。模型提供的 confirmedCommands 被丢弃。未确认的命令作为可编辑问题保留，但不能批准；禁止命令不会因用户确认而放行。写权限和 workspace trust 必须显式存在。`UpdateConfig` 或重新导入/编辑会清除旧批准；成功批准命令的幂等重放只返回旧 receipt，不能重新批准变化后的计划。执行开始后的编辑不开放。

`GeneratePlan` 先提交 Planning 意图，再由 `owned::Runtime::submit` 在独立事务中持久化唯一 claim，随后调用 Planner 适配器；结果和事件原子保存。`NativePlanner` 使用正常 Provider 检测、明确 provider_path、只读原生 adapter、现有 repo digest/prompt、超时和脱敏。不改变全局环境、账号、Git 身份或原生全局进程目录；内部 ChatRequest 的进程目录不能从传输 JSON 设置。结束必须观察 structured Done、无错误且原生进程已清理，工作区快照未变化，然后才接受计划。进程存在不等于计划成功。

生成失败进入 Blocked 并保存错误。claim 后进程崩溃或完成结果写入失败时保留未知 Planning，重复 drive 被拒绝，绝不自动再付费调用。sh2b 将此路径统一为服务持有的后台 worker；不再公开 sh2a 的直接 drive 路径。Planner trait 是正常适配边界，离线实现只放在 tests 中。

## sh2b 完成时的后续边界

以下 sh2c–sh2e 记录扩展后的服务实现；sh2a/sh2b 的验证结果不单独证明完整运行链路。

## sh2a 验证记录

2026-10-21：runtime 3 项基础契约、6 项计划、6 项仓储测试通过（15 项）。覆盖稳定规范化和自动分配、模型锁、普通导入/编辑、旧授权失效、模型伪确认拒绝、范围/DAG/危险命令拒绝、生成失败、完成写入故障后的禁止重复调用、原生只读 Provider 协议进程。离线进程 fixture 仅通过测试引用既有 cli/tests/fixtures/codex.py，未复制进生产入口，未调用真实模型。

桌面 workspace `cargo check --locked --lib -j 2` 与独立 CLI workspace `cargo check -j 2` 通过，两个 lockfile 固定 runtime 新依赖。`python3 scripts/check-test-isolation.py` 和 `git diff --check` 通过。未进行 UI、真实模型或交付验收；旧 App/CLI 能力回归将在入口迁移时保留。

## sh2b：turn 所有权、事件、审批和取消

`owned::Runtime` 持有仓储和 owner lease。后台 worker 持有服务自身，释放视图 handle 不会取消任务；正常显式 CancelRun 先原子保存 CancelRequested，再记录 Stopping 并通知原生清理。只有 worker 已返回且确认进程清理后才能变成 Stopped；Done 仅是临时完成帧，不能覆盖同时接受的取消。worker 异常或 DB 故障可以查询 worker_error，持久状态保持最后一个成功事务；ResumeRun 将未知副作用明确阻断，绝不重派发。当前只在服务中恢复证据，不猜测上个进程成功；任务/Git 精确对账属于 sh2d。

V15 新增独立 transcript 表，按 run/turn/sequence 分页；投影、业务事件及 transcript 在同一 writer 事务提交。重复同序号同内容无副作用，不同内容冲突；序号缺口拒绝，合法相同文字的新序号保留。单次最多 64 条原生观察批量持久化；后台传输队列有界，过量原生积压导致明确失败和清理，不借无限界面队列承诺交付。视图只读取持久事件，慢视图不影响执行。

每 turn 只保留一份 usage receipt，相同 Done 不累加，冲突的 structured receipt 阻断完成。requested provider/model 与 observed 字段独立；未知 observed provider/model、costUsd 为 null。原生输出先经现有 Redactor，规划结果必须与持久 Text transcript 一致；没有 structured Done、进程结束和清理证据不能接受计划。原生调用与生成路径共用此生命周期；执行/审查派发将在后续子节点接入。

审批窗口保存 approvalId、turnId、原窗口 revision、摘要、具体命令和决策；过期/重复窗口不能产生新 turn。Deny 阻断。沿用现有 App headless 边界：AllowOnce 不扩大成会话授权；仅 CLAUDE_CODE、可恢复窗口、真实 thread id 和不含规则元字符的 AllowSession 才能创建一个关联的 continuation；其他能力明确进入人工阻断。禁止命令不能被批准。取消之后的审批响应拒绝。后续 turn identity 由原始用户命令唯一决定，不由界面随机重试生成。

原生进程登记采用每 run/turn 的内部目录，未更改全局进程目录或用户环境。只有原生适配器能通过结束路径确认清理；清理未知时状态为 Blocked/Unknown，不能作为可重试成功。没有真实模型、桌面 UI 或付费调用。

## sh2b 验证记录

runtime 22 项（1 项事务观察、3 项契约、6 项计划、6 项仓储、6 项 owner）覆盖通知重放/乱序/冲突/事务回滚、重复文本、usage 去重、断流缺失完成、取消与 Done 竞态、清理未知、审批失效/once 不升级/session 唯一 continuation、视图释放、启动/完成回执丢失，以及真实离线 Provider 子进程的生成和取消。storage 原有 37 项回归通过。首轮新增 migration 后发现两项验证问题：V1–V13 fixture 只排除 V14、Cargo 未跟踪 migration 目录新增文件；分别改为明确筛选 <=13 和正常 build.rs 目录依赖，原迁移/备份/恢复断言保留，修复后回归通过。

sh2b 最终复核：runtime 22 项通过，storage 37 项通过，CLI 2 项 commands + 11 项 workflow 通过；桌面和独立 CLI 的 locked 构建检查通过。隔离检查与差异空白检查通过。所有进程测试均为离线 fixture；未运行真实模型或桌面验收。

## sh2c：共享执行调度

`execution` 复用 DagScheduler 和 matching/repair/context 领域函数。串行与并行仅由冻结的全局槽位和 Provider 槽位配置区别；槽位和预算预留在派发前同事务保存，失败决策不留下部分预留。预算依据报告费用、活动预留和冻结模型估计，未知估计/实际费用依 blockUnknownCost 决定是否阻断，未知优先速度/tier 价格不沿用基础价格。规划、任务与审查都受预算检查，原生未知费用保持 null。槽位作用域为运行的 DAG；不同数据目录仍是独立服务权威，不能据此推断机器上其他进程的资源配额。

模型目录、参数 effort/fast/tier、学习观测和学习 usage profile 都可进入冻结配置。分配、估计及修复统一消费该快照，不读取隐式的前端设置或环境补丁；显式模型锁不会被重分配覆盖。Provider 健康由原生错误分类维护，熔断阻止新派发，半开只允许探测；bounded-ladder 等待冷却时仍响应取消。manual 不自动修复；有界阶梯使用同窗口修复、允许目录内的重新分配、最后阻断，未确认清理或有待对账 Git 意图时不重新调用模型。

上下文复用由既有成本/TTL/容量决策及窗口纪律共同判断。跨任务复用还核实工作树内容、清洁状态、模型/权限/参数及 write scope；不能证明连续性时记录原因并新建窗口。修复/审批 continuation 沿用最初干净任务基线，不能把失败遗留的越界文件重新当作合法基线。原生 turn、实际 thread、attempt、流式记录和用量保留独立关联，没有合成 keepalive。

`workflow` 使用独立任务 worktree，多个任务并行执行，任务结果串行集成；依赖只由已接受且集成的任务释放。原主工作区保持原目标身份，目标合并仍需最后独立确认。服务后台持有 lease，视图不负责执行循环。

## sh2d：范围、门禁、报告与 Git 对账

任务必须同时取得：原始基线、实际写入范围、structured turn 完成及清理、真实原生门禁、未变更的门禁源快照、完整模型报告、无契约偏差/缺陷，以及精确提交/集成凭据，才可 DONE。局部生成的报告不作为模型成功证据。子目录项目的 Git 根路径显式转换到项目相对作用域；越出项目子目录同样拒绝。门禁输出、失败 attempt 和工作树保留，不自动回滚用户文件或改 Git 身份。

提交与集成均先落意图，再调用既有原生实现，再确认结果。提交回执丢失复用 commit_verified 的父提交/树/消息校验；集成回执丢失复用精确双父提交和 merge-tree 对账。恢复优先处理已记录的在途集成意图，不在陈旧 integration head 上开始下一次合并。旧引擎 CLI 的“集成成功但状态未保存”窗口在服务路径可对账；此结论不代表旧 CLI 已接入。

原生 Git 增加正常 task-local 进程登记上下文，不修改桌面全局目录、HOME 或账户配置；每次启动先保存 pending 操作，再捕获 PID，完成后确认进程退出。提交、集成和交付同时检查 Git 操作状态，拒绝未完成的 merge/rebase/cherry-pick 等操作和锁文件。接管先核查作用域内 pending/进程登记；缺失 PID、活进程、损坏记录或不一致源证据明确阻断。未知模型结束和未知门禁结果不能凭 Git clean 或 lease 释放补成 PASS。新的显式 RetryTask 必须先证明清理、有效授权且不存在待对账 Git 意图。

## sh2e：最终接受及三种交付

任务全部接受后，服务自动执行独立最终门禁和只读审查。最终门禁与 reviewer 前后均绑定同一清洁候选；审查仅接受结构化 PASS 且 findings 为空。review turn、门禁、源快照、原目标身份及批准摘要构成接受凭据。用户重做审查会创建新身份并清除旧接受，取消会撤销交付摘要。

MergeDelivery、ExportPatch（显式绝对输出路径）和 KeepBranch 都需精确接受摘要，先保存意图，再执行/确认。合并复用原生精确目标事务；保留分支核实分支仍指向接受源；导出拒绝覆盖不同内容，也拒绝源工作树、原目标仓库、服务控制目录和符号链接等不安全目的地。目标 dirty/推进、候选修改、失败/写入型审查都不能交付。合并成功但 DB 确认丢失可通过 ResumeRun 对账，无重复合并或模型调用，不自动 push。

## 开发中发现并修复的问题

- 首轮并发离线矩阵发现不同仓库复用逻辑 run/task ID 会冲突于原生门禁登记；attempt 身份加入冻结配置摘要，保留命令幂等，随后并行矩阵通过。
- 读取快照原先每次重跑 parser/分配/命令策略，造成首次 30 秒外部观察超时。完整业务校验仍在命令入口，读取改为规范摘要、关联及证据不变量检查；原观察期限下正常任务链路通过，未修改产品超时或门禁。
- Git 恢复测试最初仅给整段恢复 2 秒，包含多个真实 Git 操作时发生外层超时；改为与其余阶段一致的 30 秒观测上限，保留精确提交、状态及无新模型调用的全部断言，未修改运行时超时。
- 计划审批等待曾被误记为后台基础设施错误，影响 continuation 观察；现在业务等待/拒绝保存在持久状态，worker_error 只报告基础设施或未知清理。新增 transcript DB 故障回归验证停止并阻断，不能误标为用户取消。

所有上述进程矩阵是正常原生适配链的离线协议 fixture，位于 tests/；不是付费模型、桌面 UI 或 10/20 真实验收。Windows Full Auto/缺少原生 sandbox 能力明确 Unsupported；不能表达 once 权限的 headless adapter 不扩大授权。配置在执行中冻结，影响授权的配置变更须重新规划/批准；不能用入口覆盖快照继续。App/CLI 切换、旧写路径退役、跨入口一致性/强杀矩阵和真实验收仍属于 sh3/sh4。

## sh2c 验证记录

共享 runtime 37 项通过，其中调度 5 项、原生任务执行 3 项，覆盖串并行、原子预留、未知费用、半开探测、用户锁、有界修复和真实 worktree 隔离；计划 7 项、turn owner 7 项及原有契约/仓储回归保持通过。相互依赖的任务、恢复、最终审查实现随共享执行核心一起落地，sh2d/sh2e 分别补齐独立故障与交付验收提交。native 新增策略脱敏测试通过，敏感字段不能转化为可用身份。

## sh2d 验证记录

独立 acceptance 两项原生 Git/Provider 协议矩阵通过：实际越界、失败门禁、缺少结构化完成、报告偏差都不能接受或交付；测试数据库触发器模拟提交及集成成功后的确认写入失败，ResumeRun 精确对账已有提交，无新模型调用、无重复提交/合并。保留失败工作树和证据，不改全局 Git 身份。配套 storage 37 项、orchestration 49 项回归通过；native 进程登记两项通过。

## sh2e 验证记录

独立 delivery 三项矩阵通过：三种交付方式均需精确摘要并保持命令幂等；失败/写入型审查、dirty 目标、推进目标、变更源拒绝交付；合并成功后的确认丢失可精确恢复，取消使旧接受失效。最终 runtime 共 37 项全部通过，旧 CLI 2 项命令及 11 项 workflow 回归通过。native 脱敏回归 4 项通过；桌面及独立 CLI 的 locked 构建检查通过。测试隔离和差异空白检查通过；未运行真实模型、UI 验收或暂停中的 10/20。
