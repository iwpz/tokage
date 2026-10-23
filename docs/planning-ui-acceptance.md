# 计划结构化输出：桌面 UI 验收

2026-10-23，子节点 `ui-plan-sections-desktop`。被测源码 `0521b133b2de33de7dfbcb49aed60e70992b6be8`。结论：**部分通过，整个需求尚未验收完成**。

## 环境与操作

按照 `docs/testing-isolation.md` 使用 `external-ui-v1`，独立克隆已提交源码，正常 Tauri release 构建（6m01s）及 app 打包，通过 CUA 的原生可访问性与截图操作。配置叠加仅隔离应用标识、应用数据目录及 incognito WebView；未修改产品源码。此包不是原发布二进制，临时 WebView 不证明跨重启设置持久性。Node 22.17.1、npm 10.9.2、Cargo 1.97.1；npm ci 使用普通缓存，Rust 两个编译 job，不做冷启动性能结论。

通过正常设置界面确认 Demo / Debug Provider 关闭，计划必须审阅、自动批准阈值为 0。通过目录选择器添加独立的 Pocket Tasks 样例 Git 项目，仅允许 Tokage 和 Codex CLI。通过正常模型选择器使用 `CODEX_CLI / gpt-6-luna / medium`，Codex CLI 0.160.0；未启用 Full Auto。CLI HOME 未隔离，未复制凭据或改个人全局配置。实际模型身份与实际费用未上报，均保持未知。

两次模型请求均为只规划，无任务执行、批准或合并。可靠性第十项仍暂停，不增加 20 项样本计数。被测源码和样例 Git 项目在请求后均保持干净。

## 已观察通过

- 默认收起 Spec、Contracts、Tasks、Notes。
- Spec 可展开 Goals、Scope、Key decisions，各子段可独立查看。
- Contracts 整体为卡片，C1/C2 等各自为完整子卡片；说明、类型代码与边界规则保留在所属契约中，代码无需再次展开。
- Tasks 整体为一张卡片，展开直接显示完整 YAML；实际滚动可到第三个任务及验收字段。
- Notes 二十条列表可滚动到底部。
- Session 历史回看同样提供结构化段落和契约子卡片。
- 成功终态及错误终态均移除 loading；错误后卡片仍可展开阅读。
- 第二轮生成正常计划，Plan 页面显示 3 tasks / 2 contracts / Awaiting plan approval，未执行。

第一轮曾在外部 AX 观察到 `button (collapsed) Notes Writing…`，证明收起标题可显示 loading；随后截图时已经进入错误终态，因此仅记录瞬时观察，不声称有该瞬间的截图或连续录像。

## 两次尝试

1. `1758184b-f2e9-4f19-88d8-abfa1c5da633`：正常规划过程中读取 README，生成计划后进入 Blocked，错误 `planner text/validation evidence mismatch`。原始失败保留。持久文本事件长度为 72、4338 字符，分别对应过程说明及最终计划。具体拒绝原因仍需独立诊断，不能仅根据错误文本断定根因。
2. `dcfae610-1371-4f9d-84f8-a4f3dec1a19c`：新输入直接提供已知项目摘要，要求仅最终计划、不调用工具；同一二进制及设置下正常完成，进入 AwaitingApproval。持久事件只有一条 5099 字符的完整计划。此轮不是对首轮失败的修复证明。

设置准备期间一次控件索引刷新导致隔离实例的 confidence threshold 从 80 误变为 79，已通过正常 UI 恢复 80，发生在请求发送前。两次 CUA 操作报告应用状态被外部改变，均刷新观察后继续；不声称操作期间没有外部干预。

## 剩余待办

- [ ] `ui-plan-stream-provider`：明确并实现所需的 Codex 实时文本能力。本轮最终计划按完整消息一次到达；当前原生适配器在 `chat/codex.rs` 的 `agent_message if done` 才发出文本。不能以模拟分片冒充真实逐行流式，也不能据此推断其他 Provider 的行为。
- [ ] `ui-plan-stream-desktop`：有真实计划分片后，通过正常 UI 观察展开卡片内容连续增长、底部 loading、段落切换，以及用户展开状态持续保留。
- [ ] `ui-plan-cancel-desktop`：真实输出期间通过正常取消控件停止，检查局部 loading 清除；本轮未另发请求测试取消，也未测试断网。
- [ ] `ui-plan-validation-diagnostic`：独立定位首轮 `planner text/validation evidence mismatch`；保留原门禁及失败，不用第二轮改写结果。

## 证据

机器可读清单与外部文件哈希：[desktop-20261023.json](reliability-evidence/plan-ui/desktop-20261023.json)。完整截图、AX 文本、输入、构建日志、配置叠加、只读 SQLite 一致性备份及 SHA256 清单保存在 `/Users/iwpz/Documents/tokage-validation/ui-plan-sections-20261023/`。独立应用仍停留在第二轮计划的正常 Chat 审阅界面。

本次只新增验收记录。归档前执行测试隔离与差异检查；未更改业务或测试设施，不重复将既有 425 项自动化测试称为桌面证据。

## ui-plan-validation-diagnostic 修复

只读重放首轮持久计划和冻结配置后，实际错误确定为 `unsafe_acceptance_command`：`test -f index.html && test -f styles.css` 含 Shell 运算符。不是已经证实的文本证据差异。旧代码对 `planning::normalize` 调用 `.ok()` 丢掉具体失败，并一律写成 evidence mismatch。

现保留原始校验错误；只有返回值确实不同于持久文本时才报告 evidence mismatch。规划提示明确每个命令条目只允许一个直接执行的命令，独立检查拆成多个条目；显式重试将以有界 JSON 错误数据提供历史诊断，不自动重试、不授予命令权限。原始失败记录保持不变。

验证：runtime planning 9 项及 planning 库 61 项通过；新增回归验证复合命令仍被拒绝且持久保存真实规则、显式重试使用独立命令后进入审阅、旧失败保留，以及真实文本不一致仍阻断。重试测试初次因 fixture 未报告实际费用而正确触发未知费用门禁；仅将该离线 fixture 改为提供明确费用，没有修改产品费用政策。桌面修复复验待流式传输修复后进行。

## ui-plan-stream-provider 修复

Codex 的 Planner 角色改用官方 [app-server v2 stdio 协议](https://learn.chatgpt.com/docs/app-server)；执行与审查角色继续使用既有 exec 路径。协议与当前安装的 Codex CLI 0.160.0 导出的 JSON Schema 核对。初始化后显式指定只读沙箱、never 审批、工作目录、所选模型和 effort；必须收到相同只读权限的服务端确认才启动模型。恢复会话同样重新指定权限，不回答意外的授权或工具输入请求。没有修改用户配置或添加测试分支。

原生进程读写循环支持双向 JSONL，文本 delta 即时发布；完成消息核对聚合内容，避免重复追加，拒绝被改写或缺少最终证据的文本；响应身份与顺序防止重复派发。Done 仅在结构化用量和成功完成帧后成为候选，并仍等待进程正常退出；非零退出、迟到错误、断开及取消均不能交付成功。取消后关闭并清理正常进程组，前端将 Stopped 映射为已取消并保留部分内容。

验证：前端 63 文件 / 426 项、native/runtime 完整回归 375 项（28 个既有 ignored 不计通过）、CLI 20 项通过；新增协议解析回归及最终 planning 进程回归另行通过，覆盖增量在 Done 前到达、跨消息空行、聚合不重复、权限确认、重复响应、缺失最终消息、迟到错误、非零退出、正常 EOF 与真实子进程取消。认证失败 fixture 更新为 app-server 错误帧；原有 exec 退出失败回归保留在只读 reviewer 路径，没有撤销原断言。隔离、类型、i18n 与差异检查通过。桌面正常 UI 复验记录追加于下文。

## ui-plan-stop-race 修复

修复包 `a9bfaa2` 正常桌面复验中，原需求生成成功：真实 app-server 输出 1923 条文本事件，命令为独立的 `test -f` 条目，进入 AwaitingApproval。长契约输出也已观察到 Contracts 与 C1 展开及局部 loading。一次长输出模型漏填 commands，被正确拒绝并显示字段校验错误，不能记成功；随后长契约正常生成，但实时点击 Pipeline 的 Cancel run 返回 `revision conflict`，未能停止。这些失败均保留。

根因：界面取消使用最近观察版本，文本分片持续递增版本，命令到达前该版本往往已过期。现仅允许已有版本的 CancelRun 在 Planning / Executing / Reviewing / CancelRequested / Stopping 状态下针对同一运行原子地使用当前版本；事件序号仍由当前持久版本递增，原请求哈希及重放回执保持不变。未来版本、初始空版本、Draft、批准、执行、修改及交付等其他命令的严格版本检查不变。取消只撤销授权并停止，不授予任何执行能力。

验证：contracts、repository、turn_owner 回归通过（包含既有 Draft 过期取消必须拒绝的原断言），新增回归在流式版本推进后使用旧 UI 版本取消，验证停止后的进程清理、部分记录保留、回执幂等、未来版本和其他旧版本命令继续拒绝。初次补丁被原 Draft 版本断言检出范围过宽，已收窄至活跃状态再完整重跑上述集合。最终桌面取消复验另行登记追加。
