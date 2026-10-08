# 模型定义包格式

返回 [04 · 模型匹配引擎](README.md)

定义包由 Tokage 维护者发布，随应用内置并可在线更新。所有价格、缓存与评分数值都是定义包字段，按厂商官方文档与评测维护；下文数值仅为结构示例。

## 1. 顶层结构

```json
{
  "schema_version": 1,
  "pack_version": "2026.10.01",
  "domains": ["planning", "frontend-ui", "backend-api", "data-model", "algorithm",
              "integration", "refactor", "infra", "testing", "docs", "review", "general"],
  "success_model": { "a": 1.6, "b": 1.2, "prior_strength": 6,
                     "req": { "1": { "S": 2.0, "M": 2.5, "L": 3.0 },
                              "2": { "S": 3.0, "M": 3.5, "L": 4.0 },
                              "3": { "S": 4.0, "M": 4.3, "L": 4.5 } } },
  "providers": { },
  "models": { },
  "usage_profiles": { },
  "triage_rules": [ ],
  "usage_patterns": { },
  "noise_rules": [ ],
  "gate_allowlist": [ ]
}
```

## 2. providers

```json
"CLAUDE_CODE": {
  "billing": "SUBSCRIPTION",
  "cache": {
    "mode": "explicit_prefix",
    "ttl_seconds": 300,
    "read_multiplier": 0.1,
    "write_multiplier": 1.25,
    "shared_baseline": true
  },
  "context": { "cli_auto_compact_ratio": 0.92, "compact": "native", "compact_output_ratio": 0.06 },
  "concurrency_hint": 2,
  "models": [
    { "model": "claude-opus-5-5", "cli_model_arg": "opus" },
    { "model": "claude-haiku-4-5", "cli_model_arg": "haiku" }
  ]
}
```

`providers.<id>.models` 列出该 CLI 提供的模型，每项只写随渠道变化的字段：

| 字段 | 说明 |
|------|------|
| `model` | 引用 `models` 中的键 |
| `cli_model_arg` | 传给 CLI `--model` 的原样字符串 |
| `context_window` / `effective_context` | 可选，CLI 对上下文的限制（如 Codex 为 272K），只能小于模型自身的值 |
| `credit_multiplier` | 可选，积分制 CLI 中该模型相对基准的消耗倍率 |
| `cli_model_arg_verified` | 为 `false` 时参数未在真实 CLI 目录中核对，Adapter 以 CLI 返回的模型列表为准 |
| `skills` | 可选，该渠道的领域分数修正，按领域覆盖模型的值 |

| `cache.mode` | 含义 |
|--------------|------|
| `explicit_prefix` | CLI 显式设置缓存断点，命中稳定，通常有写入溢价、TTL 较短 |
| `automatic_prefix` | 服务端自动按前缀缓存，无写入溢价 |
| `implicit` | 隐式缓存，命中不保证，配合 `hit_reliability` 使用 |
| `disk_persistent` | 磁盘级缓存，保留数小时以上 |
| `none` | 无缓存 |
| `by_model` | 由底层模型决定（多模型 CLI），读取模型条目中的 `cache` |

## 3. models

以模型 ID 为键，记录与渠道无关的模型固有数据。同一模型被多个 CLI 提供时只写一份，渠道差异写在 `providers.<id>.models`（§2）。

```json
"claude-opus-5-5": {
  "display_name": "Claude Opus 5.5",
  "vendor": "anthropic",
  "roles": ["planner", "executor", "reviewer"],
  "skills": {
    "planning": 5, "frontend-ui": 4, "backend-api": 5, "data-model": 5, "algorithm": 5,
    "integration": 5, "refactor": 5, "infra": 4, "testing": 4, "docs": 4, "review": 5, "general": 5
  },
  "price_per_mtok": { "input": 0.0, "output": 0.0 },
  "context_window": 200000,
  "effective_context": 120000,
  "speed": 2,
  "tags": ["high-reasoning", "architecture"],
  "description": "适合规划、架构约束与高难度任务。"
}
```

| 字段 | 说明 |
|------|------|
| `roles` | 可担任的角色；`planner` 只授予规划专长最高的一档模型 |
| `skills` | 各领域 1–5 分，成功概率先验的主要输入 |
| `price_per_mtok` | 官方 API 价格；订阅制 Provider 以此作为等效价格 |
| `context_window` / `effective_context` | 模型自身的上下文窗口与质量开始明显下降的位置，上下文规划的硬约束 |
| `cache` / `context` | 可选，覆盖 Provider 级默认值 |
| `cache_rates` | 可选，`by_model` Provider 下该模型的缓存读 / 写倍率 |
| `long_context_pricing` | 可选，`{threshold, input_multiplier, output_multiplier}`：输入超过阈值时整次请求按倍率计费 |
| `price_schedule` | 可选，分时价格（如高峰时段倍率与 UTC 时段） |
| `max_output` / `released` / `vendor` | 参考信息，不参与匹配 |
| `benchmarks` | 参考跑分，仅供展示与人工校对，不直接参与计算 |
| `price_estimated` / `skills_estimated` | 价格或领域分数无官方数据、为估算值；界面标注「估算」 |

合并后的视图把每个 `(provider, model)` 展开为一个候选：模型条目为底，渠道条目逐字段覆盖。匹配引擎只读取展开后的候选。

## 4. usage_profiles

按 `(domain, size)` 给出执行窗口的用量画像初值，本地学习后逐步替换：

```json
"usage_profiles": {
  "frontend-ui": { "S": { "turns": 1, "calls_per_turn": 8,  "growth_per_turn": 10000, "output_per_turn": 3000 },
                   "M": { "turns": 2, "calls_per_turn": 14, "growth_per_turn": 18000, "output_per_turn": 6000 },
                   "L": { "turns": 3, "calls_per_turn": 20, "growth_per_turn": 25000, "output_per_turn": 9000 } },
  "testing":     { "S": { "turns": 1, "calls_per_turn": 6,  "growth_per_turn": 8000,  "output_per_turn": 3000 } }
}
```

## 5. 其他规则表

| 键 | 用途 | 使用方 |
|----|------|-------|
| `triage_rules` | 分流关键词、正则与权重 | [02 §2](../02-planning-engine/README.md) |
| `domain_keywords` | 领域推断关键词（分流的多领域判断与直接分发的领域推断） | [02 §2](../02-planning-engine/README.md) |
| `usage_patterns` | 订阅额度文本的解析正则，按 Provider 分组，`_generic` 适用于全部 | [06 §4](../06-usage-melting-engine/README.md) |
| `time_formats` | 重置时间的相对时间 / 本地时刻正则 | [06 §4.3](../06-usage-melting-engine/README.md) |
| `providers.<id>.quota` | 额度窗口类型与免费查询命令 | [06 §4](../06-usage-melting-engine/README.md) |
| `noise_rules` | 输出噪声过滤规则 | [09 §1](../09-guardrails/README.md) |
| `gate_allowlist` | 验收命令允许列表模式（`check_gate_command_safety` / `validate_plan` / 执行 gate 的唯一来源；不在名单且未 `confirmedCommands` 的命令被拒） | [09 §5](../09-guardrails/README.md)、[03 §5](../03-orchestration-engine/README.md) |
| `command_policy` / `forbidden_paths` | 指令名单分级、凭据路径 | [11 §3](../11-command-authorization/README.md) |
| `credential_env` / `providers.<id>.env_allow` | 凭据变量裁剪 | [11 §6.3](../11-command-authorization/README.md) |
| `redaction_patterns` | 输出脱敏的常见 Key 格式 | [11 §6.4](../11-command-authorization/README.md) |
| `sandbox_profiles` | 每个 CLI 的沙箱画像 | [11 §5.2](../11-command-authorization/README.md) |
| `providers.<id>.proxy` | 代理识别情况 | [12 §2](../12-network-proxy/README.md) |

## 6. 分层与本地覆盖

定义包按层合并，高层覆盖低层：

| 层 | 来源 | 可编辑 |
|----|------|-------|
| `builtin` | 随安装包内置 | 否 |
| `official` | 在线更新（[04 §10](README.md)），存在时替代 `builtin` | 否 |
| `custom` | 用户导入的自定义定义包，可多个，按顺序合并 | 导入 / 移除 / 排序 |
| `override` | 设置中逐字段覆盖，存于 `model_overrides`（JSON Merge Patch） | 是 |
| `learned` | 本地学习数据（成功概率后验、用量画像、校准系数） | 只能重置 |

- 自定义定义包使用同一 schema，可只包含部分模型或字段；不需要签名，但导入时校验 schema，并在界面标注「未签名」。
- 安全相关键（`command_policy.forbidden`、`forbidden_paths`、`credential_env`、`sandbox_profiles`）在 `custom` / `override` 层只能追加或收紧，不能删除或放宽。
- 自定义层覆盖 `models.<id>` 时所有渠道一起生效；覆盖 `providers.<pid>.models` 中的条目只影响该渠道，按 `model` 匹配。
- 合并结果 `effective` 是匹配引擎唯一读取的视图。

## 7. 查看

设置 → 模型能力页，内置与自定义定义包使用同一套视图：

- 层选择：`effective`（默认）/ `builtin` / `official` / 每个 `custom` 包；`builtin` 与 `official` 只读，始终可查看。
- 模型列表：每个模型一行，显示角色、价格、上下文窗口 / 有效上下文、速度、标签、描述，以及提供该模型的 CLI；展开后逐个 CLI 列出模型参数、上下文限制、计费方式与是否已核对。
- 模型详情：领域专长雷达或表格（1–5）、缓存特性、`usage_profiles`；每个字段标注来源层（如「builtin · 被 custom:my-pack 覆盖」），可展开查看各层原值。
- 学习数据并列显示：每个领域的本地首次通过率与样本数，与先验分数对照。
- 规则表（领域、分流规则、指令名单、沙箱画像、代理识别）以只读表格展示。
- 从任意层「复制为覆盖」或「导出为自定义包」作为编辑起点；查看不会修改任何层。
