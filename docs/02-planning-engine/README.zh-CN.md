# 02 · 规划引擎 (Planning Engine)

[English](README.md) | **中文**

返回 [README](../../README.zh-CN.md)

## 1. 职责

1. 分流：判断需求是直接分发一个任务，还是需要先规划。
2. 生成项目摘要（本地、零 Token），作为规划模型的主要输入。
3. 选择规划模型，以只读方式调用，产出精简的计划：设计稿 + 契约 + 任务图。
4. 校验计划结构，落盘为项目内的规约文件，交用户审阅。
5. 支持增量重规划：需求变化、任务反复失败或用户修改时，只更新受影响的部分。

## 2. 分流 (Triage)

本地完成，零 Token：

| 信号 | 倾向 |
|------|------|
| 涉及多个领域（同时提到界面与接口、数据库、部署等） | 规划 |
| 新功能 / 模块级描述（「实现」「开发」「搭建」「重构整个」） | 规划 |
| 预计涉及文件数 > 3（按提到的路径、模块名与项目摘要匹配） | 规划 |
| 单一问题（「这个报错」「解释」「改一下这个函数」） | 直接 |
| 消息很短且指向具体位置 | 直接 |

结果为 `DIRECT` 或 `PLAN`，置信度不足时默认 `PLAN` 的轻量形式（规划模型可以只返回一个任务）。用户可在输入框选择「自动 / 总是规划 / 直接执行」。

`DIRECT` 路径：由匹配引擎为这一个任务选模型，任务卡由本地生成（原始需求 + 相关文件），跳过规划模型。

## 3. 项目摘要 (RepoDigest)

规划模型最大的 Token 开销通常是「探索代码库」。Tokage 在本地预先生成摘要，让规划模型几乎不需要自己翻文件：

| 内容 | 生成方式 |
|------|---------|
| 目录树（遵守 `.gitignore`，折叠大目录，标注文件数） | 遍历 |
| 语言与框架、包管理器、主要依赖及版本 | 解析 `package.json`、`Cargo.toml`、`pyproject.toml`、`go.mod` 等 |
| 构建 / 类型检查 / 测试 / Lint 命令 | 从 manifest 脚本、Makefile、CI 配置中提取 |
| 公共符号：导出的类型、函数签名、路由定义、数据库模型 | 行前缀符号提取（每文件前 300 行），只保留签名 |
| 已有规约：项目说明文件（如 `CLAUDE.md`、`AGENTS.md`、`README`）摘要 | 截取标题与要点 |
| 近期变更热点 | `git log --stat` 最近 50 次提交 |

- 摘要按项目缓存，`notify` 监听变更后增量更新。
- 体积上限默认 8K Token，按「与需求相关度」裁剪：需求中提到的模块完整保留签名，其余只保留目录层级。
- 摘要内容稳定、放在规划输入的前部，同一项目多次规划时能命中缓存。
- **接线（F16）**：Plan 模式首轮由前端调用 `getRepoDigest` + `renderPlanningPrompt({ userRequest, repoDigest, domains })` 生成输入；`framed_prompt` 只保留只读约束一行。计划格式规约单一来源为 `tokage-planning::PLAN_FORMAT_SPEC`；格式修复走 `render_plan_repair_prompt`。`validatePlan` 传入 `allowedCommands = digest.commands` 并集。

## 4. 规划模型的选择

由匹配引擎按角色 `planner` 选择（[04 §5](../04-model-matching-engine/README.zh-CN.md)）：

- 只在规划能力达到下限的模型中选择，定义包中通常是各家最强的推理模型（如 Opus、Fable 级别）。
- 规划的输出很短、但质量决定整条流水线的成败，因此规划模型的能力下限高于任何执行任务，不以价格为首要因素。
- 用户可在设置中固定规划模型。

## 5. 规划调用

- 角色 `PLANNER`：只读权限，允许读文件与搜索，不允许写入或执行命令。
- 输入：固定的规划指令（缓存友好）+ 项目摘要 + 用户需求 + 可用领域列表（不含具体模型，规划模型不负责选模型）。
- 读取预算：允许按需读取少量关键文件；指令中要求优先依赖摘要。
- 输出上限：默认 6K Token，要求只写「约束」，不写实现代码；契约以签名、类型、表结构形式给出。
- 超时档位 `PLANNING`（见 [09 §2](../09-guardrails/README.zh-CN.md)）。

完整的输出格式与规划指令模板见 [plan-format.md](plan-format.zh-CN.md)。

## 6. 计划的结构

```
Plan
 ├─ 设计稿 (spec)：目标、范围、关键决策、非目标（精简 Markdown）
 ├─ 契约 (contracts)：API 定义、数据结构、组件 Props、目录与命名约定
 └─ 任务图 (tasks)：
      T1 [backend-api]  依赖: —      写入: src/server/**   验收: cargo test -p server
      T2 [frontend-ui]  依赖: —      写入: web/src/**      验收: pnpm typecheck
      T3 [integration]  依赖: T1,T2  写入: web/src/api/**  验收: pnpm test api
      T4 [testing]      依赖: T3     写入: tests/**        验收: pnpm test && cargo test
```

每个任务包含：领域、标题、目标、依赖、引用的契约、写入范围、交付物、验收命令与标准、规模（S/M/L）、难度（1–3）。

落盘：

- 当前实现：每个会话的计划写入 `.tokage/plans/<session-id>.md` 并保留历史版本，目录结构与读写边界见 [08 §1.1](../08-storage/README.zh-CN.md)。
- 计划结构化后（P5）写入 `.tokage/plans/<plan_id>/`：`spec.md`、`contracts/*`、`tasks.yaml`。
- 执行模型通过读文件获取契约，而不是把契约全文塞进每个任务简报；契约是所有并行任务共同遵守的单一事实来源。
- 用户可在设置中改为不落盘（契约改为内联进简报，Token 成本更高）。

## 7. 计划校验

规划输出经本地校验后才进入审阅：

| 校验 | 失败处理 |
|------|---------|
| 结构可解析（YAML / Markdown 段落齐全） | 本地容错修复（补全缺失字段默认值）；仍失败则请求规划模型只重发任务图 |
| 任务图无环、依赖引用存在 | 同上 |
| 领域属于定义包的领域表 | 映射到最近领域，或标为 `general` |
| 并行任务（无依赖关系）的写入范围不重叠 | 自动添加依赖使其串行，或提示用户 |
| 每个任务至少一条验收标准 | 用项目摘要中的默认构建 / 测试命令补全 |
| 验收命令匹配允许列表 | 不匹配的命令标红，需用户确认（[09 §5](../09-guardrails/README.zh-CN.md)） |
| 任务数量 ≤ 20 | 超出时提示拆分为多个阶段 |

## 8. 用户审阅

计划呈现为：设计稿、契约、任务图（含匹配引擎给出的模型分配与预估成本）。用户可以：

- 直接确认执行。
- 编辑任务（合并、拆分、改依赖、改写入范围）、编辑契约。
- 为任务指定模型。
- 提出修改意见，触发增量重规划。

**确认计划时**（F1.f / S6）：重新计算一次模型分配（手选作为 `overrides`），把每个任务的 `assigned_model` 写回为 `<ProviderId>:<cliModelArg>`（例如 `CLAUDE_CODE:sonnet`），持久化计划，并把完整的 `TaskAssignment` 快照交给流水线状态供学习与结算使用。无法解析的分配在派发时使任务 `BLOCKED`，不再静默回退到「第一个可用模型」。

设置项 `requireReview` / `planningAutoApproveBelowUsd`：校验通过且无 Error 时，若关闭强制审阅或总成本低于阈值，审阅界面自动确认（可在生效前撤销）。`plannerModel` 非 `"auto"` 时 Plan 首轮固定该模型。

## 9. 增量重规划

触发：用户追加了范围外的新需求、修复兜底升级到规划层（[03 §8](../03-orchestration-engine/README.zh-CN.md)）、用户主动要求（计划页「提出修改意见」，或主会话分流命中重规划关键词）。

- 输入：当前版本的设计稿与契约（通常仍命中缓存）+ 受本次变更影响的已完成任务报告 + 变更说明。不受影响的任务只给出编号、标题与状态。由 `assembleReplanPrompt` 组装。
- 输入总量受 `planning.replan_input_max_tokens` 约束；超出时先把报告裁剪为「完成内容」一段，再按与变更的相关度丢弃报告。
- 输出：经 Plan 轮次解析后的完整提案，再 `computePlanDiff` 得到差异；UI 以 `PlanDiffView` 展示，确认后 `applyPlanDiff`（版本 +1）并 `generateAdaptationTasks`。
- 已完成且不受影响的任务保持不变；受契约变更影响的已完成任务生成「适配任务」。
- 应用差异后，设计稿与契约整体改写为最新版本，不以追加修订说明的方式变长。每次重规划产生新的计划版本，旧版本只存档可查，不进入后续重规划的输入。仅允许在流水线未启动或已暂停时重规划。

## 10. 配置项

| 键 | 默认 | 说明 |
|----|------|------|
| `planning.mode` | `auto` | `auto` / `always` / `direct` |
| `planning.planner_model` | 自动 | 固定规划模型 |
| `planning.digest_max_tokens` | `8000` | 项目摘要上限 |
| `planning.output_max_tokens` | `6000` | 规划输出上限 |
| `planning.replan_input_max_tokens` | `16000` | 增量重规划输入上限（不含项目摘要） |
| `planning.max_tasks` | `20` | |
| `planning.write_to_project` | `true` | 计划与契约是否落盘到 `.tokage/` |
| `planning.auto_approve_below_usd` | 关闭 | |
