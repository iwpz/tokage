# Tokage 开发进度与阶段规划

返回 [README](../README.md) · 更新于 2026-10-04

本文对照 00–10 设计文档记录实现现状，并规划后续阶段。状态标记：

- `[x]` 已完成
- `[~]` 已开发但不完整（缺口写在条目后）
- `[ ]` 未开始

## 一、现状总览

目前是一个可用的多 CLI 对话客户端：探测本机 CLI、选模型、在项目中以 Plan / Direct 模式对话、查看工具步骤与 Diff、按会话建 worktree。设计核心闭环（分流 → 规划 → 匹配 → 编排 → 验收 → 学习）尚未实现。

| 模块 | 进度 | 说明 |
|------|------|------|
| 00 总体架构 | `[~]` | 单 crate，无内部事件总线，IPC 类型手写 |
| 01 原生环境 | `[~]` | 探测与 4 个 headless Adapter 完成；无统一 trait、PTY、原生会话透视 |
| 02 规划引擎 | `[~]` | 仅 Plan 模式提示词 + 只读权限；无分流、摘要、计划结构 |
| 03 编排引擎 | `[ ]` | |
| 04 模型匹配 | `[ ]` | 模型目录聚合已完成，知识库与分配未开始 |
| 05 上下文引擎 | `[ ]` | 仅有续接 / 换模型时回放历史 |
| 06 用量与熔断 | `[~]` | 计费类型、套餐、Codex 额度可读；用量不落库，无熔断 |
| 07 会话树 | `[~]` | 项目 → 会话两级；无计划 / 任务 / 执行窗口层级 |
| 08 存储 | `[ ]` | 用 localStorage 代替 SQLite |
| 09 防护 | `[~]` | 进程组回收、无 shell 拼接、凭据不读；无双层超时、过滤、权限角色 |
| 10 前端 | `[~]` | 对话工作区完成；无计划审阅、流水线、用量看板 |

## 二、模块明细

### 01 原生环境与联邦引擎

- [x] 登录 Shell 环境抓取：`$SHELL -ilc`、5s 超时、仅内存、可重新加载（`env.rs`）
- [x] Provider 白名单注册表（约 20 个 CLI），featured / 手动添加，自定义路径（`providers.rs`）
- [x] 探测：which → `--version` → 登录态（只查存在性）→ 模型列表，并发 + 单 Provider 40s 超时
- [x] `ProviderStatus` 七种状态；安装 / 登录命令仅供复制（`install.rs`）
- [x] 模型目录聚合：按底层模型合并 effort / fast / 上下文变体（`models.rs`）
- [x] Headless Adapter：Claude Code、Codex CLI、Cursor CLI、Copilot CLI，含录制样本快照测试
- [x] 统一事件模型：Thread / Model / Reasoning / Text / ToolStart / ToolEnd / FileChange / Done / Error
- [x] 续接（resume）、Claude 原生 fork、换模型时回放历史
- [x] 工作区信任：只对用户授权的 CLI 传递信任
- [~] Adapter 抽象：各 Adapter 独立实现；缺 `ProviderAdapter` trait、能力位、版本区间、`UNSUPPORTED_VERSION` 降级
- [~] 角色权限：只有 Plan（只读）/ Direct 两档；缺 PLANNER / EXECUTOR / TESTER / REVIEWER 与 `write_scope`
- [ ] Antigravity CLI（`agy`）、Aider、OpenCode Adapter
- [ ] PTY 兜底执行（`portable-pty` + `vte` 稳定帧）
- [ ] `build_compact`：调用 CLI 原生压缩
- [ ] 原生会话透视（只读解析 `~/.claude/projects`、`~/.codex/sessions` 等，`notify` 增量）
- [ ] 错误分类处理（AuthRequired → NEEDS_LOGIN、RateLimited → 熔断、ParseError 降级）
- [ ] `FakeAdapter`（回放、注入卡死 / 刷屏 / 孙进程 / 越界写入）

### 02 规划引擎

- [~] Plan 模式：首轮附加规划指令、保持只读；输出仍是自由文本
- [ ] 本地分流 DIRECT / PLAN（auto 模式）
- [ ] RepoDigest：目录树、manifest 解析、构建 / 测试命令提取、tree-sitter 签名、git 热点、8K 裁剪、缓存
- [ ] 规划输出格式（[plan-format.md](02-planning-engine/plan-format.md)）解析与本地容错修复
- [ ] 计划校验：无环、领域映射、并行写入范围不重叠、验收命令补全与允许列表、任务数上限
- [ ] 落盘 `.tokage/plans/<id>/`，写入 `.git/info/exclude`
- [ ] 计划版本与增量重规划（计划差异、适配任务）
- [ ] `planning.auto_approve_below_usd`

### 03 编排引擎

- [ ] 任务状态机（PENDING → READY → RUNNING → VERIFYING → DONE / REPAIRING / BLOCKED）
- [ ] DAG 调度：关键路径优先、全局 / 单 Provider 并发槽、写入范围冲突等待
- [ ] 任务简报输入、任务报告（模型输出 + `git diff --stat` + 关卡结果 + 用量）
- [ ] 契约偏离时暂停下游
- [ ] 验收关卡：本地执行、独立超时、尾部 200 行证据、范围校验
- [ ] 修复兜底三步：同窗口修复 → 改派 → BLOCKED
- [ ] 测试阶段：测试任务发现缺陷时为实现任务生成修复任务
- [ ] 工作区隔离：`shared` / 任务级 `worktree` 与按依赖合并、冲突生成 integration 任务
- [ ] 计划完成：全量关卡、交付汇总、可选评审任务
- [ ] 暂停 / 恢复 / 取消 / 重试、断点续跑

### 04 模型匹配引擎

- [ ] 定义包格式（[knowledge-pack.md](04-model-matching-engine/knowledge-pack.md)），安装包内置一份
- [ ] 领域专长表与需求分表
- [ ] 成功概率：sigmoid 先验 + Beta 后验
- [ ] CostModel：`eff_price` / `scarcity` / `C_turn` / `C_exec`，供规划、匹配、上下文共用
- [ ] 预期成本 `E(m, t)`、把握门槛过滤
- [ ] 规划模型选择（按 `p` 排序）、DIRECT 单任务分配
- [ ] 全局分配：累计使用率、并行负载、窗口复用加成、`explanation`
- [ ] 改派后整表重算
- [ ] 学习：`task_outcomes` → 后验、用量画像、成本校准、偏好信号
- [ ] 定义包远程更新（minisign 签名、pending → 应用）；设置中的开关已有 UI

### 05 上下文生命周期引擎

- [~] 窗口续用：同 CLI 同模型自动 resume；换模型新开窗口并回放历史
- [ ] 任务简报组装（[task-brief.md](05-context-lifecycle-engine/task-brief.md)），固定头部
- [ ] 窗口复用决策（续用成本 vs 新窗成本 + 理解成本）
- [ ] 窗口内 CONTINUE / COMPACT / FRESH 成本决策、缓存温度、硬约束
- [ ] `context.mode` auto / suggest / off 的行为（设置已有 UI）
- [ ] 主会话追加消息分流到任务窗口 / 重规划 / 直接分发
- [ ] `context_actions` 记录与实际节省结算

### 06 精准用量与熔断引擎

- [x] 计费类型识别：订阅 / 按量 / 第三方后端，Base URL 只保留 host（`usage.rs`）
- [x] Claude / Codex / Cursor / Antigravity 账号套餐识别（不读 token）
- [x] Codex 额度：从 rollout 日志读 `rate_limits`（主 / 周窗口、重置时间）
- [~] 逐轮用量：`Done` 带 input / output / cost / premium requests；缺缓存读写、`api_calls`、`context_tokens`、`source`，且不落库
- [ ] 其他 CLI 的被动额度采集（输出中的额度 / 速率限制提示）、`usage_patterns` 正则
- [ ] 本地推算 `u_now` 与 `quota_equiv_cost` 校准
- [ ] 主动额度查询（PTY `/usage`，默认关闭；设置已有开关）
- [ ] 计划预算：预估、执行中追踪、超 50% 暂停
- [ ] 节省账本（ASSIGNMENT / CONTEXT / DIRECT，参考模型）
- [ ] 熔断与恢复：QUOTA_SOFT / QUOTA_HARD / BUDGET / HEALTH / AUTH，滞回与半开探测
- [ ] `get_usage_dashboard` 数据

### 07 聚合会话树引擎

- [x] 项目 → 会话（侧边栏），会话重命名 / 归档 / fork
- [x] 对话事件实时推送（`chat-event`）
- [x] 取消运行中的轮次（整组回收）
- [ ] 主会话 → 计划 → 任务 → 执行窗口层级与 `predecessor_id`
- [ ] 主会话状态（PLANNING / REVIEW / RUNNING / PAUSED / BLOCKED / IDLE / COMPLETED）
- [ ] 完整 `UiEvent` 通道、50ms 合并、`set_focus`、重连补齐
- [ ] 终端接管（生成 `claude --resume <id>` 等，完成后跑验收）
- [ ] 外部会话挂载到树上
- [ ] 标题自动取设计稿目标

### 08 本地数据仓

- [x] 项目外文件副本：`app_data_dir()/files/<项目>/<日期>/<文件名>`
- [ ] SQLite：WAL、单写线程批量提交、读连接池
- [ ] refinery 迁移与启动前备份
- [ ] [schema.md](08-storage/schema.md) 全部表
- [ ] 从 localStorage 迁移现有项目 / 会话 / 设置
- [ ] chat_logs 截断（64KB）、FTS、保留策略与空闲清理
- [ ] 导出 / 清空项目记录、删除项目时清理 `.tokage/`

### 09 边缘约束与防爆红线

- [x] R3 进程组回收：`setsid` + `killpg` SIGTERM → 3s → SIGKILL（`proc.rs`）
- [x] R4 不经 shell 拼接（Adapter、open_in、git 均按参数数组）
- [x] R8 不开启跳过权限确认
- [x] R11 凭据文件只检查存在性
- [~] R1 输出过滤：依赖结构化输出 + `NO_COLOR`；缺 OutputFilter 流水线（CR 折叠、ANSI、spinner、重复折叠、64KB）
- [~] R2 超时：探测与环境抓取有超时；对话轮次没有空闲 / 硬超时监督与档位、倍数
- [~] R7 只读窗口：Plan 模式用 CLI 只读参数；缺「检测到改动即终止告警」
- [ ] Windows Job Object
- [ ] 异常退出残留清理（`run/children.json`）、`nice +5`
- [ ] 连续失败计数 → HEALTH 熔断
- [ ] R5 验收命令允许列表与参数数组执行
- [ ] R6 写入范围校验与一键还原越界文件
- [ ] R9 / R10 上下文纪律
- [ ] R12 预算红线
- [ ] 每条红线对应测试

### 10 前端 Mission Control

- [x] 标题栏、侧边栏、可拖拽分栏、主题（亮 / 暗 / 系统）、中英文
- [x] 对话视图：推理、正文、工具步骤、文件变更、Markdown（sanitize）
- [x] Diff 视图、文件预览面板
- [x] 模型选择器（变体合并、effort / fast / auto tier）
- [x] Provider 状态菜单、CLI 管理设置页
- [x] 项目 git 信息、在 Finder / VS Code / 终端中打开、会话 worktree
- [x] 信任确认弹窗
- [~] 设置页：规划 / 上下文 / 预算 / 熔断 / 超时 / 并发 / 隔离 / 定义包等项已有 UI，后端未使用
- [~] 输入框模式：Plan / Direct 已有；auto 分流未接后端
- [ ] 计划审阅视图（设计稿 / 契约编辑、任务表、改派、风险标注、汇总）
- [ ] 流水线 DAG 与看板视图、阻塞任务内联操作
- [ ] 会话视图：简报折叠、验收结果、任务报告
- [ ] 上下文条与上下文建议条
- [ ] 用量与质量看板（额度环、成本构成、首次通过率矩阵、节省、Token 面积图）
- [ ] 顶栏汇总（各 Provider 额度、本周已省、首次通过率）
- [ ] 快捷键（⌘K / ⌘N / ⌘↵ / ⌘. / ⌘1-3）
- [ ] 长日志虚拟列表、DAG 等价表格视图、`role="log"` / `progressbar`
- [ ] CSP 与链接协议限制核查

## 三、开发阶段

阶段按依赖排序：先补齐底座（存储、Adapter 抽象、防护），再依次实现规划 → 匹配 → 编排闭环，最后做上下文优化、熔断与看板。每个阶段结束时应保持应用可用。

### M1 底座重构

先把现有能力挪到设计的结构上，后续阶段才有地方落脚。

- M1a 存储：接入 SQLite（rusqlite bundled + refinery），建 `projects`、`main_sessions`、`attempts`、`chat_logs`、`usage_events`、`provider_health`、`settings`；从 localStorage 一次性迁移；前端改读后端
- M1b Adapter 抽象：抽出 `ProviderAdapter` trait、`AdapterMeta` 能力位、版本区间；现有 4 个 Adapter 迁入；实现 `FakeAdapter`
- M1c 工程结构：按需拆出 `tokage-core` / `tokage-native` / `tokage-storage` crate；内部 `DomainEvent` 总线；评估引入 specta 生成 IPC 类型

验收：现有对话功能无回归；重启后会话从 SQLite 恢复；Adapter 快照测试全绿。

### M2 执行防护与用量

- M2a `supervise`：空闲 / 硬超时双层监督、超时档位与倍数、超时后处置
- M2b OutputFilter 流水线（R1），验收命令输出复用
- M2c 完整 `TurnUsage`：缓存读写、请求次数、上下文量、来源；写入 `usage_events`
- M2d 角色权限：PLANNER / EXECUTOR / TESTER / REVIEWER 映射到各 CLI 参数；只读窗口改动检测
- M2e 残留进程清理、`nice +5`、Windows Job Object

验收：FakeAdapter 注入卡死 / 刷屏 / 孙进程场景的测试通过（R1–R3、R7）。

### M3 Provider 扩展

- M3a Antigravity CLI Adapter（先确认 headless 与结构化输出能力）
- M3b Aider Adapter（解析末尾 Tokens / Cost 行），接入按量模型
- M3c OpenCode Adapter
- M3d 原生会话透视（Claude / Codex 先行），侧边栏「外部会话」只读展示
- M3e 被动额度采集、各 CLI 错误分类处理

验收：featured CLI 全部可对话；外部会话可浏览。

### M4 规划引擎

- M4a RepoDigest：目录树、manifest、命令提取、git 热点，缓存与增量（tree-sitter 签名可放 M4a 后半）
- M4b 规划调用：固定指令 + 摘要 + 需求，按 plan-format 输出；解析与容错修复
- M4c 计划校验、落盘 `.tokage/plans/`、`.git/info/exclude`
- M4d 本地分流（auto 模式）与 DIRECT 路径
- M4e 前端计划审阅视图：设计稿 / 契约 / 任务表，编辑、确认、放弃
- M4f 增量重规划与计划版本

验收：一个多领域需求能产出通过校验的计划并在 UI 中审阅；此阶段任务仍由用户手动指定模型执行。

### M5 模型匹配引擎

- M5a 定义包格式与内置包、知识库表（`provider_profiles`、`model_knowledge_base`）
- M5b CostModel 与成功概率（先验）
- M5c 全局分配、`explanation`、改派重算；审阅视图展示模型、把握、预估成本
- M5d 规划模型选择、DIRECT 分配
- M5e 学习：`task_outcomes`、`learned_stats`、Beta 后验与用量画像（依赖 M6 产生样本，可与 M6 交错）

验收：审阅视图中每个任务都有自动分配与理由；单元测试覆盖分配公式与文档算例。

### M6 编排引擎

- M6a 任务状态机与 DAG 调度（关键路径、并发槽、写入范围冲突）
- M6b 任务简报组装、任务报告与上游产物传递
- M6c 验收关卡：允许列表、参数数组执行、范围校验（R5、R6）
- M6d 修复兜底三步、BLOCKED 处理（重新拆解 / 终端接管 / 跳过）
- M6e 计划完成：全量关卡、交付汇总
- M6f 暂停 / 恢复 / 取消 / 重试、断点续跑
- M6g 任务级 worktree 隔离与合并（可选模式）
- M6h 前端：会话树（计划 / 任务 / 执行窗口）、流水线 DAG 与看板视图、完整 `UiEvent` 通道

验收：核心闭环端到端跑通，FakeAdapter 模拟成功 / 失败 / 修复路径的集成测试通过。

### M7 上下文引擎与熔断

- M7a 窗口复用决策（任务间续用）
- M7b 窗口内 CONTINUE / COMPACT / FRESH 决策、原生压缩调用、`context.mode` 三种模式
- M7c 主会话追加消息分流
- M7d 订阅额度本地推算、主动额度查询（PTY，默认关闭）
- M7e 熔断与恢复全部触发类型、熔断后重新分配
- M7f 计划预算与预算红线（R12）、修复成本红线
- M7g 前端：上下文条、建议条、缓存倒计时

验收：文档 05 §5.1 算例由单元测试复现；熔断触发后未开始任务被自动改派。

### M8 看板、定义包更新与发布

- M8a 节省账本与 `get_usage_dashboard`
- M8b 用量与质量看板、顶栏汇总
- M8c 定义包远程更新（签名校验、可关闭）
- M8d 数据保留、清理、导出 / 清空
- M8e 快捷键、命令面板、长列表虚拟化
- M8f 可访问性实测（VoiceOver / NVDA）、CSP 核查、性能目标测量（10 §8）
- M8g 跨平台验证（Linux、Windows）与打包发布

验收：10 §8 性能目标达标；09 红线测试全绿；三平台安装包可用。

## 四、待决问题

- 前端技术栈：文档列出的 shadcn/ui、TanStack Router / Query、React Flow、Recharts 是否全部引入，还是延续自研轻量组件（M6h、M8b 前决定）。
- Crate 拆分粒度：一次拆成 9 个 crate，还是按阶段逐步拆（M1c 决定）。
- Antigravity CLI 的 headless / 结构化输出能力未确认，01 §4 注册表中对应字段标为「待确认」（M3a 调研）。
- Cursor / Copilot 的原生压缩与本地会话文件位置未确认（M3d、M7b 调研）。
