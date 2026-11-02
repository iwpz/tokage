# T08 · Composer 显式任务目标与分流后上下文规划

日期：2026-11-01。实现、离线验证及普通桌面分流预览观察完成；真实模型的任务续聊/新线程端到端仍留 R02，不推进暂停的 9/20 验收。

## 入口与路由

共享 Runtime 会话的聊天页使用任务追加 Composer。目标下拉来自当前冻结计划中的真实任务，默认自动分流，逐项显示任务 ID、标题和状态。保留失效目标，不自动改选邻近任务。没有现有 Runtime 的普通请求、演示及旧兼容会话沿用原入口；查询 Runtime 失败时不开放旧 Composer 作为回退。

只读 `runtime_route_task_message` 在服务端读取当前快照，以共享 `tokage_context` 分流规则产生预览。显式 `explicitTaskId` 优先，未知 ID 直接拒绝。自动分流优先精确任务 ID，T1 不匹配 T10；多个 ID 或多个匹配任务要求显式选择。自动的重新规划信号仍指向计划修订；独立问题指向独立请求。它们显示解释与正常入口，不自动更改计划、扩大范围或调用旧执行器。

只有确定目标后才显示该任务的 T07 建议/动作/历史，并在发送前使用 T06 同任务上下文预检。预览本身不派发、不消费动作、不推进 cooldown。自动预览和显式选择都绑定消息原文、会话、目标、revision、配置和授权；异步旧响应不能覆盖新的草稿或目标。

## 发送与竞态

发送使用正式 schema-v1 `AppendRoutedTaskMessage`：message、explicit_task_id（可空）、target_task_id、source_turn、config_hash、confirmation_digest，外层 expectedRevision 保留预览对应版本。服务端重新分流并核对确切任务和来源，再调用与公开 AppendTaskMessage 相同的追加核心；CLI `run submit --file` 也可使用同一协议，不引入专用验收入口。

Run/Task 必须 Blocked、冻结授权有效、原生与 Git/gate 副作用已清理、无活跃 worker、无偏离合同或未消费 follow-up、来源任务/分配/线程兼容、无待决原生审批。DONE、取消、运行中、删除、来源轮次/模型/计划/配置变化和 revision 冲突均拒绝，不改投别处。追加文本逐字保留，包括首尾空白。前端阻止重复点击；服务端维持原 commandId 幂等。发送被拒绝时保留草稿；成功应答后清除该份草稿，随后读取失败不会恢复成另一条待发送消息。

正常审批只经 ResolveApproval 续传，不能被追加消息变成新模型轮。T07 显式接受只绑定所选任务的一次动作；off 不执行上下文动作，suggest/auto、硬阈值、FRESH 新线程确认和原工作树基线均复用既有流程。承载轮完成不证明实际缩减或原生 compact。

## 当前验证

组件回归覆盖目标选择、自动路由、原文/精确源发送、失效目标保留、拒绝保留草稿、终态/审批/读取失败、信任撤销、异步旧响应与重复点击。Rust 覆盖只读分流、ID 边界/歧义、计划/独立请求路由、错误目标、取消/完成/审批/source/config 竞态和选中任务的上下文门禁。离线子进程协议场景从公开命令走显式和自动路由，验证同任务续聊、suggest 接受、保留载荷及真实子进程协议中的新 thread；fixture 不代表真实模型。

前端全量 645 项、context 单元 46 项、Runtime 全量 85 项通过；最终角色/待决审批检查另经 Runtime 单元 19 项与任务上下文协议 6 项验证。集合有重叠，不相加为独立场景数。TypeScript、i18n、build、App/CLI 编译及隔离检查通过。

## 普通桌面 UI 观察

按 [登记](reliability-evidence/t08/registration.json)，从提交 `b8233a3` 创建独立克隆，完整 release 构建，使用标准 Tauri incognito overlay 和预先声明的旧隔离 App 身份/数据目录。UI 中确认 Demo/Debug Provider 关闭、在线用量查询关闭；没有修改 CLI HOME 或复制账号凭据，不宣称跨重启设置持久性或原发布包实测。

通过普通目录选择器加入新的隔离 Git 文档项目，经正常导入表单创建 T1、T10 两项任务，保留 AwaitingApproval。界面实际观察如下：

- 自动分流的 `T10: please inspect NOTES.md.` 显示目标 T10，未误匹配 T1。
- 草稿不变、下拉显式选 T1 后显示目标 T1。
- 自动分流的 `T1 and T10 need updates.` 要求显式选择，保留草稿。
- `replan T1 and T10` 显示计划修订提示；独立问题显示 DIRECT_DISPATCH 与新会话提示，均不误投任务。
- 所有场景因尚无获准的执行窗口，Send to task 均禁用。未点击生成、审批、执行或任务发送。

只读核对持久快照：新 run revision 2、AwaitingApproval、approvedDigest=null、execution=null、turnCount=0、仅两次创建/导入命令和两个事件、零 turn events；旧隔离库仍为 5 条历史 attempts。没有新模型轮。之后清空草稿并关闭验证 App，应用清单确认退出。

UI 自动化曾误选外部父目录，已拒绝该目录的 Tokage 执行授权，未在其中创建会话；随后从正常选择器选定正确子目录。AX 索引刷新和文本选中失败的自动化摩擦也保留在 [桌面记录](reliability-evidence/t08/desktop-verification.json)，未用内部驱动或写库补状态。

[自动分流截图](reliability-evidence/t08/ui/auto-t10.jpg) 与 [显式目标截图](reliability-evidence/t08/ui/explicit-t1.jpg) 展示普通控件与真实导入任务。普通 UI 证据限于分流预览和未授权目标保护；完成/取消等竞态、实际发送与上下文绑定目前是离线证明，真实模型端到端保留 R02。

最终命令、构建版本、数据观察及证据边界见 [最终验证清单](reliability-evidence/t08/final-verification.json)。下一施工项为 T09。
