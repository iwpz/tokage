# T10 · 窗口身份约束与匹配成本

日期：2026-11-01。源码起点 `fc63e3e`。范围为共享 Runtime 窗口身份/派发、matching 的复用费用假设、相关单元与公开协议离线测试、规格和证据；没有模型请求、账号操作、桌面或跨平台实测。工作区同时出现的界面样式修改属于其他工作，未纳入本项提交或验收结论。

## 启动身份与失效

每个实际执行任务在原生启动前持久保存 `windowIdentity`：冻结的 configHash、授权计划 digest、完整 ModelAssignment（含 providerPath、模型参数）、executor 角色、write scope、实际 Workspace root/cwd/branch。配置 hash 覆盖工作区/目标、权限模式、信任、命令策略、模型和其他有效设置；旧轮次没有启动记录时不能凭当前配置重建旧许可，保守新开窗口。读取旧快照不补身份，不改原始证据。

共享调度候选要求 DONE/Completed、ended、cleanup 确认、时间有效、角色/分配/原生可用模型身份一致、原任务没有遗留 session allowed 命令，并核对启动记录及原工作树记录。更换目录、任务范围、模型/CLI 路径参数、配置或授权计划均失去复用资格。未报告 observed model 时仍仅有请求身份，不能宣称服务商实测确认。

派发边界再次核对身份，并检查实际路径 canonical identity、Runtime 管理目录、Git 顶层、Git common directory、注册分支、树内容、清洁状态及 scope。修复允许沿用同任务的失败修改，但必须保留原始干净 baseline 且 HEAD 未变；跨任务要求与集成树相同、干净且精确相同 scope。无法证明连续性时新建独立工作树，不迁移旧 session allowed 命令，不删除失败现场。若重新启动的原生 resume 返回了不同线程，当前 attempt 在验收/提交前阻断。

占用以 provider/path/native thread 及 workspace 为资源，而不是以某个历史 turn ID 为资源。活跃任务借用过的来源、未结束/清理未知的别名、同批派发和更新的别名均不能被重复借用；更晚失败的线程也不能被旧成功回执遮盖。同任务按持久 attempts 顺序，跨任务秒级时间打平时保持保守。角色/权限改变不继承旧许可。

## 费用假设

原 matching 将上游同 provider/model 分配视为窗口已复用：baseline 从 15,000 降到 8,000 并按热缓存计价，还生成不存在的 attempt-reuse ID。分配阶段实际上还没有原生窗口、工作目录、授权或用量证明。本项删除该优惠，模型分配、重算与预算预留统一采用新窗预测；返回 reuseAttemptId=null。

Runtime 仍通过 T09 的真实来源参数独立评估候选，身份或容量/缓存等不符则新开窗口。预算预留不预扣预测节省，已经批准的复用也不回填成账单节省。知识包的价格/画像变化仍经 matching 正常重算，并通过 runtime packVersion/configHash 和输入来源追溯；这两类预测采用不同粒度，数值不承诺相等，不把保守预留称为价格上限。T11 继续费用归属和 UI 下钻，T12 继续校准。

## 验证与限制

单元覆盖冻结身份、权限配置/计划变更、cwd/branch/scope、角色、观察模型/Provider、CLI 路径/模型参数、旧轮次、清理未知、活跃来源别名、更新失败别名及不同线程共用目录；matching 对照验证仅增加同模型依赖分配不会降低成本、改变候选或制造复用 ID。公开 Create/Import/Approve/Start/AppendTaskMessage 协议夹具验证不同 scope 新窗、身份持久重读、外部改变来源分支后保留旧产物并新窗，以及 resume 线程变化不得进入门禁/提交。fixture 仅在 tests 下，不属于真实 CLI/模型验收。

在并行开发推进期间，从已提交基线 `6e96535` 创建独立 checkout 并仅复制 T10 文件，排除未提交的 gate/validator/知识包/界面改动；复用 Cargo 编译缓存，不作冷启动性能结论。最终 Runtime 96 项、matching 50 项全部通过；最后强化的线程失效/占用及上下文分支另经 45 项针对性复验，集合重叠不相加。App/CLI 编译和隔离检查通过。本项没有前端改动，不借用其他界面工作的测试作为 T10 验收。

早期异步 Git 公共目录查询闭包有 Rust 生命周期错误，已修复；新测试重复了夹具已完成的审批，已改用夹具返回的正式执行请求。早期工作区验证因源码并行变化而中止，记录保留，完整通过结论取独立 checkout 的最终日志。

具体命令、数量、失败及最终验证记录见 [验证清单](reliability-evidence/t10/verification.json)。真实正常 UI 的任务续聊/跨任务复用仍在 R02，暂停的 9/20 不变，Claude 当前不可用仍在 B01。T10 不证明其他平台、CLI 更新后的普遍兼容性、服务商实际计费或真实节省。
