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
  "models": [ ],
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
  "concurrency_hint": 2
}
```

| `cache.mode` | 含义 |
|--------------|------|
| `explicit_prefix` | CLI 显式设置缓存断点，命中稳定，通常有写入溢价、TTL 较短 |
| `automatic_prefix` | 服务端自动按前缀缓存，无写入溢价 |
| `implicit` | 隐式缓存，命中不保证，配合 `hit_reliability` 使用 |
| `disk_persistent` | 磁盘级缓存，保留数小时以上 |
| `none` | 无缓存 |
| `by_model` | 由底层模型决定（多模型 CLI），读取模型条目中的 `cache` |

## 3. models

```json
{
  "model_id": "claude-opus-5-5",
  "provider": "CLAUDE_CODE",
  "cli_model_arg": "opus",
  "display_name": "Claude Opus 5.5",
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
| `cli_model_arg` | 传给 CLI `--model` 的原样字符串 |
| `roles` | 可担任的角色；`planner` 只授予规划专长最高的一档模型 |
| `skills` | 各领域 1–5 分，成功概率先验的主要输入 |
| `price_per_mtok` | 订阅制填等效 API 价格 |
| `effective_context` | 质量开始明显下降的位置，上下文规划的硬约束 |
| `cache` / `context` | 可选，覆盖 Provider 级默认值 |

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
| `usage_patterns` | 订阅额度文本的解析正则 | [06 §4](../06-usage-melting-engine/README.md) |
| `noise_rules` | 输出噪声过滤规则 | [09 §1](../09-guardrails/README.md) |
| `gate_allowlist` | 验收命令允许列表模式 | [09 §5](../09-guardrails/README.md) |

## 6. 本地覆盖

用户可在设置中覆盖任意模型或 Provider 字段，存于 `model_overrides`（JSON Merge Patch），优先级：本地学习数据 > 用户覆盖 > 定义包。
