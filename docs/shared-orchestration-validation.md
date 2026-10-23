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

### sh4c-review-output-contract（2026-10-23）

正常 UI 重新扫描确认 Claude Ready；旧登录失败轮次费用保持未知，原运行 revision 10 按既有策略阻断，未续发模型。按环境恢复隔离规则，新会话 ee1c9327-280e-4743-a855-e7c6a5f140b5 使用相同冻结需求。Planner / task / reviewer 通过正常 UI 设为 haiku（真实报告 claude-haiku-4-5-20251001），blockUnknownCost 保持 true，精确勾选 node --test 后批准。任务 DONE、任务 gate 和最终 gate 通过；独立克隆候选 8d1bd7d640d636415b526f4e580212786542267c 验证原六项测试通过、冻结文件哈希不变、仅 src/format-duration.mjs 改变、目标仍为原始提交且干净。

最终 reviewer 返回 fenced JSON PASS/[] 后追加 Verification summary，严格解析拒绝为 expected value at line 1 column 1；不截取 PASS、不自动交付。原始输出、revision 438 失败快照和截图完整保留。现在审查输入结束后明确重复单一 JSON 输出协议，禁止围栏和附加说明，保留发现问题必须 FAIL 的要求；解析失败报告明确的输出格式诊断。原来支持的完整纯 JSON / 单一完整 fenced JSON 不变，任何附加说明、FAIL、非空 findings、缺字段继续拒绝。

验证：runtime 库 3 项与 delivery 真实离线进程 3 项通过，保留所有失败审查、候选变化、dirty target、过期确认及回执恢复门禁；新增格式拒绝回归。类型 / 前端 431 项已在前一 UI 修复验证，i18n、隔离及差异检查通过。待新提交外部构建后通过正常 UI 重跑最终审查；原候选不编辑、原失败不重写。

### sh4c-native-review-result：真实失败与原生结构化修复（2026-10-23）

登录恢复后的真实运行已覆盖正常 UI 规划、精确命令批准、隔离执行、任务提交 / 集成、最终 gate 和计划修订。首次候选虽然原六项测试通过，外部精确数值复核发现 1e30 输出科学计数法和错误余数。正常 Plan → Tasks 新增依赖任务 T2、锁定 haiku、保存修订并重新批准后实际执行，第二候选 45a1cd0ff0313e4e82cef4c269121088c03aa6cf 仍未满足大数精度，并引入 -0:00:00。原六项测试始终不变；外部观察者没有编辑候选源码。此过程明确为人工提供真实 finding 并通过正常 UI 增补计划的辅助修复，不称首次自主成功。

两次真实 reviewer 都在普通说明文字中输出 PASS 加 JSON；仅加严提示词不足以修复传输问题，第二次仍因格式不符被拒绝。全部五个恢复后 turn 均结束并确认清理，实际报告模型 claude-haiku-4-5-20251001，CLI 报告已知费用合计 $0.3296813；此前认证失败费用仍为未知。没有确认交付、合并、push 或恢复第十项。

现在最终 Claude delivery reviewer 改用服务持有的 JSON Schema（用户 IPC 不能注入），以 --output-format json / --json-schema 请求原生 structured_output。依据安装的 Claude Code 2.1.288 --help 及 [官方 headless 文档](https://code.claude.com/docs/en/headless)。普通说明文本保留展示，原生结构化结果以独立 structuredOutput 事件持久化，执行 / 规划流式路径不变。只从当前成功结束并清理的 review turn 持久事件中接受恰好一个原生结果；缺失、重复、失败终态、FAIL、非空 findings、额外字段都拒绝。不从说明文字截取 PASS，不把 unknown 成本变零，不修改权限 / scope / 原测试。结果中的 session_id 和唯一 modelUsage 身份照实记录，不能确定的模型身份保持未知。结构化字符串沿用凭据脱敏，Chat / Session 可回看原生结果；旧版读取器不认识新增事件，不应盲目写入这些新记录。

验证：前端 432 项、类型 / i18n / 隔离检查通过；native 全量 322 项通过（28 个既有 ignored），最终新增差异的 Claude 7 项和结构化脱敏单项另行通过；runtime 库 4 项、delivery 真实离线进程 4 项、公开 CLI 20 项通过。新增离线 Claude 进程实际核对调用参数 / schema，并分别返回 PASS、FAIL、缺失结果，验证只有严格原生 PASS 进入 AwaitingMerge，目标均未改动；普通 prose PASS 不能覆盖 native FAIL。离线结果不能代替真实模型复验。

按 docs/testing-isolation.md“连续两次同因失败停止重复付费调用”，本轮停止新增模型请求。已从正常 UI 新增 T3，保留原两个 DONE 任务不变，加入原 node --test 及精确 1e30 / -0 断言命令；这两条命令在当前候选上独立运行均退出 1，计划经正常 runtime 修订校验保存，T3 锁定 haiku，停在 AwaitingApproval（未批准 / 未执行）。下一轮应在原生结构化新版上批准这些精确命令，修复到边界全部通过后再审查 / 确认交付；当前 sh4c 仍未完成。详见 [本次续验结果](reliability-evidence/shared-orchestration/sh4/desktop-delivery-continued-20261023.json) 及外部 attempt-2-reauth / attempt-3-review-format 目录。

原生结构化修复提交 4541da3 的独立正常 release app 构建成功（原生 2m58s），新版已通过正常启动 / 已有信任确认加载原会话；Plan → Tasks 展开 T3 后完整显示三条精确验收命令、单文件范围及 haiku 模型。只观察恢复，不批准、不启动模型；turn 数仍为 5，P0.M1 / T2 仍为 DONE、T3 PENDING、运行 AwaitingApproval。机器可读构建 / 二进制哈希与截图见 [新版桌面准备记录](reliability-evidence/shared-orchestration/sh4/desktop-native-schema-build-20261023.json)。这证明构建和计划恢复，不证明 Claude 原生 schema 的真实模型复验或交付通过。新增记录哈希为外部 continued-SHA256.json，原始失败 manifests 不覆写。


### sh4c-desktop-delivered：原生审查实测与精确交付（2026-10-23）

用户明确要求继续验证后，在已登记外部 release 4541da3 上新建 attempt-4-native-schema-live，未更换环境或凭据。正常 UI 审阅并勾选三条精确命令，T3 锁定 haiku 后执行；任务和最终 gate 全通过。真实最终 reviewer 首次返回独立 structuredOutput PASS / []，当前成功 turn 的恰好一个结构化结果持久化，模型 claude-haiku-4-5-20251001、session_id、用量及清理状态齐全，进入 AwaitingMerge。原生 JSON Schema 修复由此完成真实验证，不再只是离线结论。

候选 6e0a158e33b1ce9b168fd0a7886b62322cf1c085 虽通过原六项及 24 个外部边界，进一步独立检查发现 1000000000000000.5 / 1000000000000000.1 为合法有限输入，却因直接 BigInt(ms) 抛 RangeError。reviewer 也遗漏了该问题。未合并该候选；正常 UI Cancel run 到 Stopped，再新增依赖 T3 的 T4 finding 和精确小数断言，保留此前三条命令，锁定 haiku 并重新批准。外部观察者始终没有编辑候选源码，这次补充属于辅助，不算自主首次成功。

T4 完成后候选 75a6cab8b110ac5407bab998b57f5073b0b1fa5a 通过四条产品 gate；独立克隆验证原六项测试、24 个原边界以及跨数量级 / 小数的 1241 个数值用例全部通过。仅 src/format-duration.mjs 改变，README 和原测试内容 / 哈希不变。第二次真实原生 reviewer 仍返回恰好一个 PASS / []，Completed / ended / cleanupConfirmed，全程为 haiku 实际模型 claude-haiku-4-5-20251001。

正常 Pipeline 明示源 / 目标后点击 Merge into target checkout；界面与只读 SQLite 均为 Delivered / revision 1411。目标提交 a96e573ff3467a7d512b1dfea155fa0a7ff1b8d8 的父提交为冻结基线 66eba51067e3df4aacc50c19fd8e2436c5aab06e 和精确候选，内容树一致，工作区干净。目标上重新执行原测试与两组独立边界全部通过。没有 push，没有恢复第十项。

恢复登录后的运行累计 9 个真实 turn，全结束并清理，CLI 报告费用合计 $0.725696；此前原认证失败轮次费用仍未知，不能计零或当账单。标准 Tauri app-data / incognito 覆盖及 CLI HOME 未隔离的限制不变；不宣称原发布包逐字节验收、跨重启前端设置持久性或普遍自主可靠性。此前失败目录和 manifests 保留，新证据单独归档并哈希。sh4c 此次按“辅助后真实桌面交付”完成，审查质量的漏检明确保留。

证据：[机器可读交付记录](reliability-evidence/shared-orchestration/sh4/desktop-delivered-20261023.json)，外部 attempt-4-native-schema-live 包含正常 UI 审批 / 待合并 / Delivered 截图、原生 review 事件、精确候选独立检查、目标复验、SQLite 一致性备份与 SHA256.json。


### sh4c-ui-delivery-evidence：最终审查证据卡片（2026-10-23）

已交付样例的 Pipeline 原先只有任务结果和候选箭头，最终审查实际模型、费用及接受结果需到历史或外部证据查找。现在新增默认收起的只读证据卡片：Reviewing 时标题局部 loading；展开展示当前 delivery.reviewId 精确绑定的 observedModel、费用（未知不计零）、清理状态、候选 / 原目标、已保存审查对象及可用交付产物。缺失当前 reviewer 时不会借用历史其他 reviewer，不从说明文本或终态推断 PASS。服务、权限、审查和交付门禁不变。

验证：前端 64 文件 / 435 项测试通过，typecheck、i18n、生产前端构建、隔离与差异检查通过。新增回归覆盖当前轮次绑定、未知费用 / 缺失结果、审查 loading 与默认收起。此节点仅完成前端实现和回归；新增卡片的原生桌面视觉复验仍待进行，不能沿用旧版 Delivered 截图声称新卡片已实测。无需为此重新调用模型或重新交付。


### sh4c-ui-delivery-evidence-desktop：卡片桌面观察与恢复（2026-10-24）

已提交 a8087e0 在既有外部 checkout 正常 release 构建成功（原生 2m07s），沿用标准 Tauri identifier / app-data / incognito 隔离配置。没有新依赖安装、模型请求、计划修订或重复交付。启动及重启通过正常 Done 保留既有 workspace / CLI 授权选择，不增加权限。

正常 Pipeline 中卡片默认收起；展开显示当前 reviewId 对应的实际模型 claude-haiku-4-5-20251001、费用 $0.052273（界面六位小数）、清理 Confirmed、候选 75a6cab8b110ac5407bab998b57f5073b0b1fa5a、原目标 66eba51067e3df4aacc50c19fd8e2436c5aab06e 和已保存 PASS / []。1280×820 正常窗口内展开内容和底栏可见，任务看板独立滚动区域随内容收缩。Session 仍显示全部 9 个历史 turn，最终 reviewer 记录含独立原生结果。切换至原认证失败会话，其没有 delivery，因此不展示虚假的审查卡片；返回 Delivered 后恢复卡片。正常退出、重启、重新进入 Pipeline，卡片默认收起，再展开内容完全一致。

只读 SQLite 前后完整 snapshot 相等：Delivered / revision 1411、9 turns 不变；目标 HEAD 仍为 a96e573ff3467a7d512b1dfea155fa0a7ff1b8d8 且干净，新增模型调用为 0。真实记录没有 Reviewing 或当前 reviewer 缺失 / 未知费用场景，也没有 artifact（该 Merge 记录为 null），这些分支仅保留组件回归覆盖，不称真实桌面实测；没有注入状态或额外付费制造场景。标准 incognito 的前端设置持久性和 CLI HOME 隔离限制不变。本轮没有发现需修复的问题。

证据：[桌面卡片验收记录](reliability-evidence/shared-orchestration/sh4/desktop-delivery-card-20261024.json)。外部 delivery-card-ui 目录保存登记、构建日志、6 组截图 / 可访问性记录、前后快照、SQLite 备份及独立 SHA256 清单。现有 435 项前端全量、typecheck / i18n / build 结果仍见实施节点；本轮卡片 3 项回归及隔离 / 差异检查通过。未把旧 Delivered 图追认为新卡片证据。


### pr1-executor-phase：并发中断验收与执行阶段修复（2026-10-24）

新独立 invoice-project 使用冻结原测试和三模块 DAG：P0.A / P0.B 独立，P0.C 依赖两者；真实 Haiku 生成计划，正常 UI 锁定三任务 / reviewer 为 haiku，并发 2、执行预算 $3、manual repair。规划初次沿用原设置的空预算，随后在执行前正常 UI 设为 $3，不追认规划也受该预算限制。外部登记 / 基线 / 失败证据位于 /Users/iwpz/Documents/tokage-validation/parallel-recovery-20261024。第十项仍暂停。

运行 c7acced1-e6d5-41d0-9a8b-c59424a0c2e1 首次 A / B 同时 Running，实际 claude-haiku-4-5-20251001、不同原生 thread / worktree，C Pending。正常 Cmd-Q 后重启，turn 数仍为 3，无自动模型请求；两个旧 PID 和进程组均已不存在。正常 Reconcile interrupted run 把原两次转 Failed / ended / cleanupConfirmed，错误为 native process idle; lost completion cannot be accepted，费用仍 null。通过正常 UI 明确把未知费用阻断改为 off、保留预算并重新批准后，A / B 各新增一次尝试；这是显式辅助，不能称默认未知费用策略下自动恢复。预算不代表未知调用的完整实际费用上限。

恢复后的 A / B 均只输出等待批准的实现计划，源码未改；真实门禁分别失败，C 未启动，目标仍基线且干净。调查 prompt 发现生产 workflow 在 Task Brief 之后直接追加原始需求，其中含 Plan first / do not execute before approval，未明确当前阶段已获批。两个真实轮次均受此冲突影响。停止旧源码的重复付费调用，保留所有失败。

修复仅在现有 active 授权检查之后，把原需求标为历史上下文并 JSON 编码保留，末尾明确当前执行阶段、准确 approvedDigest / task ID：只执行已批准任务，早先等待规划批准指令已完成，功能 / 范围 / 冻结测试约束继续适用；新的 CLI 审批仍走正常通道，不允许绕过权限。没有识别样例、里程碑或模型，不更改派发、范围、门禁、报告验收或费用逻辑。

新增真实离线协议进程回归先失败（缺少当前执行阶段），修复后通过；覆盖没有 StartExecution 时无调用、过期摘要拒绝且无调用，以及实际传给子进程的执行指令在原规划请求之后并绑定正确摘要 / 任务。runtime 库 4、acceptance 3、delivery 4、scheduling 5 共 16 项通过，隔离及差异检查通过。修复后的真实模型及交付复验待新的已提交外部二进制，不以离线回归代替。


### pr1-desktop-delivered：并发 / 正常退出 / 显式恢复全链路（2026-10-24）

执行阶段修复 0bffa54 在外部正常 release 构建成功（2m16s）。另行登记 attempt-2-phase-fixed，沿用同一运行、需求、原测试及单文件任务范围，保留先前所有中断和门禁失败；本次不再中断，通过正常 Prepare explicit retry / Start execution 获得显式重新授权。不是在失败旧源码上连续重复调用，不提供实现代码或改测试。

修复后的 A / B 确实修改各自模块并通过独立 gate，各完成后才启动 C。C 的源码实际 import normalizeSku / subtotal，无输入变更。精确候选 57bbbfdce3d0fa395e0896a3cf3c5f1e410b9a6f 在独立 clone 通过冻结 9 项测试及 200 个外部用例；README 与三份测试哈希不变，仅三个允许模块改变。每个任务恰好一个成功实现提交，各提交的父树 diff 只含其自己的文件，恰好三次集成 merge；之前四次失败尝试无 commit / integratedHead。C 的 createdAt 不早于两个上游成功 turn 的 finishedAt，初次 Running 快照与截图明确保留 A / B 同时运行、C Pending。

最终 Haiku reviewer 返回恰好一个原生 structuredOutput PASS / []，全部 9 个 turn 已 ended / cleanupConfirmed。正常 UI 显示候选 / 原目标并点击 Merge into target checkout，界面和 SQLite 均为 Delivered / revision 1162。目标 fc279ee66bd8d3acf422f05f7a7c79cbb3867b99 的两个父提交精确为冻结基线 f7eff9d03f1fa07cdee900258781c5123db09872 和该候选，内容树一致、目标干净；交付后再次运行原测试及 200 用例全部通过。没有自动重复派发、提交、集成或目标 merge。

实际模型为 claude-haiku-4-5-20251001。CLI 已知报告费用小计 $0.4855136，两次中断费用 null，因此不能称为实际总费用，也不能称 $3 已限制全部历史未知开销。本次明确为正常 UI 辅助恢复 / 费用策略修订 / 产品修复后的通过，不称首次自主成功；初次 Planning 沿用空预算、执行前设 $3 的差异保留。正常退出后的磁盘状态仍是 Executing，显式 Reconcile 才把丢失完成事件的原轮次确认失败；不把陈旧 RUNNING 显示误写为自动恢复成功。本次覆盖正常 Cmd-Q 退出与重启，未覆盖强杀、OS 崩溃或跨平台故障。第十项继续暂停，CLI HOME 未隔离及标准 Tauri 配置差异仍适用。

证据：[并发恢复交付结果](reliability-evidence/shared-orchestration/sh4/parallel-recovery-delivered-20261024.json)，外部目录含预登记、冻结需求 / 基线、并发截图、只读进程登记与信号 0 存活检查、重启前后连续快照、原失败 transcript / gate、修复前红测及后续回归日志、新版构建记录、精确候选验证、提交归属 / 依赖时序核对、Delivered 截图、SQLite 一致性备份及 SHA256 清单。前一验收记录 30bbd44 已按本轮用户授权推送；本轮新修复和验收本地提交，未自动推送。


### p0-force-kill-cost：强杀恢复与默认未知费用阻断（2026-10-24）

独立 invoice-crash-project 冻结原需求、README 和三份原测试，基线 888bf9af6c60423b2ae13c294808bc050eaef23f，不含上一轮实现历史。预登记保存在 /Users/iwpz/Documents/tokage-validation/force-kill-recovery-20261024/request.json。使用既有已提交 0bffa54 的普通外部 release，二进制 SHA256 e1aa63ff5dcbf9b8eb137a1bf3ed4d794915977abc659e9b5cede59d03200f2c；当前主仓库 3c349ed 相对该版本仅增加验收文档。未重新安装依赖，既有热缓存，不作为性能测试。标准 identifier / app-data / incognito 隔离差异及 CLI HOME 未隔离继续适用。

正常 UI 生成三任务计划、锁定执行模型 Haiku；规划沿用原设置的并发 3 / 空预算 / bounded repair，执行前通过正常 UI 冻结并发 2 / $3 / manual repair / blockUnknownCost=true。没有把执行预算追认到规划。审批原计划的三条精确 node --test 命令后启动；运行 8f5f732e-1ed7-42a2-97e8-32df111c70a7 的两个独立执行轮次在原生快照与外部桌面观察中同时 Running，依赖任务 Pending。在 revision 383 对已核实路径的隔离桌面 PID 40711 发送 SIGKILL，只杀父进程，未信号子进程组。

强杀后曾观察到 CLI 孤儿进程，PID 69160 的 PPID=1，之后两个 CLI 均自行退出；不能称为桌面退出时即时清理成功。原始进程导出第一次误查 runtime-v1 子目录及原始字符串 hash，得到空对象，错误导出保留；后续按实际 runtime-v1.native / JSON digest 路径读取两个 children.json，记录 PID / PGID 69160、69161，最终逐个 signal 0 检查 PID / 进程组均已不存在。没有删除登记或人工终止孤儿进程。此次没有捕获“孤儿仍存活时点击 Reconcile”的窗口，也不覆盖 OS 崩溃 / 重启。

正常重启后仍是 Executing / revision 383 / 3 turns，没有自动重派。显式 Reconcile 将丢失完成回执的两个执行轮次确认 Failed / ended / cleanupConfirmed，费用仍 null；不接受孤儿进程留下的实现修改，不推断任务完成。正常 Prepare explicit retry / Start execution 后，默认未知费用策略阻断为 Blocked / revision 391：界面点名两个费用未知任务，提供费用策略说明和设置入口；turn 数仍为 3，没有新增付费轮次。blockUnknownCost 始终保存为 true，未关闭策略继续执行。

独立检查确认原 README / 测试哈希不变，两个任务 HEAD、集成 HEAD 和目标 HEAD 均保持冻结基线，无任务提交、集成 merge 或目标 merge，依赖任务从未派发。任务工作树中的未接受修改及已保存事件另行归档。第二次正常退出 / 重启后完整 native snapshot 与阻断前相等，Pipeline 正常展示费用阻断。已知费用仅规划 CLI 报告 $0.065453，两个中断轮次未知，不能称为实际总费用或预算覆盖全部开销。

本轮范围结论为强杀后的显式状态核对、拒绝接受丢失回执及默认费用阻断通过，故意停在 Blocked，不声称功能交付完成或崩溃后自主恢复；没有发现需修改生产代码的问题。第十项保持暂停。证据：[强杀与费用阻断结果](reliability-evidence/shared-orchestration/sh4/force-kill-cost-block-20261024.json)。

相关离线回归：runtime 库 4、acceptance 3、delivery 4、scheduling 5 共 16 项通过；native recovery 1 项通过，覆盖重复 launch receipt、损坏登记拒绝及存活进程不判 idle。隔离检查通过。本节点没有生产代码或前端变更，不重复全量前端构建。外部目录归档 7 组截图 / AX、连续快照、SQLite 一致性备份、未接受补丁、事件及 SHA256 清单。离线存活进程断言不追认为真实桌面 live-orphan Reconcile 验收。


### p0-live-orphan-ui：存活孤儿进程的恢复核对与提示修复（2026-10-24）

另行登记 /Users/iwpz/Documents/tokage-validation/live-orphan-recovery-20261024，独立项目基线 2cb992da17a9492fd7a4792518470408a5375124，沿用冻结需求及原测试，普通已提交 0bffa54 外部 release。规划继承并发 3 / 空预算 / bounded repair，执行前正常 UI 锁定全部任务 Haiku、并发 2 / $3 / manual / blockUnknownCost=true。没有补缓存、SIGSTOP、人工延迟、DB 写入或协议替换；只对精确核实的隔离桌面父 PID 72110 发送 SIGKILL。

运行 f165820b-2831-407b-9f02-c0cf8d76282f。revision 462 两个 executor Running、依赖任务 Pending。派发前记录实际 children.json 与 ps，两个 CLI PID / PGID 为 81079、81081。立即正常重启，恢复前 PID 81079 的 PPID=1、进程组仍存活；正常点击 Reconcile 后再观察，仍为 PPID=1 / 存活。revision 464 进入 Blocked：已退出轮次 Failed / ended / cleanupConfirmed，存活轮次 Unknown / ended=false / cleanupConfirmed=false。显式 Prepare retry 被拒绝，未新增模型轮次或接受丢失回执。随后 CLI 自行退出，无人工信号子进程组。

安全阻断正确，但 UI 的阻断主原因从历史轮次取第一个错误，显示另一个已退出任务的 native process idle，掩盖当前轮次清理未确认。新增只读展示规则：Blocked 中存在明确未结束或未确认清理的轮次时，优先点名这些轮次并提示等待进程停止后再次核对；不声称进程实时存活，不以旧错误替代当前清理条件，旧错误保留在技术详情。费用提示排在清理确认之后。未更改权限、重试、费用、恢复或门禁逻辑。

新增分类和 Plan / Pipeline 展示回归先红后绿；真实存活窗口的原始失败提示、快照与进程记录保留。新提示的普通桌面构建 / 视觉复验仍待进行，不以旧版截图追认新版通过。

修复节点验证：前端 64 文件 / 437 项全部通过，tsc 与生产前端构建、双语检查（1765 keys）、隔离和差异检查通过。本节点仅修改只读展示及回归，不重跑无变更的 Rust 全量；下一步使用已提交源码的普通外部 release 观察保留的 Unknown / 未确认清理状态，再通过正常 UI 核对已经自然退出的进程。


### p0-live-orphan-badge：中断任务卡不再显示绿色运行中（2026-10-24）

16aaf46 的普通外部 release 构建成功（原生 1m45s），同一原生 revision 464 在 Plan / Pipeline 已正确显示 P0.M1a 的清理未确认及再次核对指引，旧 idle 错误只在展开的技术详情中；无新模型调用。首次构建的短 SHA fetch 被拒，旧源码构建已 SIGINT 停止且未启动；完整 SHA fetch / clean checkout 后才构建并复验，失败日志保留。

目视复验另发现任务卡还直接显示持久化执行游标 RUNNING，虽然对应的最新 turn 已是 Unknown 或 Failed。新增只读徽标：仅在 Blocked、原任务游标 RUNNING、精确最新 attempt 对应同一 task 的 turn 已知 Unknown 或 Failed / ended 时，卡片和表格显示警示色“等待核对”。不改执行快照、任务调度或成功计数，不覆盖真正 Running 的轮次，也不借用其他任务 / 历史 turn。新回归先红后绿；最终普通桌面复验待本节点提交后构建。

本节点前端全量 64 文件 / 438 项通过，tsc / 生产构建、i18n（1766 keys）、隔离和差异检查通过。无 Rust / native 服务源码变更。


### p0-live-orphan-desktop：修复后桌面复验完成（2026-10-24）

f18246b 在相同外部 checkout 正常 release 构建成功（原生 1m39s）。正常启动保留既有授权，未增加 CLI 权限、模型调用或新任务。重新读取同一运行完整 native snapshot 与原 revision 464 完全相等；进程已自然退出但未核对的 Unknown / ended=false / cleanupConfirmed=false 保持原样，未注入现场或“清理成功”。Plan / Pipeline 的新提示在 16aaf46 已观察；f18246b Pipeline 卡片及表格的两个中断任务均以警示色显示“等待核对”，依赖任务仍 Pending，1280×820 窗口中布局与底栏正常。新提示说清理尚未确认，不声称此时进程仍存活。

只读确认原两个 PID / 进程组均不存在后，正常 Reconcile 把最后的 Unknown 轮次确认 Failed / ended / cleanupConfirmed，revision 467，清理提示消失、任务均 Blocked。随后正常 Prepare explicit retry / Start：未知费用策略再次阻断，终态 Blocked / revision 471，仍为原 3 turns。两个中断费用保持 null，原测试 / README 哈希不变，目标 2cb992da17a9492fd7a4792518470408a5375124 干净，无任务提交 / 集成 / 交付，依赖任务未派发。已知 CLI 规划费用 $0.0329905，不代表实际总费用。

本次覆盖了真实孤儿 PID 81079 / PGID 81079 在正常恢复核对前后均存活时拒绝确认清理，以及进程退出后的显式核对和默认费用阻断。存活窗口在旧 0bffa54 观察，新版仅修改前端展示，未再付费重复制造窗口；不能称为新版 live-orphan 二次实测、OS 崩溃或跨平台通过。第十项继续暂停。外部目录包含预登记、原生登记及 ps、SIGKILL 精确父进程记录、失败 / 修复前后截图与 AX、完整连续快照、SQLite 一致性备份、未接受补丁、事件、构建失败及成功日志和 SHA256 清单。

证据：[存活孤儿与提示修复验收](reliability-evidence/shared-orchestration/sh4/live-orphan-ui-fixed-20261024.json)。全量前端 438 项、tsc / 生产构建、i18n、隔离与差异检查通过；未改 native 服务源码，不重复无关 Rust 全量。所有失败与辅助步骤保留，本次不称自主功能交付。


### p1-target-drift：交付前目标修改 / 新提交保护（2026-10-24）

预登记 /Users/iwpz/Documents/tokage-validation/target-drift-20261024，独立 parity-project 基线 acdf6ce8f35f4ced07c520de8cc20915ff8684da，冻结 isEvenInteger 需求、README / 9 项原测试及模型任务不得改 user-notes.txt 的约束。普通外部 f18246b release，主仓库 28bf873 仅文档差异，沿用已归档标准 identifier / app-data / incognito 叠加；CLI HOME 未隔离。Node 内置测试，无依赖安装，既有热缓存，不是性能测试。

正常 UI 使用 Haiku 生成恰好一项计划并锁定执行模型；规划继承并发 3 / 空预算 / bounded repair，执行前正常配置并发 1 / $1 / manual / blockUnknownCost=true，再逐项批准 node --test。运行 bb1cef56-9fd0-4665-bad6-4d3ab875b00d 的 planner / executor / reviewer 均实际 claude-haiku-4-5-20251001，正常执行、真实 gate、任务提交 2dce77ef4cbe1466cf404bbe1a77b7388e534849、集成候选 a5260ab5a8d6879202c3583aa61ad1aacffb2840，最终原生 structuredOutput 恰好一个 PASS / []，revision 372 AwaitingMerge。真实 Reviewing 局部 loading 另有截图。

独立 clone 精确候选通过冻结 9 项测试及 2,027 个外部用例；只有 src/is-even-integer.mjs 改变，候选 README / 原测试 / user-notes 哈希不变。此后才按预登记由外部操作员修改测试目标，未修改模型工作树、计划、门禁或运行数据库：

- 目标 tracked user-notes.txt 有未暂存修改，另有 untracked personal-draft.txt；正常 UI 确认 Merge into target checkout 后 revision 375 Blocked，错误为 Delivery target branch or content changed。
- 将同样内容暂存后，正常 UI Reconcile pending delivery，revision 377 仍 Blocked。两步前后逐项比较全部文件 SHA256、index entries、staged / unstaged patch、目标 HEAD / reference，全相等；未强制清理、自动 stash 或覆盖用户内容。
- 按默认 Git 身份把上述测试用户修改提交到原 main，得到 0116bd5479f0ff33748c02f0ad8f046d3bfa67f0，父提交精确为原冻结基线；目标现在 clean 但 HEAD 已变。正常 UI Reconcile 仍拒绝旧交付，revision 379 Blocked，错误为 Delivery target does not match the recorded merge transaction。用户提交、文件和暂存区前后不变，没有自动 merge / rebase。
- 正常退出、重启并保留原授权后，完整 native snapshot 与 revision 379 完全一致，Pipeline 保留目标变更阻断；无自动重派、重审或交付。

最终仍为原 3 turns，均 ended / cleanupConfirmed；任务提交 / 集成各一次，目标仅增加操作员的一个用户提交，没有目标交付 merge。原候选工作树仍 clean 且 HEAD 精确匹配，候选与用户提交均保留；原测试及 README 在目标不变。CLI 报告费用已知小计 $0.1330851，不作为账单校验；没有用过期确认交付，也不声称新目标重新审批后的交付已验证。第十项继续暂停。

相关离线回归原等待器的 30 秒总期限连续两次耗尽，分别停在 Reviewing / reviewer Prepared 和 Executing / task Verifying；未运行到该场景的交付保护断言。原日志完整保留。测试专用 common/native.rs 的完整流程等待上限调整为 120 秒并记录 revision / elapsed 供诊断，生产超时、业务门禁和所有功能断言不变；未进入产品依赖，也没有验收特判。带诊断的原五场景用例通过，总计 55.20 秒，每次 settle 约 8–10 秒；没有复现原长等待，不宣称已定位具体 OS 负载或系统调用根因。此为测试等待预算加固，不把两次失败追认通过。

相关回归最终通过：acceptance 3 项（31.24s）、delivery 4 项（139.07s），其中原失败的五场景用例也通过；隔离与差异检查通过。未修改前端 / 生产源码，不重复无关前端全量或原生打包。证据：[目标变更与未提交修改保护结果](reliability-evidence/shared-orchestration/sh4/target-drift-protection-20261024.json)。外部目录保留预登记、基线失败、原生事件、审查 PASS、精确候选 clone / 验证、各探测前后完整 target 文件 / 暂存区 / HEAD 记录、6 组桌面截图与 AX、重启前后快照、SQLite 一致性备份、原两次超时 / 诊断重跑 / 最终回归日志及 SHA256 清单。本节点完成 P1 目标修改的拒绝交付验收，保持 Blocked，不称已完成新目标的功能交付。

### codex-executor-01 — Codex 原生审查输出契约（2026-10-24）

用户授权推送现有工作并完成 Codex 执行器测试、修复及桌面验收。现有提交已推送至 `419670e`。真实派发前检查发现 Codex exec 适配器未传递服务端 output_schema，也不生成原生 StructuredOutput。最初误判所有 Provider 都强制该事件；真实桌面检查后确认旧服务仅为 Claude 设置 schema / 要求原生回执，Codex 保持严格 JSON 文本路径，不能称为必然阻断（更正保留）。本节点接入当前本机 `codex-cli 0.160.0` 正常公开的 `--output-schema` / `--output-last-message`，服务端持有私有临时目录，退出并清理成功后读取独立原生结果文件；缺失 / 非 JSON 对象、失败退出及取消不输出成功回执，不从聊天说明中抽取 PASS。原范围、权限、沙箱及费用策略保持不变。

离线 Codex 相关回归 21 项通过（2 项真实付费测试保持 ignored），包含真实进程 fixture 验证原生文件缺失、非法 JSON、失败退出不能被 Done 或聊天 PASS 掩盖；审查回执 3 项通过，隔离与差异检查通过。首次回执过滤表达式未匹配测试（0 tests），已纠正过滤并实际运行 3 项，不将空跑算通过。完整桌面实测另行登记，当前节点不宣称真实 Codex 交付已通过。外部记录目录 `/Users/iwpz/Documents/tokage-validation/codex-executor-20261024`；第十项保持暂停。

### codex-executor-02 — 审查服务接线与单任务真实交付（2026-10-24）

第一轮普通桌面源 `bc0f226`：run `3fed63c8-349e-465b-b458-57feb7bab653`，原始需求经正常 Plan first / Codex GPT-6-Luna Medium 生成单任务计划。默认未知费用策略在 rev566 阻断派发，只有已完成 planner；正常 UI 关闭该项、失效旧批准并重新确认 node --test / Full Auto 后进入执行。真实 executor 仅修改 src/slug.mjs，任务提交 `382f535f9355843beb1ec23e281964eab93f66c3`、集成候选 `d2c9ff739090002130c5181f4d49523aa611acaa`，独立 clone 的 15 项冻结测试 / 513 个独立断言通过，README/test 哈希不变。严格 JSON 文本审查 PASS 后正常 UI 交付目标 `10273fc9106e0a8a61127d10739ac3e90a811762`；目标 clean / 冻结测试通过，正常退出重启后快照严格相等（Delivered rev613，3 turns），无重复派发。

真实观察纠正了上一节点的初始静态判断：服务只为 Claude 设置 schema，Codex 当时仍采用完整响应严格 JSON 文本解析，第一轮无原生 StructuredOutput，因此不算新适配能力的真实验证。现服务对 Claude / Codex 共用原生 schema 与恰好一次结果门禁，其余 Provider 的既有严格文本解析保持不变。当前 Codex exec resume --help 不支持 --output-schema，故 schema 审查使用完整候选提示开启新只读会话；普通执行 / 非 schema resume 不变。离线协议 fixture 通过公开 schema / last-message 参数产出回执，没有产品测试例外。

接线后 Codex 专项 22 项通过（2 ignored），审查门禁 4 项，runtime acceptance 3 项 / delivery 4 项全部通过；隔离与差异检查通过。前一节点后已完成 native 全量 326 通过 / 28 ignored；本节点针对变动运行上述相关回归。第一轮请求 / CLI / 构建 / 默认费用阻断 / 策略批准 / 原生 transcript / 候选与目标验证 / 4 组桌面证据和重启快照在外部 `codex-executor-20261024`。planner observed=gpt-6-luna；exec/review 未报告 observed identity，均 null；3 turns 实际费用均 null，不把 $1 策略当作已知实际总费用或硬账单上限。CLI HOME 未隔离，incognito 不证明前端设置持久性。后续三任务并行 / 依赖 / 正常退出恢复和原生审查另起预登记；第十项保持暂停。

### codex-executor-03 — 显式恢复后不再把历史失败当作当前错误（2026-10-24）

普通桌面 `211af1f` 三任务真实 Codex 恢复验收中，旧中断回执保留在 turns；PrepareRetry / StartExecution 已正常进入新 Executing，但 footer 又从旧 Failed turn 中选取“native process idle; lost completion cannot be accepted”作为当前错误。新增回归先失败后修复：仅在 Blocked / Stopped 时回退到尝试 / 任务 / 交付的历史错误；明确 runError 仍优先显示，历史记录及调度不改。Approved、Executing、Reviewing、AwaitingMerge、Delivering、Delivered 不再误报旧中断，Blocked 仍可回看错误。

前端全量 64 文件 / 440 项通过，生产构建（含 tsc）、i18n 1766 keys、隔离与差异检查通过。外部目录 `codex-executor-20261024/parallel-recovery` 保留原 UI（04-retry-two-running / 05-native-review）、红测和完整回归日志。后续通过已提交源码普通桌面构建复核只读展示；本节点没有额外模型请求。
