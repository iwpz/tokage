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

### 新提交版本的 haiku CLI 全链路通过（2026-10-23）

attempt-07 从修复提交 `362f5ad648910591bf0757db30a493b8edd1f5f8` 的独立克隆 locked 构建。正常 Claude 2.1.309 / haiku，三次原生 turn 均报告实际模型 claude-haiku-5-5，全部 Completed 且 cleanupConfirmed。继续原冻结六项测试、同一功能要求，并保留 attempt-06 的单任务流程说明；没有沿用旧接受状态、编辑生成计划或由操作员修改代码。该结果是经过失败诊断、流程辅助和产品修复后的通过，不称首次自主成功。

经公开 CLI create → update-config（限制唯一目标文件）→ plan → approve（精确确认 node --test / npm run test、本会话 Full Auto）→ execute：任务 DONE、真实任务 gate 和候选最终 gate 完成、服务提交与集成完成、独立只读模型审查返回严格 `{"verdict":"PASS","findings":[]}`，状态 AwaitingMerge。没有放宽 reviewer 的工具权限或忽略 FAIL。

候选 `de77fa1aa85beaff6a104c54a0fddc1df7eef264` 在独立 checkout 执行六项原测试通过；test 与 package.json 哈希保持冻结值，差异仅 src/format-duration.mjs。额外只读数值核对涵盖 Number.MAX_VALUE、1e300、1e30、MAX_SAFE_INTEGER 及普通值，均等于精确整秒 / 商余数预期；不向冻结 gate 添加断言，不反馈算法或修改候选。

随后用正常 `run merge --source ... --target ... --confirm` 将候选交付到该 attempt 的外部 sample 仓库。持久状态 Delivered、交付意图 completed=true；实际合并提交 `02c42abec2e9b3aaf0054e0525328d7bea85e7eb` 的父提交严格为原目标 `a3df3f1474fd6025a1ed5912f3a3e3ef0a30992d` 与候选，合并 tree 与候选 tree 相同。交付后六项原测试再次通过，工作区干净、测试与配置未变。不修改 Tokage 主仓库来完成样本，不手工制造状态，不 push。

该成功运行三次唯一 turn 报告费用：规划 $0.02843977、执行 $0.02921230、最终审查 $0.02811772，合计 $0.08576979。此为该运行结构化费用回执；先前最小直连超时费用仍未知，不将其计为零或把本数称为全部历史费用。

追加摘要见 [重新登录后结果](reliability-evidence/shared-orchestration/sh4/reauth-live-results.json) 和 [新证据哈希](reliability-evidence/shared-orchestration/sh4/reauth-external-sha256.json)。新 attempts 5–7 的命令、stdout/stderr、游标重放、transcript、SQLite 一致备份、候选 / 交付复核及修复回归日志保留在既有外部证据目录，追加 100 个文件哈希；原 attempts 1–4 及其清单不重写。

结论：sh4c 的真实 CLI 部分通过，App 正常 UI 仍按用户明确决定本轮不验收；sh4c 整项和 sh4 整体继续不标完成。可靠性 10/20 仍暂停，历史样本不改分类。

## sh4c：App 人工 UI 验收发现展示回退（2026-10-23）

用户恢复正常 UI 验收。外部提交 f76ec0a 的标准隔离 release 构建已成功启动；构建安装先因 Node 20 引擎限制缺失 Rolldown 原生依赖失败，保留两次失败日志后，以既有 Node 22.16.0 重新 locked npm ci 完成构建，未修改产品源码或依赖锁。使用标准 appDirectoriesOverride 与 ephemeral WebView，不证明重启前端设置持久性，CLI HOME 仍为正常用户。

用户通过正常设置关闭 Debug Provider、选择先规划 / Claude haiku、worktree 和并发 1，再提交冻结需求及预先登记的单任务流程说明。真实 planner 为 claude-haiku-5-5，费用 $0.03211741，单任务计划进入 AwaitingApproval，尚无执行或交付。用户截图证明 Chat 被原始 RuntimeView 取代，文本流片段被插入换行，工具事件 JSON 外露，输入框下方多出导入按钮。此项明确为 UI 失败，不能以规划成功代替通过。[现场摘要与外部哈希](reliability-evidence/shared-orchestration/sh4/app-ui-regression.json) 保留来源及费用。Reasonix 由用户正常点击验证 API Key 后显示可用；不将 key 存在直接改判为已验证。

sh4c-ui 修复：聊天视图重新复用既有 UserBubble / AssistantReply、Markdown、思考折叠与工具 / 文件展示；只读 transcript 投影按原始顺序连接连续文本片段，不将 thread、model、工具事件 dump 到回答，也不接入旧聊天审批 / 调度。命令审批继续经共享 runtime 的正常显式确认。Chat / Plan / Pipeline 恢复各自展示，计划 Markdown 阅读与折叠编辑分开；策略编辑收起，任务卡恢复限宽和图标。导入入口移入新需求输入工具栏的图标，运行列表入口移入页签栏。状态补齐中英文翻译。

验证：新增 6 项针对分片拼接、元数据隐藏、真实工具展示、审批边界、费用未知、页签及 Markdown 的回归；全量前端 400 项、typecheck、双语检查、生产构建、隔离与差异检查通过。此为自动化展示回归，修复版正常桌面 UI 尚待用户复验；旧窗口和原始失败证据保留，不自动重启、不重派模型、不改记成功。sh4c / sh4 整项仍未完成，可靠性 10/20 仍暂停。

修复提交 `4483aae` 已在新的外部克隆以同一标准隔离配置完成普通 release 原生构建（3m06s），未启用验收 feature；构建前预装锁定依赖，仅构建时使用 Node 22 与既有外部 Cargo 缓存。修复二进制已单独保存，旧实例仍运行，等待用户正常退出后再打开，未并发访问验收运行、未新派发模型。[修复构建记录](reliability-evidence/shared-orchestration/sh4/app-ui-repair-build.json) 含源码提交、构建日志和二进制哈希；正常桌面样式复验仍未通过，不据此勾选 sh4c。

## sh4c-ui2：补齐结构化视图与 Session 数据源（2026-10-23）

用户对 4483aae 修复版的正常 UI 复验仍不通过：聊天消息的 Markdown 已恢复，但缺少任务板；Plan 仍以整份 Markdown 代替原结构化布局，Pipeline 缺少卡片 / 表格 / 详情交互，Session 查询旧历史表而不显示共享运行的模型消息。三张用户截图归档到同一外部证据目录，原始失败结论不改写。[第二轮 UI 观察](reliability-evidence/shared-orchestration/sh4/app-ui-structured-regression.json) 包含哈希。

本次将共享运行的四个页签接到完整展示层：Chat 增加结构化任务板及展开验收目标；Plan 直接复用原 SpecEditor、ContractsEditor、TasksTable，恢复子页、布局和编辑，保存经既有 native renderer 与 ImportPlan / AmendRun 重新校验，Markdown Spec 编辑也保留原合同和任务。任务模型仍来自冻结配置，经正常 UpdateConfig / AmendRun 显式重审批。未保存草稿阻断审批与执行，精确命令未勾选时禁用审批按钮。运行快照没有分配成功率 / 费用估计时，不凭空填入数值。

Pipeline 恢复卡片 / 表格切换、进度、模型图标、依赖 / 范围、任务详情、合同、验收命令与回执，按真实状态展示 runtime 支持的重试、取消、核对、审批、接手和交付操作。重试只提交 RetryTask，不由 UI 改任务状态；接手仍使用原有检查与新门禁。Session 复用原厂商 / 模型树、搜索、自动跟随和消息浏览器，通过可替换的只读数据源读取 runtime turn / transcript；项目范围合并普通历史，未知创建时间 / 上下文仍显示未知，不制造旧 attempt 行、执行者 prompt 或模型用量。离开 / 返回页面不派发模型。

验证：全量前端 61 个文件、411 项测试通过；新增 / 更新回归覆盖四个页面展示、7 项真实 DOM 控件交互、记录恢复、原文分片、保存命令、审批勾选、任务重试不改本地权威状态和 Markdown Spec 保存。typecheck、i18n、隔离与差异检查通过。首次交互测试的 Tauri mock 缺少 isTauri、happy-dom 未提供 confirm、测试误点费用未知复选框，均为测试准备错误，修正后完整回归通过，未更改生产权限或门禁。桌面正常 UI 尚待新构建复验；sh4c / sh4 不勾选，可靠性 10/20 不恢复，修复期间不新增模型调用。

sh4c-ui2-order 收尾：运行快照的 turn map 按 UUID 排序，不能充当聊天顺序。投影现从持久事件首次出现的 turnId 记录顺序，Chat 与 Session 共用此顺序；刷新 / 重放不派发命令。补充两项回归后，全量前端 413 项与 typecheck 通过。b17c149 的外部普通 release 已构建成功（3m32s），尚未启动；消息顺序补丁将包含在最终重新构建中，再进行正常 UI 复验。

最终源码 `8d2989a` 的普通隔离 release 已重新构建成功（增量 1m43s），并在确认旧独立实例退出后启动，未操作原开发版。沿用同一隔离数据只读核对：原运行仍为 revision 316 / AwaitingApproval / 一个 planner turn；未重发需求或执行。归档 [最终构建](reliability-evidence/shared-orchestration/sh4/app-structured-repair-ui2-final-build.json) 与 [启动记录](reliability-evidence/shared-orchestration/sh4/app-structured-ui2-app-launch.json)。新窗口视觉与交互正常 UI 观察仍待用户复验，因此未完成 sh4c。

## sh4c-ui3：布局空间与真实 WebKit 回归（2026-10-23）

用户对 8d2989a 的正常 UI 反馈 Plan / Pipeline 仍不通过，原截图保留到外部 `ui3-user-plan.png`、`ui3-user-pipeline.png`。本次将逐项命令确认集中到“审阅并批准”正常弹窗，最终按钮仍须全部勾选且保留原 Full Auto 显式确认；策略、补充和接手表单移入“执行设置”弹窗，不再挤占任务板。Plan 工具栏紧凑化，字段组在有明确尺寸的 flex 主体内定位，避免内在尺寸干扰；保存按钮窄列显示带完整可访问名称的图标。Spec / Contracts 工具栏可换行，窄合同列改为水平合同标签，任务内容保持独立滚动。执行和所有门禁仍由原共享 runtime 判断。

新增 `tests/visual` 独立组件布局回归：只通过测试 Vite 配置使用 fixture transport，所有派发直接报错，不读 App 数据、不开真实 App、不注入产品页面、不调用模型、不点击最终授权按钮。Playwright 及 WebKit / FFmpeg 安装在外部 visual-tools，浏览器缓存也在该目录；PLAYWRIGHT_BROWSERS_PATH 仅用于离线浏览器，产品运行环境未改变。最终测试服务器使用 OS 临时目录的独立 Vite 缓存，与产品开发服务器分开；初始离线尝试使用默认依赖缓存，最终独立缓存重跑通过。

同一离线 fixture 对照旧提交 8d2989a：420×480 内容列的 Edit spec 按钮 x390.17 + width67.73 超过 420 边界；640×480 打开 Pipeline 策略面板后任务标题 y597，父视口底为480，可见高度为0。新版验证 1280×820、1024×700、640×480、420×480 共8个 WebKit 场景：Spec / Contracts / Tasks 内容和滚动、编辑按钮边界、Plan 主体高度、文档溢出、首屏任务卡、设置弹窗不推移任务板、逐项授权门禁均通过；截图及几何结果归档。新增420场景最初发现合同预览仍被挤出，保留该失败后调整合同标签与窄列保存按钮再通过。

全量前端413项、typecheck、i18n、隔离及差异检查通过。DOM 测试不能计算真实布局，故此前的413项不追认为视觉通过；本次 WebKit 结果仍仅是离线组件证据，不能代替正常桌面 external-ui-v1。参见[本轮原始观察、基线与布局结果](reliability-evidence/shared-orchestration/sh4/app-ui-layout-regression.json)。新桌面构建及用户正常 UI 复验继续进行，sh4c仍不勾选，修复期间无新增付费模型调用。

布局修复提交 `2c46ad5` 已完成外部标准隔离 release 构建（2m11s），包含上述 WebKit 已验证的同一源码；[构建记录](reliability-evidence/shared-orchestration/sh4/app-ui-layout-build.json) 包含二进制和日志哈希。新包已准备，旧独立验收实例仍在运行，等待用户正常退出后再启动同数据的新包。未强制重启原开发版，也未重发需求 / 执行模型；正常桌面视觉结论仍为待复验。

### sh4c-ui4 — controls parity follow-up

The user paused execution after finding native blue checkboxes and inconsistent execution controls. Replaced runtime approval checkboxes with accessible native inputs styled using application theme tokens, used the existing Switch for unknown-cost policy, restored compact icon actions and primary start/merge emphasis, and made execution settings an accessible icon affordance. Native commands and explicit authorization remain unchanged. The root cause and surface/state inventory, including unverified desktop states, are recorded in [the parity audit](shared-runtime-ui-parity.md).

413 component/unit regressions, typecheck, i18n, isolation and 18 offline WebKit scenarios passed. The browser matrix includes the four existing Plan/Pipeline widths, unchecked/checked approval screenshots and five execution states in both themes, with real computed checkbox appearance/accent and action-size assertions. Evidence: `app-ui-controls-regression.json`. These are offline UI regressions, not real App execution/merge evidence. sh4c remains incomplete; no new model calls were made.

### sh4c-ui5 — restore toolbar and chat navigation structure

The user rejected ui4's desktop preview for its duplicate pipeline headings/background strip and temporary chat status card. Restore the original Conversation board navigation and Pipeline control-bar presentation patterns: one integrated pipeline toolbar, selected Cards/Table controls, Back to review, and icon navigation within the chat task board. Remove the duplicated status card. Native runtime command/authorization behavior is unchanged. The parity audit records this previous omission explicitly.

414 tests and 22 offline WebKit scenarios pass, now including Chat at all four widths. Reviewed dark Chat and pipeline screenshots; real desktop acceptance is still pending. See `app-ui-structure-regression.json`. No new model requests or execution were started.

### sh4c-ui6 — shared bottom action panel

Restore the original PlanReview Bottom Floating Actions layout through one RuntimeActionPanel used by both Plan and Pipeline. Task/contract summary and state sit at left; native run, settings and delivery actions sit at right. Remove run actions from top headers. Content scrolls independently above the fixed flex footer; narrow contract padding is reduced without changing wide layout. Authorization/command handlers remain unchanged.

415 tests and 32 offline WebKit scenarios pass, including footer placement at all four widths and both pages' five run states in both themes. Preserve the earlier narrow contract clipping failure and its repair. Evidence: `app-ui-bottom-actions-regression.json`. Desktop preview and sh4c full-chain acceptance remain pending; no new model calls.

### sh4c-ui7 — blocked-run diagnostics and task details

Real App execution under ui6 reached Blocked revision 354 after P0.M1a completed and both independent test commands passed. The requested Luna executor had null actual cost; blockUnknownCost remained enabled. Scheduler reserve-ready checks reject unknown actual turn cost even after the last task completes, so no final reviewer was dispatched. The old error handler attached failures only to unfinished tasks, losing this run-level reason. Preserve the original snapshot, screenshot and failed/assisted attempt; this is not completed desktop delivery.

Shared snapshots now durably retain optional runError on execution/review/delivery failures and clear it on leaving Blocked. Older snapshots remain readable. App Plan/Pipeline show a visible reason, localized missing-cost explanation and expandable raw technical details when recorded; older records display existing policy/cost evidence. Include previously omitted delivery/planning/work errors, and do not hide unrelated errors behind cost diagnostics. No cost policy, allocation, test gate or authorization was relaxed. The original PipelineView uses a right-side details panel; keep it and remove the duplicate detail-header status badge.

418 frontend regressions, full tokage-runtime regressions, the focused SQLite persistence/old-schema/retry test, 32 offline WebKit scenarios and two final details scenarios passed. Evidence: `app-blocked-reason-regression.json`. Desktop build/observation and sh4c full delivery remain pending. No new real-model call or retry was made during repair.

### sh4c-ui8 — chat Contracts hierarchy and streaming disclosure

Stable Markdown renderer identities prevent streamed text updates from remounting disclosure state. Explicit Contracts/C1/C2 document sections form sibling contract cards with their own full content; source blocks inside preserve copying without an extra collapsed contract layer. 421 frontend tests and three external offline WebKit stream scenarios pass. Desktop observation and full sh4c acceptance remain pending; no real model calls or retries.


## sh4c-desktop-delivery：后续优先工作与登录阻断（2026-10-23）

计划展示专项已完成真实 macOS UI 验收：结构卡片、流式局部 loading、展开状态、正常取消和完整历史恢复，见 [计划 UI 记录](planning-ui-acceptance.md)。下一优先项是尚未通过的桌面完整链路：计划审阅 → 精确授权 → 执行 / gate → 独立审查 → 源 / 目标确认交付。第十项保持暂停，不混入可靠性样本。

已在外部创建独立 duration-project，六项原始 Node 内置测试冻结并记录哈希，缺陷基线测试失败，只允许 src/format-duration.mjs 改动。复用已提交 ea55992 的正常隔离 release，经正常添加项目、workspace / Claude 授权、Plan first 和 Haiku 选择后派发一次。保留此前任务格式失败所得的格式澄清，不提供实现算法。

本轮运行 79e0e146-9415-4efb-b7a5-01ff62b5d26f 停在 Blocked / revision 8：真实 CLI 报 OAuth token revoked，实际报告模型 claude-haiku-4-5-20251001，费用未知，turn Failed / ended / cleanupConfirmed。没有生成计划、执行任务、批准或交付，样例工作区仍干净。正常 Pipeline 显示具体登录错误及 Prepare explicit retry；未通过重新派发掩盖失败，也未修改凭据或费用门禁。继续前需要使用者恢复本机正常 Claude 登录；恢复后通过正常 UI 显式重试再完成其余链路。

证据：独立目录 /Users/iwpz/Documents/tokage-validation/sh4-desktop-delivery-20261023，包含冻结需求 / 基线、派发前模型截图、失败截图、只读 DB 备份和哈希；[机器可读结果](reliability-evidence/shared-orchestration/sh4/desktop-delivery-20261023.json)。沿用标准 Tauri app-data / incognito 隔离，不宣称 CLI HOME 或跨重启设置隔离。

### sh4c-ui-planner-retry（2026-10-23）

用户报告恢复正常 Claude 登录后，原规划失败运行点击 Prepare explicit retry 暴露产品 UI 缺陷：尚无有效 plan / confirmationDigest，却发送了执行重试命令，Tauri 正确拒绝缺少 confirmation_digest。此次没有模型派发或状态变更，原登录失败保留。修复为仅有有效计划摘要时显示执行重试；初次规划失败继续使用正常 Generate plan，它由 runtime 校验清理及状态条件，不改门禁或凭据。

新增 UI 回归覆盖无计划时仅允许重新规划，以及有摘要时仍提交原严格 PrepareRetry。431 项前端测试（两个 worker）、类型和隔离检查通过；正常桌面修复复验随后进行。原始按钮错误证据位于外部 attempt-2-reauth/01-retry-missing-digest-*。
