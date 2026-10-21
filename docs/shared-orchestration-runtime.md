# sh2：共享运行服务实现记录

sh2 按子节点推进；本文件记录已实现边界与未完成验收。旧 App/CLI 运行没有切换引擎，接入和退役仍属于 sh3。10/20 真实需求继续暂停。

## sh2a：计划与授权

runtime 使用既有 planning parser/validator、matching allocation 和 orchestration gate-command safety。普通 `ImportPlan` 与模型生成结果走相同规范化路径；任务按 ID 固定顺序，作用域冲突由既有 validator 串行化。计划必须含有效 DAG、范围和可运行验收命令；超出运行范围、路径穿越和不可确认的 shell 组合命令拒绝。任意 glob 包含关系不能由 overlap 推断，包含证明只接受相同 glob 或明确的目录父前缀。

模型目录来自冻结配置；任务锁优先于 executor 锁，锁定 provider/model/path 原样保留，模型输出的 assigned_model/model_params 不取得用户授权。未锁定任务复用匹配引擎，在稳定排序的可用候选中分配。未知显式模型允许保留，价格不会因此推断为零；预算预留属于 sh2c。自定义包逐个核对 SHA256，并用既有安全合并规则构建有效 pack；层内容和 override 同样包含在配置摘要中。

`ApprovePlan` 绑定规范化计划、分配、完整有效配置、工作区/目标、策略版本和用户确认命令。模型提供的 confirmedCommands 被丢弃。未确认的命令作为可编辑问题保留，但不能批准；禁止命令不会因用户确认而放行。写权限和 workspace trust 必须显式存在。`UpdateConfig` 或重新导入/编辑会清除旧批准；成功批准命令的幂等重放只返回旧 receipt，不能重新批准变化后的计划。执行开始后的编辑不开放。

`GeneratePlan` 先提交 Planning 意图，再由 `owned::Runtime::submit` 在独立事务中持久化唯一 claim，随后调用 Planner 适配器；结果和事件原子保存。`NativePlanner` 使用正常 Provider 检测、明确 provider_path、只读原生 adapter、现有 repo digest/prompt、超时和脱敏。不改变全局环境、账号、Git 身份或原生全局进程目录；内部 ChatRequest 的进程目录不能从传输 JSON 设置。结束必须观察 structured Done、无错误且原生进程已清理，工作区快照未变化，然后才接受计划。进程存在不等于计划成功。

生成失败进入 Blocked 并保存错误。claim 后进程崩溃或完成结果写入失败时保留未知 Planning，重复 drive 被拒绝，绝不自动再付费调用。sh2b 将此路径统一为服务持有的后台 worker；不再公开 sh2a 的直接 drive 路径。Planner trait 是正常适配边界，离线实现只放在 tests 中。

## 尚待实现

sh2c 的 DAG/预算/健康/修复/上下文；sh2d 的任务证据和 Git 事务；sh2e 的最终接受与三条交付路径尚未完成。StartExecution 及交付命令仍返回 Unsupported，不能把计划批准当作任务完成或可交付。

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
