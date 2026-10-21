# sh2：共享运行服务实现记录

sh2 按子节点推进；本文件记录已实现边界与未完成验收。旧 App/CLI 运行没有切换引擎，接入和退役仍属于 sh3。10/20 真实需求继续暂停。

## sh2a：计划与授权

runtime 使用既有 planning parser/validator、matching allocation 和 orchestration gate-command safety。普通 `ImportPlan` 与模型生成结果走相同规范化路径；任务按 ID 固定顺序，作用域冲突由既有 validator 串行化。计划必须含有效 DAG、范围和可运行验收命令；超出运行范围、路径穿越和不可确认的 shell 组合命令拒绝。任意 glob 包含关系不能由 overlap 推断，包含证明只接受相同 glob 或明确的目录父前缀。

模型目录来自冻结配置；任务锁优先于 executor 锁，锁定 provider/model/path 原样保留，模型输出的 assigned_model/model_params 不取得用户授权。未锁定任务复用匹配引擎，在稳定排序的可用候选中分配。未知显式模型允许保留，价格不会因此推断为零；预算预留属于 sh2c。自定义包逐个核对 SHA256，并用既有安全合并规则构建有效 pack；层内容和 override 同样包含在配置摘要中。

`ApprovePlan` 绑定规范化计划、分配、完整有效配置、工作区/目标、策略版本和用户确认命令。模型提供的 confirmedCommands 被丢弃。未确认的命令作为可编辑问题保留，但不能批准；禁止命令不会因用户确认而放行。写权限和 workspace trust 必须显式存在。`UpdateConfig` 或重新导入/编辑会清除旧批准；成功批准命令的幂等重放只返回旧 receipt，不能重新批准变化后的计划。执行开始后的编辑不开放。

`GeneratePlan` 先提交 Planning 意图，再由 `service::drive_planning` 在独立事务中持久化唯一 claim，随后调用 Planner 适配器；结果和事件原子保存。`NativePlanner` 使用正常 Provider 检测、明确 provider_path、只读原生 adapter、现有 repo digest/prompt、超时和脱敏。不改变全局环境、账号、Git 身份或原生全局进程目录；内部 ChatRequest 的进程目录不能从传输 JSON 设置。结束必须观察 structured Done、无错误且原生进程已清理，工作区快照未变化，然后才接受计划。进程存在不等于计划成功。

生成失败进入 Blocked 并保存错误。claim 后进程崩溃或完成结果写入失败时保留未知 Planning，重复 drive 被拒绝，绝不自动再付费调用。安全恢复/审批/可取消的服务生命周期将在 sh2b 完成；当前 drive 不应被入口当作可随视图销毁的任务。Planner trait 是正常适配边界，离线实现只放在 tests 中。

## 尚待实现

sh2b 的持续 turn actor、transcript/usage 持久化、审批窗口、取消与恢复；sh2c 的 DAG/预算/健康/修复/上下文；sh2d 的任务证据和 Git 事务；sh2e 的最终接受与三条交付路径尚未完成。StartExecution 及交付命令仍返回 Unsupported，不能把计划批准当作任务完成或可交付。

## sh2a 验证记录

2026-10-21：runtime 3 项基础契约、6 项计划、6 项仓储测试通过（15 项）。覆盖稳定规范化和自动分配、模型锁、普通导入/编辑、旧授权失效、模型伪确认拒绝、范围/DAG/危险命令拒绝、生成失败、完成写入故障后的禁止重复调用、原生只读 Provider 协议进程。离线进程 fixture 仅通过测试引用既有 cli/tests/fixtures/codex.py，未复制进生产入口，未调用真实模型。

桌面 workspace `cargo check --locked --lib -j 2` 与独立 CLI workspace `cargo check -j 2` 通过，两个 lockfile 固定 runtime 新依赖。`python3 scripts/check-test-isolation.py` 和 `git diff --check` 通过。未进行 UI、真实模型或交付验收；旧 App/CLI 能力回归将在入口迁移时保留。
