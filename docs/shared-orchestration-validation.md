# sh4 验证记录

本记录独立于可靠性 20 项样本；10/20 不因本迁移测试自动恢复。离线 Provider fixture、Rust 服务测试和前端 transport 单测均不计真实模型 / 正常 UI 验收。

## sh4a：共享契约和确定性（2026-10-22）

`tests/fixtures/shared-runtime-v1.json` 是独立预期：12 种 App 命令及对应 Rust / CLI wire 格式、完整有效配置、普通计划和四步状态 / 事件预期。前端测试调用真实 runtimeSubmit codec，Rust 测试解析同一 wire，公开 CLI `run submit` 在独立数据目录执行同一输入。三个入口不各写一套决策来互相证明正确。

重复提交返回相同回执且不产生额外事件；不同 commandId 的陈旧版本拒绝且不改状态；模型自带分配被冻结的用户锁替代；取消撤销批准；只读分页和完整重放一致，无 turn / Git 副作用。语义比较不包含随机 ID、时间戳、绝对临时目录或事件批次。

另两次真实原生 fixture 执行各自产生单个执行 turn、独立最终审查、通过且清理的 gate、任务提交及集成证据；任务 DONE、候选 AwaitingMerge、目标文件不变。实际集成 Git blob 为期望内容，模型调用各两次，未知费用保留 null。断言具体预期，不用两个结果相等代替正确性。

验证：runtime contract_parity 3 项、公开 CLI shared_contract 1 项、App runtimeContract 12 项、typecheck、测试隔离和差异空白检查通过。首次 CLI 测试错误地把 serde 省略的空 turns 字段当必有对象而失败；修正测试以接受契约的省略空集合表示，未改产品或业务断言。未调用真实模型。

## sh4b：真实进程离线故障矩阵（2026-10-22）

新增公开 CLI 场景使用 tests/ 中的 vendor JSONL fixture，经正式 Provider 路径、真实进程、共享 runtime、SQLite、gate 和 Git 执行，不添加产品 test mode：

- owner 强杀：只杀测试自己的 CLI owner，原生子进程继续等待。resume 在活跃进程下拒绝、retry 拒绝；释放 fixture 后只对账为 Failed / cleanupConfirmed，丢失结构化完成不被接受，调用次数仍为 1，目标不变。
- gate 中断：观察真实 gate children.json 登记后请求正常 cancel，最终进程清理确认；无 PASS、任务接受、提交或集成，目标不变。
- 审批 / 取消竞争：真实 Provider 发出不可静态解析命令的拒绝事件，两个 CLI 进程用同一 expectedRevision 竞争 Deny / CancelRun。恰一个接受事件，无续发 turn；重复同 commandId 保持幂等，新 commandId 的陈旧审批拒绝。

已有真实原生 / SQLite 故障测试继续覆盖：重复命令和事件分页（contract_parity、repository、shared_contract），连续流取消（workflow），DB / transcript 写失败（repository、turn_owner），任务提交及集成回执丢失（acceptance），三种交付及交付回执丢失（delivery、公开 CLI workflow）。Git 对账核实实际提交 / 父提交 / 树及调用次数，缺证阻断；不是仅验证返回枚举。

开发失败记录：owner 恢复测试最初把“已确认清理但仍 Blocked”的 CLI 非零误作未恢复，改为独立检查真实 Failed / cleanupConfirmed，保留 Blocked 断言。审批 fixture 最初发出静态可解析命令，被 Full Auto 正常策略允许；改用确实无法静态解析的 vendor 拒绝事件，不修改产品权限策略。测试也修正了等待审批及通用 submit 的退出码假设：已持久化的业务等待 / Deny 回执允许成功返回，不能用退出码代替状态；同 ID 的获胜 Deny 必须允许幂等重放，新 ID 的陈旧审批仍须拒绝。所有失败均为离线尝试，未调用真实模型、未放宽期限或业务断言。

sh4b 最终验证：runtime 全量 49 项通过，CLI 全量 20 项（commands 2 + shared_contract 1 + workflow 17）通过；前端全量 57 文件 / 394 项、typecheck、i18n、测试隔离和差异检查通过。测试集合串行执行，每集最多两个线程 / worker。完整首次失败、针对性重跑和最终日志归档于外部证据目录 `offline/`，附 results.json 和日志 SHA-256；不是付费或正常 UI 证据。

## sh4d：迁移与运维说明（2026-10-22）

shared-orchestration-operations.md 固定 schema-v1 的数据位置、两端配置差异、普通计划 / 历史迁移、游标观察、显式取消 / 重试、强杀 / gate / DB / Git 回执故障恢复及回退约束。兼容代码退出须先清点旧活跃运行并对账，不宣称个人数据已清零；可靠性 10/20 仍需另行冻结正常入口协议。文档与公开命令、运行时拒绝规则及 sh4a / sh4b 已通过回归对照，未执行真实数据迁移、清理或发布 / push。

## sh4c：真实尝试已执行，整项未通过（2026-10-22）

外部克隆构建的产品提交为 `22fa49bb6bf5efd7ba3da03eb7d5cf0bd79b02f5`；之后提交仅测试 / 文档，产品源码差异为空。普通 CLI debug 构建使用声明过的普通 Cargo 缓存 APFS 副本，构建产物留在外部克隆；没有性能 / 冷启动结论。未安装模型依赖或更换 HOME、认证、代理环境、Git 身份。正常探测版本：Claude 2.1.301、OpenCode 2.0.29、Codex 0.162.0；不据此改写旧可靠性 baseline。

样本是独立 Git 仓库的时长格式化函数修复，仅允许修改 `src/format-duration.mjs`。六项冻结 Node 测试在原实现全部检出失败；无第三方依赖。原需求、原测试、哈希、正常权限 / 明确命令审批及每次失败均已登记。数据目录、失败工作树、Git refs、SQLite 一致备份和 transcript 保留在外部证据目录。

四次尝试分别记录，不计四个可靠性需求：

1. 正常 Claude / `haiku` 规划遇 OAuth token revoked。CLI 元数据曾报告 `claude-haiku-5-5`，不据此声称真实推理完成；费用未知，未执行代码。
2. 正常 OpenCode / `deepseek/deepseek-flash` 完成规划与代码修改，报告适配器无 shell 因而未运行测试，服务按契约偏差阻断。正常 requested / observed 模型一致；按唯一 turn 去重，已报告费用合计 $0.00338907。离线独立检查该失败工作树的六项测试通过，但不能代替服务接受，更不能交付。
3. 正常 Codex / `gpt-6-luna` 完成规划、执行、真实 gate、任务提交和集成。独立最终审查输出 FAIL，发现大数小时数会转为科学计数法；响应还带说明前缀，严格 JSON 解析失败，运行 Blocked。requested 模型明确，observed / 费用未知，均保持 null。
4. 新注册的 `gpt-6-luna` 运行附上第三次实际审查发现，明确为操作员反馈辅助；没有提供代码 / 算法或修改旧尝试。生成计划选择 `npm run test`，冻结的 package.json 中该脚本恰为 `node --test`；正常确认此精确命令，不编辑计划或测试。任务和门禁完成，但最终审查再次输出 FAIL，指出 Number.MAX_VALUE 先浮点除法造成分钟 / 秒精度错误；同样因说明前缀无法解析严格 JSON。保持 Blocked，没有交付。两次连续相同审查 framing 失败后停止付费重复调用。

第三 / 四次候选分别在精确提交的独立 clone 执行原六项测试，均通过；测试文件和 package.json 哈希不变，候选仅目标函数发生变化。额外只读数值复核独立确认模型所报缺陷，未修改冻结测试或 gate：第四次对 Number.MAX_VALUE 给出 `40:48`，精确整秒计算应为 `14:18`。这些是有缺陷的候选，不把门禁通过误记为完整交付成功。原目标仓库全部文件 / HEAD 保持原样，没有 merge、手工接手代码或 push。

App 已用既有 prepare-desktop-test.py 从提交源码准备独立克隆、独立 app-sample、标准 appDirectoriesOverride / ephemeral WebView 配置和操作步骤；状态严格为 prepared-not-executed，未构建 / 启动 / 派发 UI 模型请求。系统外部 UI 自动化未开启（System Events UI elements enabled=false），本会话无桌面控制工具。依据 testing-isolation.md，不能改 Store / DB、注入 JS、恢复旧驱动或用 CLI 代替 UI。

因此 sh4c 保持未勾选，sh4 整体未完成。后续先修复正常登录或另行冻结合适的真实样本与停止条件，正常 UI 仍须外部操作；本轮不继续付费重试、不削弱接受协议、不恢复可靠性 10/20。

版本化摘要见 [证据登记](reliability-evidence/shared-orchestration/sh4/registration.json)、[真实结果](reliability-evidence/shared-orchestration/sh4/live-results.json)、[离线结果](reliability-evidence/shared-orchestration/sh4/offline-results.json) 与 [哈希清单](reliability-evidence/shared-orchestration/sh4/external-evidence-sha256.json)。完整本地证据：`/Users/iwpz/Documents/tokage-validation/sh4-20261022-152706`；正常 App 人工操作步骤为该目录的 APP-MANUAL.md。没有重新分类或修改历史 1–9 与原第十项。

### Claude 认证对照补查（2026-10-23）

用户反馈正常 Claude 会话可用，因而追加独立诊断，不重开 sh4c 付费矩阵。PATH 的 `/Users/iwpz/.local/bin/claude` 指向本机独立安装 2.1.301；正常 `claude auth status` 返回 loggedIn=false / authMethod=none。交互登录 zsh 解析到同一可执行文件，无 alias / function，相关认证 / 配置环境变量只检查存在性、均未设置。未读取或复制凭据。

绕过 Tokage、原生沙箱和子进程环境组装，直接调用该可执行文件，以 haiku、禁用工具、只答 OK 的最小请求复现：退出 1，is_error=true，`OAuth session expired and could not be refreshed`。该次 CLI 报告费用 0（只记录该结构化回执，不推断历史费用）。因此不是只有 Tokage 沙箱才会触发的失败，之前将沙箱差异列为优先根因的推断没有证据支持。

同时观察到运行中的 Claude 进程来自桌面应用附带的 2.1.308；单独启动该二进制查询 auth status 也返回 loggedIn=false。这表明“现有可用会话”与“新建独立 CLI 子进程”可能使用不同认证上下文，但未确认用户可用会话来源，不能将版本差异直接断言为根因，更不能断言用户所有 Claude 都不可用。原生适配器显式移除宿主会话通道变量，意图是启动独立正常登录的 CLI；本次 shell 中这些宿主变量也未设置，故不能归因于这次移除。

原始诊断输出留在外部 `/Users/iwpz/Documents/tokage-validation/claude-auth-20261023-084746`，含 summary.json 和 SHA-256。未改登录、认证、HOME、全局配置或系统权限；未升级 / 切换产品 Provider。sh4c 和正常 UI 未验收状态保持不变。

### 重新登录后的 haiku 验收与审查上下文修复（2026-10-23）

用户在新消息中确认交互请求也要求登录，并已通过正常 Claude 登录恢复。重新探测 2.1.309 为 loggedIn=true / claude.ai，先前缓存账号解释获得确认。外部提交版本 `4640990` 的新 attempt-05 保留原需求和六项测试。最小直连 haiku 请求在 90 秒外部期限内无响应而终止，费用未知；随后正常 Tokage 规划实际读取样本并输出完整计划，observedModel 为 claude-haiku-5-5。计划增加空 write_scope 的只读 testing 任务，不符合 runtime 每任务明确写范围契约，保持 Blocked / cleanupConfirmed，未执行代码；该 turn 报告费用 $0.03733552。

attempt-06 使用新运行追加流程说明：只建一个实现任务，独立验证由 Tokage 执行。记录操作员流程辅助，没有编辑计划、代码或测试。haiku 完成规划、真实执行、任务门禁与独立最终门禁、提交和集成；候选 `84e4819f5b5c91998eebece02f31282fbe2cd375` 在独立 checkout 的六项原测试及 Number.MAX_VALUE 精确数值核对均通过，仅目标函数变化。最终审查仍输出严格 FAIL：模型未获得服务已执行的 gate 回执，并误把内部规范化 literal-path/** 当作不同于原 Markdown literal-path 的权限。服务保持 Blocked，无交付。三次唯一 turn 报告费用合计 $0.10783674；原始 findings 和所有失败保留，不追认通过。

据此修复通用审查输入：从 gate 完成后的新持久快照读取真实 final gate receipts，核实覆盖全部去重命令、结果命令 / 退出码 / 成功状态、精确候选 before / after 及完整清理，再附给只读 reviewer。说明既有 scope matcher 的 canonical literal P -> P/** 语义，原 Markdown 不是额外权限。gate 输出文字仍视为不可信输入；模型仍可因代码缺陷或缺证返回 FAIL。不放宽只读工具权限、不代替模型输出 PASS、不放宽严格 JSON 或交付确认。

新增回归拒绝缺失 / 重复 / 陈旧 / 失败 / 未清理 gate 回执，并从真实原生 fixture 捕获审查 prompt，验证提供的回执与最终持久证据逐字段相同。原三种交付、FAIL 阻断、陈旧 / dirty 源和回执恢复测试保持通过。真实旧运行不被修改或改记成功，后续验收须从修复提交构建新版本并登记新 attempt。App 仍按用户决定不验收，可靠性 10/20 保持暂停。

审查上下文修复验证：runtime 全量 50 项、CLI 全量 20 项通过，桌面 locked lib check、测试隔离与 diff 检查通过；交付专项 3 项独立通过。前端未变。原权限、gate、范围、FAIL 和精确交付阻断断言保留。
