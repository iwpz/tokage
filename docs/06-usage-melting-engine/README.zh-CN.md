# 06 · 精准用量与熔断引擎 (Usage & Melting Engine)

[English](README.md) | **中文**

返回 [README](../../README.zh-CN.md)

## 1. 职责

1. 逐轮记录精确用量：输入、输出、缓存读、缓存写、金额。
2. 获取订阅制 Provider 的滚动额度（使用率、重置时间）。
3. 计划级预算：审阅时给出预估，执行中追踪实际。
4. 维护节省账本，让用户看到 Tokage 实际省了多少。
5. 依据额度、预算与健康度熔断、恢复 Provider，为成本模型提供 `scarcity`。

## 2. 计费类型

| 类型 | 例子 | 关注指标 | 成本模型中的价格 |
|------|------|---------|----------------|
| `SUBSCRIPTION` | Claude Code（Pro / Max）、Codex（ChatGPT 订阅）、Cursor（订阅）、Copilot（Premium 请求数） | 滚动窗口使用率、周额度、重置时间 | 等效 API 价格 × `scarcity` |
| `PAY_AS_YOU_GO` | Aider + DeepSeek / OpenRouter API Key | Token、美元 | 真实价格 |

## 3. 逐轮用量

每轮结束时 Adapter 给出 `TurnUsage`（[01 §5](../01-native-environment-engine/README.zh-CN.md)）：

- 写入 `usage_events`，关联执行窗口、任务、计划（`ChatRequest.planId` / `taskId` / `domain` / `attemptId`；确认计划后经 `record_plan_approved` 回填此前 `plan_id` 为空的 `PLANNING` 行）。
- `phase` 仅允许 `PLANNING` / `EXECUTION` / `REPAIR` / `CONTEXT` / `DIRECT`（与 `role` 分离；`REVIEWER` / `TESTER` 角色默认写 `EXECUTION`，流水线派发显式传 `EXECUTION`，修复轮次传 `REPAIR`）。
- 金额：`equiv_cost_usd = (input·p_in + cache_read·p_in·r + cache_write·p_in·w + output·p_out) / 1e6`，价格与缓存倍率取有效定义包候选；订阅制写 `equiv_cost_usd`（保留 CLI 的 `cost_usd`），按量制且 CLI 未给 `cost_usd` 时按同一公式写 `cost_usd`。
- 无结构化用量时解析末尾统计行；仍缺失则本地估算（`ESTIMATED`），看板上区分显示。
- 同时交给匹配引擎更新用量画像（[04 §9](../04-model-matching-engine/README.zh-CN.md)）。

## 4. 订阅额度

### 4.1 采集（按优先级）

1. 被动采集（默认，零成本）：子会话输出中出现额度或速率限制提示（含 429）时立即解析，并在工作区对应步骤以红色文字展示原文与重置时间。
2. 结构化来源：CLI 在结构化输出或本地文件中提供额度信息时直接读取（Codex 读 rollout 日志的 `rate_limits`）。需要改动 CLI 配置才能产出的来源（Claude Code statusline 输入中的 `rate_limits`），Tokage 只给出配置步骤，由用户自行修改；Tokage 只检测约定的缓存文件，读到即展示，读不到不展示。
3. CLI 内置用量命令（默认关闭）：由 CLI 自己执行官方用量命令，Tokage 只读取其输出，等同用户在 CLI 中手动输入。表中命令均须先实测，仅对「执行不消耗 Token / 额度」的命令开放，命令与实测结论写入定义包 `providers.<id>.quota`。子命令直接按参数数组执行；斜杠命令经 PTY 进入 CLI → 发送命令 → 取稳定帧 → 正则解析 → 退出。开启后频率随使用率自适应：< 50% 每 30 分钟，50–80% 每 10 分钟，> 80% 每 3 分钟；空闲时暂停。
4. 用户手动刷新：立即执行一次第 2、3 项。

| CLI | 用量命令 | 形式 | 能取到 |
|-----|---------|------|-------|
| Claude Code | `/usage` | 斜杠命令（PTY） | 5 小时 / 每周 / 按模型每周使用率、重置时间 |
| Codex CLI | `/status` | 斜杠命令（PTY） | 主 / 周窗口使用率、重置时间（第 2 项已覆盖时不执行） |
| Cursor CLI | 进入 `agent` 后 `/usage` | PTY 内置命令；不能作为启动 prompt | 当前套餐、账期 reset 标签、Included / Auto / API 百分比、On-Demand 状态 |
| Antigravity CLI | `/usage`（`/quota`） | 斜杠命令（PTY） | 按模型剩余额度、重置时间 |
| GitHub Copilot CLI | `/usage` | 斜杠命令（PTY） | 本会话用量与额度进度 |
| Kiro CLI | `/usage` | 斜杠命令（`chat --no-interactive`），是否消耗 credits 待实测 | credits 使用率、重置时间 |
| Amp | `amp usage` | 子命令 | 余额 |
| Augment Auggie | CLI 内置 credits 查询，命令待确认 | 待实测 | credits 余量 |
| Reasonix | `/status` | 斜杠命令（PTY） | PAYG 钱包余额（如 `¥29.55`）；探测不默认跑，CLI 卡片手动刷新时执行 |

未接入查询的 CLI 只走第 1、2 项；不因探测 READY 就执行未验证的斜杠命令。Cursor 查询需已有 CLI 信任目录及已确认登录的官方订阅，不自动创建信任；无内置命令补全就停止。Kiro `/usage` 已单独调查，但尚未接入此产品入口。

Tokage 自身不向厂商接口发送额度查询，也不为此读取 Token（[09](../09-guardrails/README.zh-CN.md) R11）；第 3 项的请求由 CLI 发出。

### 4.2 本地推算

两次快照之间，用该 Provider 的等效成本累积量推算使用率：

```
u_now ≈ u_last + Σ equiv_cost_since_last / quota_equiv_cost
```

`quota_equiv_cost`（一个额度窗口约等于多少等效美元）由相邻快照的使用率差与期间等效成本之比拟合，持续校准。推算值在看板上标注「推算」。

匹配引擎做全局分配时，在 `u_now` 之上叠加本计划的预计消耗（[04 §6.1](../04-model-matching-engine/README.zh-CN.md)）。

### 4.3 正则与时间解析

正则随定义包下发（`usage_patterns`）。重置时间可能是相对时间（「in 2h 13m」）或本地时刻（「3pm」），统一解析为 UTC；无法解析时保留原文显示。

## 5. 计划预算与节省账本

### 5.1 计划预算

- 审阅时：匹配引擎给出每个任务的预估成本，汇总为计划预估，并显示对各订阅额度的预计占用。
- 执行中：按任务累计实际成本；超出预估 50% 时编排引擎暂停队列并提示（[03 §3](../03-orchestration-engine/README.zh-CN.md)）。
- 完成后：预估与实际的偏差回写成本校准系数。

### 5.2 节省账本

每个计划到达交付终态时由 `settle_plan_savings_cmd` 结算，写入 `savings_events`：

| 类型 | 基准（反事实） | 实际 |
|------|-------------|------|
| `ASSIGNMENT` | 计划中所有执行任务都由「参考模型」完成的预估成本 | 各任务实际成本之和 |
| `CONTEXT` | 上下文规划中 `CONTINUE` / 新窗方案的预估成本 | 动作执行后的真实成本 + 动作本身成本 |
| `DIRECT` | 直接分发的任务由参考模型完成的预估 | 实际成本 |

- 「参考模型」默认是用户已安装模型中规划能力最高者，代表「不用 Tokage、一个强模型包办一切」的做法；可在设置中更改。结算时若 `referenceModel ≠ "auto"`，以该值作为 `user_override` 传入。
- 结算前先把每个任务的 `tasks.actual_cost_usd` 回填为该任务 `usage_events` 的 `SUM(COALESCE(cost_usd, equiv_cost_usd, 0))`；计划级实际成本同样用 `COALESCE`，因此订阅制（常只有 `equiv_cost_usd`）也不会把实际成本算成 0。
- 规划本身的成本计入实际成本，不从节省中剔除：节省 = 基准 − （规划成本 + 执行成本 + 修复成本）。规划阶段的 `usage_events` 因带 `plan_id` 自然计入。
- 基准是估算，看板上明确标注，并可展开查看参数。
- `DIRECT` / `CONTEXT` 两类事件随规划与上下文接线（F16）写入。

## 6. 熔断

### 6.1 触发条件

| 条件 | 类型 | 默认阈值 |
|------|------|---------|
| 订阅滚动额度使用率 | `QUOTA_SOFT` | ≥ 95% |
| CLI 返回额度耗尽 / 速率限制 | `QUOTA_HARD` | 立即 |
| 按量 Provider 当日 / 当月花费超出预算 | `BUDGET` | 用户设置 |
| 连续 3 次超时或进程异常 | `HEALTH` | 见 [09](../09-guardrails/README.zh-CN.md) |
| 需要登录 | `AUTH` | 立即 |

80–95% 之间不熔断，通过 `scarcity = 3.0` 让分配自然转向其他 Provider。

### 6.2 熔断效果

- 新计划与重新分配不再使用该 Provider。
- 已分配但未开始的任务：匹配引擎自动重新分配（任务尚未开始，没有上下文损失），在计划视图中标注变更。
- 运行中的任务：`QUOTA_SOFT` 继续完成当前任务；`QUOTA_HARD` / `AUTH` 时任务中断，按修复兜底第 2 步改派（[03 §8](../03-orchestration-engine/README.zh-CN.md)），简报附带已有改动清单。
- 熔断状态写入 `provider_health`，`melted_until` 取重置时间；未知时 1 小时后重新检测。

### 6.3 恢复

- 应用层总线订阅者处理 `TurnFailed` / `TurnSucceeded`：连续 3 次 `Timeout` / `NonZeroExit` / `SpawnFailed` → `HEALTH` 熔断；`RateLimited` → `QUOTA_HARD`（`melted_until` 取额度 `reset_at`，未知则 +1h）；`AuthRequired` → `AUTH`；成功清零连续失败，半开态下成功则恢复。
- Rust 周期任务（启动后立即 + 每 60 秒）扫描到期行：`QUOTA_*` 取一次额度，使用率 < 80% 才恢复，否则顺延 1 小时；`HEALTH` 置 `HALF_OPEN`（并发上限 1）。
- `provider_health.status` 所有权：provider 列表快照写入不得覆盖 `MELTED` / `HALF_OPEN`。
- `meltThreshold` 默认 0.95（与 §6.1 一致），作为 `QUOTA_SOFT` 阈值；`activeQuotaQuery` 开启后按使用率自适应轮询。
- 恢复后，未开始的任务可选择「按最新状态重新分配」；`PAUSED_QUOTA` 任务在 `provider-recovered` 时回到 `READY`。

## 7. 看板数据

```ts
type UsageDashboard = {
  range: 'today' | '7d' | '30d';
  providers: Array<{
    provider: ProviderId;
    billing: 'SUBSCRIPTION' | 'PAY_AS_YOU_GO';
    usagePct?: number;  usagePctSource?: 'SNAPSHOT' | 'ESTIMATED';
    resetAt?: string;
    spentUsd?: number;  equivCostUsd?: number;
    cacheHitRate: number;
    status: ProviderStatus;  meltReason?: MeltReason;
  }>;
  quality: {
    firstPassRate: number;                                   // 全部任务的首次通过率
    matrix: Array<{ model: string; domain: string; tasks: number; firstPassRate: number }>;
  };
  savings: { totalUsd: number; byKind: Record<'ASSIGNMENT' | 'CONTEXT' | 'DIRECT', number>; referenceModel: string };
  costSplit: { planningUsd: number; executionUsd: number; repairUsd: number };   // 规划 / 执行 / 修复占比
  tokenSeries: Array<{ bucket: string; provider: ProviderId; input: number; cacheRead: number; cacheWrite: number; output: number }>;
};
```

`costSplit` 直接反映「贵模型只做规划」是否落实：规划占比高说明计划过重，修复占比高说明匹配不准。
