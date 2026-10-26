# 上下文决策闭环 · 排期 Todo

返回 [05 · 上下文生命周期引擎](README.md)

目标：把 CONTINUE / COMPACT / FRESH 从「库 + 半截 suggest」跑成可校准的生产决策。  
前置：库与 F16.e 骨架已在；本表只跟 **尚未完成** 的接线与产品行为。

勾选约定：子项全部完成后再勾父阶段；验收项不过不勾阶段。

---

## 总览

| 阶段 | 目标 | 完成判据 |
|------|------|----------|
| **M1** | 主会话 suggest 决策可稳定算出并执行 COMPACT | 真实参数进规划器；COMPACT 按当前 provider 发出可执行载荷；结算口径可比 |
| **M2** | `auto` / `FRESH` / 原生压缩路径齐备 | 三种 `context.mode` 与文档一致；FRESH 真开新窗；有 Adapter compact 能力时走原生 |
| **M3** | 任务执行窗 + 复用 + 校准 | 长任务多轮也走同一规划器；显式目标任务；复用参数对齐知识包；节省回写可校准 |

---

## M1 · 最小可跑通（主会话 suggest + 真实参数 + COMPACT）

- [ ] **M1** 主会话在 suggest 下能稳定出现建议，采纳后下一轮真正压缩，并留下可比的 CONTEXT 结算

### M1.a 规划调用不再静默失败

- [x] **M1.a1** `InWindowContextParams`（及如需的 `WindowReuseParams`）补 `#[serde(default)]` 或与前端契约对齐，缺字段不反序列化失败
- [x] **M1.a2** `Workspace` 调 `planInWindowContext` 时去掉「失败 → `setActivePlan(null)` 无日志」的吞错；至少 `console.warn` + 可观测原因
- [x] **M1.a3** 前端 mock / 非 Tauri 路径与 Rust 默认值语义对齐（避免测绿、端上永远无建议）

### M1.b 规划输入接真实状态（去掉硬编码）

- [x] **M1.b1** `historyTokens` / `baselineTokens`：优先 CLI 上报的 `contextTokens`；缺省时用可解释的估算，并记录来源
- [x] **M1.b2** `effectiveContext` / `contextWindow` / `cliAutoCompactRatio` / `compactOutputRatio` 从知识包 · provider/model 填入（不再只靠模型名字符串猜 `maxTokens`）
- [x] **M1.b3** `pIn` / `pOut` / 缓存倍率 `r`/`w` 从当前会话模型成本模型填入
- [x] **M1.b4** `cacheType` / TTL：统一 `cachePolicy` 与 Rust `is_cache_warm_now` 的枚举（如 `ephemeral` vs `standard`）
- [x] **M1.b5** `remainingTurns` / `growthPerTurn` / `apiCallsPerTurn`：有会话统计则用滑动估计，否则用按 provider 的默认表（文档化），禁止魔法常量散落

M1.b 实现与离线验证：见 [真实输入说明](m1b-inputs.md)。当前主会话自动建议/压缩仍关闭；仅接入真实输入和只读规划预览，不将旧 M1.a 状态追认为当前完整建议链已通过。M1.c 及后续执行/结算验收保持待办。

### M1.c 冷却与动作历史

- [ ] **M1.c1** 按 `attemptId` / session 持久或内存跟踪 `turnsSinceLastCompact`
- [ ] **M1.c2** 规划传入该字段，使「两次非 CONTINUE 至少间隔 2 轮」生效
- [ ] **M1.c3** 采纳 COMPACT/FRESH 后重置计数；CONTINUE 递增

### M1.d COMPACT 载荷与当前会话对齐

- [ ] **M1.d1** `buildCompact` 的 `providerName` 取自当前 thread/provider，禁止写死 `CLAUDE_CODE`
- [ ] **M1.d2** `taskGoal` / `contractPaths` / `pendingErrors` 从当前会话或活跃任务真实状态组装（主会话无任务时用会话目标/最近错误摘录）
- [ ] **M1.d3** 按 `CompactMethod` 分流执行意图：`NATIVE_SLASH_COMMAND` 记下 command；`INSTRUCTION_PROMPT` / `SYNTHESIZED_FRESH_BRIEF` 用对应 prompt（M1 至少保证后两者在主会话下一轮生效）

### M1.e 执行路径（主会话）

- [ ] **M1.e1** 明确并实现 M1 策略：无原生 compact 时 = instruction prompt 作为独立一轮或下一轮前缀（与现 `pendingCompact` 二选一写清，避免「伪压缩」）
- [ ] **M1.e2** 有 `payload.command` 时：要么本阶段跳过并在 UI 标明「需 M2 原生路径」，要么最小可用地经 PTY/slash 发出（不得假装已压缩）
- [ ] **M1.e3** 采纳后系统消息区分「已排队压缩」vs「已执行压缩」

### M1.f 结算口径

- [ ] **M1.f1** 重定义 settle：用**同一 horizon**（例如「压缩后首轮实际 vs 未压缩首轮反事实」或「规划器给出的 continue/compact 差额 × 校准系数」），禁止「多轮 CONTINUE 估价 − 单轮实际」
- [ ] **M1.f2** `settle_context_savings` + `CONTEXT` `savings_events` 按新口径写；看板数字可解释
- [ ] **M1.f3** 单测覆盖口径边界（无节省、负差额夹零、缺 usage）

### M1.g M1 验收

- [ ] **M1.g1** 单元：缺字段反序列化成功且默认值合理
- [ ] **M1.g2** 单元/集成：冷长历史 → COMPACT；热短历史 → CONTINUE（参数来自 fixture，非前端硬编码）
- [ ] **M1.g3** 手动：suggest 采纳后下一轮可见压缩载荷；结束后有 CONTEXT 节省记录且数量级合理
- [ ] **M1.g4** 文档：05 §7 suggest 行为与实现差异消除（或注明 M1 限制）

---

## M2 · 模式与动作齐备

- [ ] **M2** `auto` / `suggest` / `off` 与文档一致；FRESH 真开新窗；原生 compact 可走 Adapter

### M2.a `context.mode = auto`

- [ ] **M2.a1** 推荐非 CONTINUE 时自动 `record` + `apply`（无需点建议条）
- [ ] **M2.a2** 「下一轮开始前可撤销」：UI 入口 + `UNDONE` 状态回写 + 取消 pending compact/fresh
- [ ] **M2.a3** 默认值与文档对齐（`auto` vs 当前设置默认 `suggest`）并做迁移说明

### M2.b `FRESH`

- [ ] **M2.b1** 采纳/自动 FRESH：旧窗拉进展报告（热缓存优先模型报告；冷则本地抽取，见 task-brief §3）
- [ ] **M2.b2** 新窗首条 = 重装简报/会话摘要 + 进展报告 + 本条用户消息
- [ ] **M2.b3** `apply_context_action` 写入 `newAttemptId`；建议条文案与成功/失败反馈
- [ ] **M2.b4** FRESH 后结算与 COMPACT 共用可比口径

### M2.c 原生 Adapter compact

- [ ] **M2.c1** `Adapter::build_compact`（或等价）接入生产 turn/PTY 路径
- [ ] **M2.c2** 能力位 `COMPACT_HEADLESS` / `COMPACT_PTY` 按已测 CLI 标注；无能力则回退 M1 prompt 路径
- [ ] **M2.c3** Claude `/compact [指示]` 等原生命令端到端（录制或 live skip-if-absent）
- [ ] **M2.c4** Codex 等 `SYNTHESIZED_FRESH_BRIEF` 与 M2.b 策略对齐（避免两套「新开」）

### M2.d 状态机补齐

- [ ] **M2.d1** `FAILED`：执行压缩/开窗失败时落库并提示重试/忽略
- [ ] **M2.d2** 同一窗口动作历史可 `list_context_actions` 供调试或设置页只读查看（可选）

### M2.e M2 验收

- [ ] **M2.e1** auto：建议出现后未点击也会在发送前挂上动作；撤销后本轮不压缩
- [ ] **M2.e2** FRESH：新旧 attempt/thread 可区分；进展报告进入新窗首包
- [ ] **M2.e3** 有原生能力的 provider 走 slash/命令，无能力走 prompt，UI 不谎报

---

## M3 · 任务窗、复用与校准

- [ ] **M3** 任务执行多轮与主会话共用决策；复用参数真实化；节省可校准规划器

### M3.a 任务执行窗口内规划

- [ ] **M3.a1** 长任务多轮 / 任务窗追加对话前调用同一 `plan_context_action`
- [ ] **M3.a2** follow-up 注入（`takePendingTaskFollowUp`）与上下文动作的顺序写清（先 compact 再注入 vs 反之）
- [ ] **M3.a3** 任务窗 suggest/auto UI（看板或任务详情）或复用 ContextSuggestionBar 变体

### M3.b 主会话分流产品缺口

- [ ] **M3.b1** Composer「显式选择目标任务」→ 传 `explicitTaskId` 给 `triage_main_session_message`
- [ ] **M3.b2** 分流到任务窗后，可选触发该窗的一次上下文规划

### M3.c 窗口复用参数对齐

- [ ] **M3.c1** `findReusableWindow` 的价、窗口、TTL、`minSaving` 从知识包/设置注入（去掉 25K/3/15 硬编码）
- [ ] **M3.c2** 历史 token 估计改进（usage / artifact，而非固定 15K）
- [ ] **M3.c3** worktree / 权限 / 角色变化时强制新窗（与 R9 一致，补测）

### M3.d 与匹配引擎叙事对齐

- [ ] **M3.d1** 分配阶段的「复用加成」与运行时 `evaluateWindowReuse` 使用同一套成本假设（或文档标明差异）
- [ ] **M3.d2** 复用成功/失败写入 `context_actions`（kind `REUSE`）便于账本与调试

### M3.e 校准闭环

- [ ] **M3.e1** 用已结算 CONTEXT 事件回写 `estimate_calibration` / 相关知识字段（或本地覆盖表）
- [ ] **M3.e2** 设置项补齐并接线：`brief_max_tokens` / `report_max_tokens` / `rework_phi`（05 §9）
- [ ] **M3.e3** 看板：CONTEXT 节省可下钻到 action id / attempt

### M3.f M3 验收

- [ ] **M3.f1** 流水线长任务：接近硬阈值前主动 COMPACT/FRESH，且不破坏 prompt cache 纪律（R9/R10）
- [ ] **M3.f2** 显式目标任务分流 E2E
- [ ] **M3.f3** 复用：同模型相邻 DONE 任务在热缓存下续 thread；冷或无关领域开新窗
- [ ] **M3.f4** 05 文档与实现同步（§4–§9、接线日期）

---

## 明确不在本表（避免膨胀）

- 重新设计任务简报模板或主会话「无限历史压缩」
- 为保持缓存热度而发空请求（05 §8 禁止）
- 替换各 CLI 官方自动 compact；本表只做「在其触发前主动决策」

---

## 进度快照

| 阶段 | 状态 | 备注 |
|------|------|------|
| M1 | M1.a 历史完成；M1.b 真实输入已接入 | 冷却、载荷、执行和结算仍待后续；自动建议未开放 |
| M2 | 未开始 | 依赖 M1 载荷与结算口径 |
| M3 | 未开始 | 可与 M2.c 部分并行，但验收在 M2 后 |
