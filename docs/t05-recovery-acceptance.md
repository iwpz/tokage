# T05 · 阻塞、恢复与人工接手

日期：2026-11-01。结论范围为本轮实现与离线回归；不宣称新的真实模型、真实磁盘故障、外部桌面 UI 或跨平台验收。

## 已确认的问题与修复

1. **失败解释遗漏 gate 清理状态**：旧视图只看模型 turn，可能在 gate 尚未清理时提供重试/接手。统一投影现在检查 turn、gate、reservation、Git commit intent 和 delivery intent；未知时不提供执行重试、改计划或接手建议。后端仍是所有命令的唯一准入与验收来源，前端不设成功状态、不恢复进程。
2. **旧错误冒充当前原因**：改为读取未完成任务的最后 attempt/gate；失败 gate 没有 errorMessage 时也保留命令、退出状态及输出。已完成/已重试任务的旧错误保留在证据里，不显示为当前任务失败。
3. **读取失败被当成普通空结果**：missing snapshot 现在明确报错；读取错误和 worker 错误分别保存。旧快照不丢弃，但标明“最后确认的状态/revision”，执行动作在读取失败时禁用，取消仍可显式请求。独立“重试读取”只查询，不提交 ResumeRun，也不重复上一条命令。
4. **接手检查结果存在竞态**：原来在 taskId effect 中清空 inspection，可能擦掉快速返回的合法检查；慢响应也可能覆盖用户刚切换的任务。现在请求绑定任务/会话/计划生命周期；立即响应可见，失效响应不发布。切任务清除报告、用时和旧检查，后端仍核对 source/plan digest 并重跑范围、gate、集成与独立审查。
5. **规划中断恢复提前返回**：`execution` 尚未创建时，旧恢复函数在检查原生 turn 前返回，规划完成写入失败后无法确认清理。修复使规划和执行 turn 都经过同样的 launch receipt + process registry idle + cwd 验证。原生进程正常注销时会移除空 children registry，因此在等待清理后另写持久 idle receipt；SQLite 的完成写入失败时，它提供独立的清理证据，不能仅以 registry 文件不存在判断安全。无 receipt/清理未知继续拒绝；已停止且身份匹配时仅记录丢失完成为失败，不导入该计划、不自动调用模型。恢复 turn 使用正常 TurnFinished 事件记录失败与清理，不对无 execution 的运行写执行事件。用户之后可显式重新规划。

## 统一视图与动作

Plan / Pipeline / Chat / Session 共用失败证据面板，区分取消中、已停止、owner 冲突、存储/观察失败、清理未知、Git/交付待对账、gate 拒绝、审查拒绝、费用策略与未知阻塞。非 Pipeline 视图提供跳转入口。面板保留所有已记录 attempt 的工作目录、分支、报告、gate 命令/输出/退出状态、任务与集成提交，以及候选和交付产物。

- 取消请求不等于清理完成，也不等于交付成功。中断/未知清理不证明进程仍活着，更不证明请求免费。
- 恢复是显式检查已记录副作用；不盲目重发模型。worker 存储/清理错误不会把恢复按钮永久禁用；原生服务仍拒绝活跃 owner/job 或未知副作用。
- 重试要求可用计划、已确认清理与无冲突副作用；任务重试还要求有效授权。任务全部完成时优先提供最终审查，不能对未完成任务一律显示“重审”。
- 人工接手需要实际保留工作区；无工作区或未知清理不假装可接手。完成报告及 T04 人工申报用时仍走正常命令，不能跳过 gate 或把人工完成记成模型成功。
- 观察错误消失不等于 worker 写入故障已恢复；两种错误分别根据各自来源清除。一次命令的提交与随后读取不是原子操作，读取失败后先核对结果，不重复提交试探。

## 公开恢复步骤

App：先看失败面板及“重试读取”，检查最新持久状态和保留工作区。若仍有活跃 owner，在原 App/CLI 正常取消并等待清理；释放后显式“核对中断运行”。原生恢复拒绝时保留原因，不清锁/改库。gate 失败可在条件允许时重试任务，或查看工作区、人工修复、刷新检查，填写完整报告后重新验收。全部任务接受后才重跑最终审查，交付另需明确确认。

CLI 使用显式数据目录，勿混淆 App 的 `runtime-v1` 与独立 CLI 根：

```sh
tokage --data-dir /outside/repo/runs run show --id RUN_ID
tokage --data-dir /outside/repo/runs run events --id RUN_ID
tokage --data-dir /outside/repo/runs run accounting --id RUN_ID
# 仅在明确需要时请求取消，确认收到请求不等于已停止：
tokage --data-dir /outside/repo/runs run cancel --id RUN_ID
# owner 已退出/原生服务可接管后，显式恢复对账：
tokage --data-dir /outside/repo/runs run resume --id RUN_ID
# 确认清理、计划和范围之后，可重新授权未完成工作：
tokage --data-dir /outside/repo/runs run retry --id RUN_ID --confirm
```

retry 不等于 execute；planning 无有效计划时使用正常 `run plan` 显式重新规划。`run review` 会请求模型审查，不是只读刷新。人工接手使用 `inspect-task` / `takeover`，参数及报告要求见 [CLI 文档](cli.md)。不要删除 owner lock、children registry、launch receipt 或编辑运行数据库来绕过恢复；恢复也不会猜测丢失的产物已通过门禁。

## 历史记录、验证与 R03 边界

核查来源：

- [m2g 异常记录](reliability-evidence/m2g/validation.md)：历史强杀后无交付、保留未知费用，并保留首次认证失败与恢复探测。该证据来自旧协议，不追认为当前 UI 或当前 Adapter 通过。
- [m3a-009 拒绝记录](reliability-evidence/m3a/m3a-009/attempt-1/review-findings.json)：范围外自动记忆写入被拒绝。保留原结论，不通过放松 scope 或改 HOME 掩盖；本轮只改解释/接手，不宣称已解决该 CLI 行为。
- [m3a-010 准备记录](reliability-evidence/m3a/m3a-010/preparation-result.json)：external-ui-v1 在模型发送前阻塞，不能算模型或运行恢复成功；9/20 不推进。

离线验证覆盖页面与动作、立即/延迟接手响应、读/写错误分离、命令提交后观察失败不重发，以及公开 runtime/CLI 的取消、owner 中断、未知清理、存储失败、gate/scope/review 拒绝与人工接手。规划恢复回归先在旧实现复现失败，再验证保留 receipt、缺 launch receipt、缺 idle receipt、清理回执存在但进程仍活跃、重复恢复不增模型调用、显式重新规划才增调用。测试 fixture 与故障注入只在测试文件/临时目录中，不加入运行时特判。

本轮前端全量 625 项通过；共享 runtime 全量 67 项通过，最终关键路径回归另 16 项通过；CLI workflow 18 项通过；App Rust 编译检查、TypeScript、i18n、生产前端构建及隔离检查通过。全量与最终差异复测的执行范围分别保留，不将离线 fixture 算作真实模型通过。

实际日志、失败记录与源文件哈希见 [验证清单](reliability-evidence/t05/verification.json)。旧运行同时缺少清理 receipt/registry、真实 OS 杀进程、真实磁盘满/权限故障、多平台 owner、长期人工救场复发率，以及本轮外部正常桌面 UI 观察仍进入 R03/R01 后续样本；“重启正常”不作为根因修复证据。
