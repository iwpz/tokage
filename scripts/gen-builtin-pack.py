#!/usr/bin/env python3
"""Generates src-tauri/resources/knowledge/builtin.json.

Usage: python3 scripts/gen-builtin-pack.py src-tauri/resources/knowledge/builtin.json
"""
import json, sys

D = ["planning", "frontend-ui", "backend-api", "data-model", "algorithm", "integration",
     "refactor", "infra", "testing", "docs", "review", "general"]

def sk(*v):
    assert len(v) == 12, v
    return dict(zip(D, v))

# ---------------------------------------------------------------- base models
# price: (input, output) USD / 1M; cache: (read_mult, write_mult) on input price.
M = {}
def base(mid, name, vendor, skills, price, ctx, eff, speed, tags, desc, released,
         cache=None, bench=None, long_ctx=None, max_output=None, est=False, extra=None):
    e = dict(display_name=name, vendor=vendor, skills=skills,
             price_per_mtok={"input": price[0], "output": price[1]},
             context_window=ctx, effective_context=eff, speed=speed, tags=tags,
             description=desc, released=released)
    if max_output: e["max_output"] = max_output
    if cache: e["cache_rates"] = {"read_multiplier": cache[0], "write_multiplier": cache[1]}
    if long_ctx: e["long_context_pricing"] = dict(zip(["threshold", "input_multiplier", "output_multiplier"], long_ctx))
    if bench: e["benchmarks"] = bench
    if est: e["price_estimated"] = True
    if extra: e.update(extra)
    M[mid] = e

# Anthropic — platform.claude.com pricing / models overview (2026-10)
base("claude-fable-5-1", "Claude Fable 5.1", "anthropic",
     sk(5, 5, 5, 5, 5, 5, 5, 4.5, 5, 5, 5, 5), (10, 50), 1_000_000, 500_000, 2,
     ["high-reasoning", "architecture", "long-context", "premium"],
     "当前最强档。适合规划、跨模块架构约束与最高难度任务；价格最高，执行任务只在其他模型达不到把握门槛时使用。",
     "2026-09-01", cache=(0.025, 1.25), max_output=128_000,
     bench={"terminal_bench_4": 55.8, "osworld_2_strict": 41.7, "hle_tools": 65.0, "cursorbench_3_2": 73.4})
base("claude-fable-5", "Claude Fable 5", "anthropic",
     sk(5, 4.5, 5, 5, 5, 5, 5, 4, 4.5, 5, 5, 5), (10, 50), 1_000_000, 450_000, 2,
     ["high-reasoning", "architecture", "premium", "previous-gen"],
     "上一代旗舰，SWE-bench 系列分数仍处第一梯队；同价位下 Fable 5.1 缓存读取更便宜，优先选 5.1。",
     "2026-06", cache=(0.1, 1.25), max_output=128_000,
     bench={"swe_bench_verified": 95.0, "swe_bench_pro": 80.0, "terminal_bench_4": 42.0})
base("claude-opus-5-5", "Claude Opus 5.5", "anthropic",
     sk(5, 4.5, 5, 5, 5, 5, 5, 4.5, 4.5, 4.5, 5, 5), (4, 20), 1_000_000, 500_000, 3,
     ["high-reasoning", "architecture", "long-context", "cost-efficient-flagship"],
     "规划与高难度执行的首选：能力接近 Fable，价格不到一半，缓存读取 0.05×。Claude Code 的默认模型。",
     "2026-09-22", cache=(0.05, 1.25), max_output=128_000,
     bench={"terminal_bench_4": 66.4, "osworld_2": 81.8, "frontiercode_1_1": 54.4, "cursorbench_4": 57.8})
base("claude-opus-5", "Claude Opus 5", "anthropic",
     sk(4.5, 4.5, 5, 4.5, 4.5, 4.5, 5, 4.5, 4.5, 4.5, 5, 4.5), (5, 25), 1_000_000, 400_000, 2,
     ["high-reasoning", "previous-gen"],
     "上一代 Opus。比 Opus 5.5 贵且慢，仅在锁定版本时使用。",
     "2026-07-24", cache=(0.1, 1.25), max_output=128_000,
     bench={"swe_bench_verified": 96.0, "swe_bench_pro": 79.2, "terminal_bench_4": 52.3, "osworld_2": 70.6, "arc_agi_3": 30.16})
base("claude-opus-4-8", "Claude Opus 4.8", "anthropic",
     sk(4, 4, 4.5, 4.5, 4, 4.5, 4.5, 4, 4, 4.5, 4.5, 4.5), (5, 25), 1_000_000, 400_000, 3,
     ["previous-gen"],
     "旧版 Opus，能力已被 Sonnet 5.5 覆盖，价格更高。",
     "2026-05", cache=(0.1, 1.25), max_output=128_000)
base("claude-sonnet-5-5", "Claude Sonnet 5.5", "anthropic",
     sk(4.5, 4.5, 5, 4.5, 4.5, 4.5, 4.5, 5, 4.5, 4.5, 4.5, 4.5), (2, 10), 1_000_000, 500_000, 4,
     ["workhorse", "agentic", "terminal", "long-context"],
     "高性价比主力执行模型。Terminal-Bench 4.0 分数高于 Opus 5.5，适合绝大多数开发、测试与基础设施任务。",
     "2026-09-28", cache=(0.1, 1.25), max_output=128_000,
     bench={"terminal_bench_4": 70.6, "osworld_2_1": 80.1, "frontiercode_1_1": 46.2, "cursorbench_4": 55.5})
base("claude-sonnet-5", "Claude Sonnet 5", "anthropic",
     sk(4, 4, 4.5, 4, 4, 4, 4, 3.5, 4, 4, 4, 4), (2, 10), 1_000_000, 400_000, 4,
     ["workhorse", "previous-gen"],
     "上一代 Sonnet，与 Sonnet 5.5 同价；终端类任务明显弱于 5.5。",
     "2026-06", cache=(0.1, 1.25), max_output=128_000,
     bench={"terminal_bench_4": 10.3})
base("claude-haiku-4-5", "Claude Haiku 4.5", "anthropic",
     sk(2.5, 3, 3, 3, 2.5, 2.5, 2.5, 3, 3, 3.5, 3, 3), (1, 5), 200_000, 120_000, 5,
     ["fast", "cheap", "small"],
     "快速低价，适合文档、小范围测试补全与简单修改。上下文 200K；2026-10-15 起进入退役窗口。",
     "2025-10-15", cache=(0.1, 1.25), max_output=64_000)

# OpenAI — developers.openai.com pricing / prompt caching (2026-10). >272K input bills 2× in, 1.5× out.
OA_LONG = (272_000, 2.0, 1.5)
base("gpt-6-astra", "GPT-6 Astra", "openai",
     sk(5, 4.5, 5, 5, 5, 5, 5, 5, 4.5, 4.5, 5, 5), (10, 50), 1_050_000, 500_000, 1,
     ["high-reasoning", "architecture", "premium", "slow-first-token"],
     "OpenAI 最强档。推理与算法顶尖（GPQA 96.1、ARC-AGI-2 95.0），首 token 延迟很长；适合规划与最难任务。",
     "2026-09-03", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"deepswe_1_1": 74.0, "terminal_bench_2_1": 87.4, "gpqa_diamond": 96.1, "arc_agi_2": 95.0})
base("gpt-6.1-sol", "GPT-6.1 Sol", "openai",
     sk(4.5, 4.5, 5, 4.5, 5, 4.5, 4.5, 5, 4.5, 4, 4.5, 4.5), (2, 10), 1_050_000, 500_000, 3,
     ["workhorse", "agentic", "terminal", "cost-efficient-flagship"],
     "OpenAI 当前主力：DeepSWE 接近 Astra，成本约五分之一，缓存读取 0.05×。适合高难度后端、算法与基础设施任务。",
     "2026-09-29", cache=(0.05, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"deepswe_1_1_delta_vs_gpt_6_sol": 6.4, "osworld_2_delta_vs_astra": -2.1})
base("gpt-6-sol", "GPT-6 Sol", "openai",
     sk(4, 4, 4.5, 4, 4.5, 4, 4, 4.5, 4, 4, 4, 4), (2, 10), 922_000, 400_000, 3,
     ["workhorse", "previous-gen"],
     "已被 GPT-6.1 Sol 以同价替代，缓存读取更贵。",
     "2026-09-22", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"deepswe_1_1": 68.8, "osworld_2": 60.5})
base("gpt-6-luna", "GPT-6 Luna", "openai",
     sk(3, 3, 3.5, 3, 3, 3, 3, 3.5, 3.5, 3, 3, 3), (0.1, 0.5), 922_000, 128_000, 4,
     ["cheap", "fast", "effort-sensitive"],
     "极低价。高推理档位下表现不错，低档位明显退化（DeepSWE low 仅 2.4%），只适合 S 规模的简单任务。",
     "2026-09-22", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"deepswe_1_1_max": 66.6, "deepswe_1_1_low": 2.4})
base("gpt-5.6-sol", "GPT-5.6 Sol", "openai",
     sk(4.5, 4, 4.5, 4.5, 4.5, 4.5, 4.5, 5, 4.5, 4, 4.5, 4.5), (4, 20), 1_050_000, 500_000, 3,
     ["agentic", "terminal", "long-context", "previous-gen"],
     "长上下文保持出色（MRCR 512K–1M 73.8%），终端任务强；价格是 GPT-6.1 Sol 的两倍。",
     "2026-07-09", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"swe_bench_pro": 64.6, "terminal_bench_2_1": 88.8, "mrcr_v2_512k_1m": 73.8})
base("gpt-5.6-terra", "GPT-5.6 Terra", "openai",
     sk(4, 4, 4.5, 4, 4, 4, 4, 4.5, 4, 4, 4, 4), (2, 12), 1_050_000, 450_000, 3,
     ["balanced", "previous-gen"],
     "均衡型，适合常规执行任务；同价位通常 GPT-6.1 Sol 更优。",
     "2026-07-09", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"swe_bench_pro": 63.4, "terminal_bench_2_1": 87.4, "mrcr_v2_512k_1m": 72.5})
base("gpt-5.6-luna", "GPT-5.6 Luna", "openai",
     sk(3, 3.5, 3.5, 3.5, 3, 3, 3, 4, 3.5, 3.5, 3, 3.5), (0.2, 1.2), 1_050_000, 128_000, 5,
     ["cheap", "fast", "short-context"],
     "快速低价，短上下文表现稳定；长上下文利用差（MRCR 41%），不宜用于联调与大范围重构。",
     "2026-07-09", cache=(0.1, 1.25), long_ctx=OA_LONG, max_output=128_000,
     bench={"terminal_bench_2_1": 84.7, "mrcr_v2_256k_512k": 41.3})
base("gpt-5.5", "GPT-5.5", "openai",
     sk(4, 4, 4.5, 4, 4.5, 4, 4, 4.5, 4, 4, 4, 4), (5, 30), 1_050_000, 400_000, 3,
     ["legacy"],
     "旧版编码模型，价格高于新一代。缓存为 24h 保留、无写入溢价。",
     "2026-04-23", cache=(0.1, 1.0), long_ctx=OA_LONG, max_output=128_000,
     bench={"swe_bench_pro": 58.6, "swe_bench_verified": 82.6, "terminal_bench_2": 82.7, "gpqa_diamond": 93.6, "arc_agi_2": 85.0},
     extra={"cache": {"mode": "automatic_prefix", "ttl_seconds": 86400, "read_multiplier": 0.1, "write_multiplier": 1.0, "shared_baseline": True}})
base("gpt-5.4", "GPT-5.4", "openai",
     sk(3.5, 3.5, 4, 4, 4, 3.5, 3.5, 4, 3.5, 3.5, 3.5, 3.5), (2.5, 15), 1_050_000, 200_000, 3,
     ["legacy"], "旧版通用模型。", "2026-03", cache=(0.1, 1.0), long_ctx=OA_LONG)
base("gpt-5.4-mini", "GPT-5.4 mini", "openai",
     sk(2.5, 3, 3, 3, 3, 2.5, 2.5, 3, 3, 3, 2.5, 3), (0.75, 4.5), 400_000, 96_000, 5,
     ["cheap", "fast", "small"],
     "小模型，适合文档与 S 规模测试。长上下文衰减明显（MRCR 128–256K 33.6%）。",
     "2026-03-17", cache=(0.1, 1.0), max_output=128_000,
     bench={"swe_bench_pro": 54.4, "terminal_bench_2": 60.0, "gpqa_diamond": 88.0})
base("gpt-5.3-codex", "GPT-5.3 Codex", "openai",
     sk(3.5, 3.5, 4, 3.5, 4, 3.5, 3.5, 4, 3.5, 3, 3.5, 3.5), (1.75, 14), 400_000, 200_000, 3,
     ["legacy", "coding"], "旧版 Codex 专用模型。", "2026-02", cache=(0.1, 1.0))

# Google — ai.google.dev pricing (updated 2026-10-01). 3.6–3.8 Flash intro price through 2026-12-31.
FLASH_NOTE = {"price_note": "推广价至 2026-12-31，2027-01-01 起 $1.50 / $7.50"}
base("gemini-3.8-flash", "Gemini 3.8 Flash", "google",
     sk(4, 4.5, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4), (0.75, 3.75), 1_048_576, 300_000, 5,
     ["fast", "frontend", "cost-efficient", "implicit-cache"],
     "Google 当前默认模型，速度快、价格低，前端与界面任务突出。官方未公布编码跑分，分数按 3.7 Flash 外推。",
     "2026-09-02", cache=(0.1, 1.0), max_output=65_536, extra=dict(FLASH_NOTE, skills_estimated=True))
base("gemini-3.7-flash", "Gemini 3.7 Flash", "google",
     sk(3.5, 4.5, 4, 4, 4, 3.5, 3.5, 4, 4, 4, 3.5, 4), (0.75, 3.75), 1_048_576, 300_000, 5,
     ["fast", "frontend", "cost-efficient"],
     "WebDev Elo 1588，界面类任务强；跨模块联调与重构一般。",
     "2026-08-13", cache=(0.1, 1.0), max_output=65_536, extra=FLASH_NOTE,
     bench={"deepswe_1_1": 65.3, "frontiercode": 43.6, "terminal_bench_2_1": 85.8, "swe_bench_vals": 80.8, "gpqa_diamond": 93.9, "arc_agi_2": 84.6})
base("gemini-3.6-flash", "Gemini 3.6 Flash", "google",
     sk(3, 4, 3.5, 3.5, 3.5, 3, 3, 3.5, 3.5, 3.5, 3, 3.5), (0.75, 3.75), 1_048_576, 250_000, 5,
     ["fast", "previous-gen"], "已被 3.7 / 3.8 Flash 同价替代。", "2026-07-21",
     cache=(0.1, 1.0), max_output=65_536, extra=FLASH_NOTE,
     bench={"deepswe_1_1": 49.0, "swe_bench_vals": 79.6, "terminal_bench_2_1": 73.8})
base("gemini-3.5-flash", "Gemini 3.5 Flash", "google",
     sk(3, 3.5, 3.5, 3.5, 3.5, 3, 3, 3.5, 3.5, 3.5, 3, 3.5), (1.5, 9), 1_048_576, 250_000, 4,
     ["legacy"], "旧版 Flash，价格高于 3.8 Flash 推广价。", "2026-05-19",
     cache=(0.1, 1.0), max_output=65_536,
     bench={"swe_bench_pro": 55.1, "terminal_bench_2_1": 76.2, "arc_agi_2": 72.1})
base("gemini-3.1-pro", "Gemini 3.1 Pro", "google",
     sk(4, 4, 4, 4, 4.5, 3.5, 3.5, 3.5, 3.5, 4, 4, 4), (2, 12), 1_048_576, 400_000, 3,
     ["preview", "high-reasoning"],
     "仍为 Preview。推理与算法较强，Agent 型工具循环不及新一代 Flash。>200K 输入按 2× / 1.5× 计费。",
     "2026-02", cache=(0.1, 1.0), long_ctx=(200_000, 2.0, 1.5), max_output=65_536)

# xAI — docs.x.ai models. ≥200K input bills the whole request 2×.
XAI_LONG = (200_000, 2.0, 2.0)
base("grok-4.7", "Grok 4.7", "xai",
     sk(4, 4, 4.5, 4, 4.5, 4, 4, 4, 4, 3.5, 4, 4), (2, 6), 500_000, 250_000, 3,
     ["agentic", "cheap-output"],
     "输出价格低（$6），DeepSWE 71%；终端类任务（Terminal-Bench 4.0 37.6%）弱于同代 Claude / GPT。",
     "2026-09-21", cache=(0.25, 1.0), long_ctx=XAI_LONG,
     bench={"deepswe_1_1": 71.0, "terminal_bench_4": 37.6, "cursorbench_4": 46.3})
base("grok-4.6", "Grok 4.6", "xai",
     sk(3.5, 3.5, 4, 4, 4, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5), (2, 6), 500_000, 250_000, 4,
     ["previous-gen"], "已被 Grok 4.7 同价替代。", "2026-08-12",
     cache=(0.25, 1.0), long_ctx=XAI_LONG,
     bench={"deepswe_1_1": 65.9, "terminal_bench_3": 26.0, "cursorbench_3_2": 69.9})
base("grok-4.5", "Grok 4.5", "xai",
     sk(3.5, 3.5, 4, 4, 4, 3.5, 3.5, 4, 3.5, 3.5, 3.5, 3.5), (2, 6), 500_000, 250_000, 4,
     ["previous-gen"], "上一代 Grok，缓存读取比 4.7 便宜（0.15×）。", "2026-07-16",
     cache=(0.15, 1.0), long_ctx=XAI_LONG,
     bench={"swe_bench_pro": 64.7, "terminal_bench_2_1": 83.3, "deepswe_1": 62.0})
base("grok-build", "Grok Build 0.1", "xai",
     sk(2.5, 3, 3, 3, 3, 2.5, 2.5, 3, 3, 3, 2.5, 3), (1, 2), 256_000, 128_000, 5,
     ["fast", "cheap", "cli-default"], "Grok Build CLI 的默认模型，速度 >100 tok/s，适合 S 规模任务。", "2026-05-29",
     cache=(0.2, 1.0))

# DeepSeek — api-docs.deepseek.com pricing. Off-peak prices; weekday peak hours bill 2×.
DS_SCHED = {"price_schedule": {"peak_multiplier": 2.0, "peak_utc_hours": [[1, 4], [6, 10]], "weekdays_only": True}}
base("deepseek-v4-pro", "DeepSeek V4 Pro", "deepseek",
     sk(4, 3.5, 4, 4, 4.5, 3.5, 4, 3.5, 4, 3.5, 4, 4), (0.66, 1.98), 1_000_000, 300_000, 3,
     ["cost-efficient", "algorithm", "disk-cache", "open-weights"],
     "价格极低而后端与算法能力接近一线（SWE-bench Verified 80.6）。磁盘缓存命中价约 0.03×，工作日高峰时段价格翻倍。",
     "2026-08-13", cache=(0.033, 1.0), max_output=384_000, extra=DS_SCHED,
     bench={"swe_bench_verified": 80.6, "terminal_bench_2": 67.9, "livecodebench": 93.5})
base("deepseek-flash", "DeepSeek V4.1 Flash", "deepseek",
     sk(3, 3, 3.5, 3.5, 4, 3, 3, 3, 3.5, 3, 3, 3.5), (0.15, 0.6), 1_000_000, 200_000, 4,
     ["cheap", "fast", "algorithm", "disk-cache"],
     "最便宜的一线可用模型之一，算法题表现突出（LiveCodeBench 91.6）。",
     "2026-08", cache=(0.02, 1.0), max_output=384_000, extra=DS_SCHED,
     bench={"swe_bench_verified": 79.0, "livecodebench": 91.6})

# Alibaba Qwen — Model Studio docs; qwen3.8-max price from secondary sources.
base("qwen3.8-max", "Qwen3.8 Max", "alibaba",
     sk(4, 4, 4.5, 4, 4, 4, 4, 4.5, 4, 4, 4, 4), (2, 6), 1_000_000, 300_000, 3,
     ["agentic", "terminal"],
     "通义当前旗舰，Terminal-Bench 2.1 86.6、SWE-bench Pro 67.7。缓存价格未公布，按隐式缓存 0.2× 估算。",
     "2026-08-03", cache=(0.2, 1.0), max_output=131_072,
     bench={"swe_bench_pro": 67.7, "terminal_bench_2_1": 86.6, "frontierswe": 73.5})
base("qwen3-coder-plus", "Qwen3 Coder Plus", "alibaba",
     sk(3, 3.5, 3.5, 3.5, 3, 3, 3.5, 3, 3.5, 3, 3, 3), (1, 5), 1_000_000, 200_000, 4,
     ["coding", "tiered-price"],
     "编码专用版。价格按输入长度分档：≤32K $1 / $5，256K–1M $6 / $60；隐式缓存 0.2×，显式缓存 0.1×。",
     "2025-07", cache=(0.2, 1.0), max_output=65_536)
base("qwen3-coder-next", "Qwen3 Coder Next", "alibaba",
     sk(2.5, 3, 3, 3, 2.5, 2.5, 2.5, 3, 3, 2.5, 2.5, 2.5), (0.3, 1.5), 256_000, 128_000, 5,
     ["cheap", "fast", "small"], "轻量编码模型，Coding Plan 与 Kiro 中消耗最低。价格为估算。", "2026",
     cache=(0.2, 1.0), est=True)

# Moonshot Kimi — kimi.com blog / Kimi Code docs.
base("kimi-k3", "Kimi K3", "moonshot",
     sk(4.5, 4.5, 4.5, 4.5, 4.5, 4, 4.5, 4.5, 4, 4, 4, 4.5), (3, 15), 1_000_000, 400_000, 2,
     ["agentic", "terminal", "open-weights", "max-effort-only"],
     "开源权重中最强，Terminal-Bench 2.1 88.3、FrontierSWE 81.2；目前只开放最高推理档，速度偏慢。",
     "2026-07-16", cache=(0.1, 1.0),
     bench={"terminal_bench_2_1": 88.3, "frontierswe": 81.2, "deepswe": 67.3})
base("kimi-for-coding", "Kimi for Coding (K2.8 Preview)", "moonshot",
     sk(4, 4, 4, 4, 4, 3.5, 4, 4, 4, 3.5, 3.5, 4), (1, 4), 1_000_000, 300_000, 3,
     ["preview", "subscription-only"], "Kimi Code 订阅专用别名，当前指向 K2.8 Preview。价格为等效估算。", "2026-09",
     cache=(0.1, 1.0), est=True, extra={"skills_estimated": True})
base("kimi-k2.7-code", "Kimi K2.7 Code", "moonshot",
     sk(3.5, 3.5, 4, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5), (0.6, 2.5), 256_000, 160_000, 4,
     ["coding", "always-thinking"], "编码专用、始终思考；有 HighSpeed 版本。价格为估算。", "2026",
     cache=(0.1, 1.0), est=True, extra={"skills_estimated": True})

# Others
base("glm-5.2", "GLM-5.2", "zhipu",
     sk(3.5, 4, 4, 4, 4, 3.5, 3.5, 4, 4, 3.5, 3.5, 4), (1.4, 4.4), 1_000_000, 300_000, 3,
     ["open-weights", "cost-efficient"], "开源（MIT）高性价比模型，Terminal-Bench 2.1 81.0。缓存价格未公布，按 0.2× 估算。",
     "2026-06-16", cache=(0.2, 1.0), max_output=131_072,
     bench={"swe_bench_pro": 62.1, "terminal_bench_2_1": 81.0, "frontierswe": 74.4})
base("minimax-m3", "MiniMax M3", "minimax",
     sk(3, 3.5, 3.5, 3.5, 3.5, 3, 3, 3, 3.5, 3, 3, 3.5), (0.3, 1.2), 1_048_576, 250_000, 4,
     ["open-weights", "cheap"], "低价开源模型，适合 S / M 规模常规任务。", "2026-06-01",
     cache=(0.2, 1.0), max_output=262_144,
     bench={"swe_bench_pro": 59.0, "terminal_bench_2_1": 66.0})
base("muse-spark-1.3", "Muse Spark 1.3", "meta",
     sk(3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5), (1.25, 4.25), 1_048_576, 300_000, 3,
     ["low-confidence"], "Meta 编码模型。官方跑分只以图片发布，领域分数为保守初值，依赖本地学习校准。", "2026-09-02",
     cache=(0.12, 1.0), extra={"skills_estimated": True})
base("composer-2.5", "Cursor Composer 2.5", "cursor",
     sk(3.5, 4, 4, 3.5, 3.5, 3.5, 4, 3.5, 4, 3.5, 3.5, 4), (0.5, 2.5), 200_000, 160_000, 5,
     ["fast", "cheap", "cursor-native"],
     "Cursor 自研模型，速度快、价格低，适合前端、测试与中小规模重构。Fast 版 $3 / $15。上下文为估算。",
     "2026-05-18", cache=(0.1, 1.0),
     bench={"terminal_bench_2": 69.3, "swe_bench_multilingual": 79.8, "cursorbench_3_1": 63.2})
base("devstral-2", "Devstral 2", "mistral",
     sk(2.5, 3, 3.5, 3, 3, 2.5, 3, 2.5, 3, 3, 2.5, 3), (0.4, 2), 256_000, 128_000, 4,
     ["open-weights", "cheap"], "Mistral 开源编码模型，终端类任务较弱（Terminal-Bench 2 32.6）。", "2025-12-09",
     bench={"swe_bench_verified": 72.2, "terminal_bench_2": 32.6})
base("cursor-auto", "Cursor Auto", "cursor",
     sk(3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5), (1.25, 6), 200_000, 128_000, 4,
     ["router", "low-confidence"], "Cursor 自动路由，实际模型不固定；分数取保守中值。价格为估算。", "-",
     cache=(0.1, 1.0), est=True, extra={"skills_estimated": True})
base("kiro-auto", "Kiro Auto", "amazon",
     sk(3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5), (2, 10), 200_000, 128_000, 4,
     ["router", "low-confidence"], "Kiro 自动路由（1.0× 积分基准），实际模型不固定。价格按 Sonnet 级估算。", "-",
     est=True, extra={"skills_estimated": True})

# ---------------------------------------------------------------- provider listings
# (model, cli_model_arg, overrides). verified=False marks args not seen in a real catalog.
P = {
  "CLAUDE_CODE": [("claude-fable-5-1", "fable"), ("claude-opus-5-5", "opus"), ("claude-sonnet-5-5", "sonnet"),
                  ("claude-haiku-4-5", "haiku"), ("claude-fable-5", "claude-fable-5"), ("claude-opus-5", "claude-opus-5"),
                  ("claude-opus-4-8", "claude-opus-4-8"), ("claude-sonnet-5", "claude-sonnet-5")],
  "CODEX_CLI": [(m, m) for m in ["gpt-6-astra", "gpt-6.1-sol", "gpt-6-sol", "gpt-6-luna",
                                 "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"]],
  "CURSOR_CLI": [("cursor-auto", "auto"), ("composer-2.5", "composer-2.5"),
                 ("claude-fable-5-1", "claude-fable-5-1-high"), ("claude-opus-5-5", "claude-opus-5-5-medium"),
                 ("claude-sonnet-5-5", "claude-sonnet-5-5-high"), ("claude-opus-5", "claude-opus-5-high"),
                 ("claude-sonnet-5", "claude-sonnet-5-thinking-high"),
                 ("gpt-5.6-sol", "gpt-5.6-sol-medium"), ("gpt-5.6-terra", "gpt-5.6-terra-medium"),
                 ("gpt-5.6-luna", "gpt-5.6-luna-medium"), ("gpt-5.5", "gpt-5.5-medium"),
                 ("gpt-5.4-mini", "gpt-5.4-mini-medium"), ("gpt-5.3-codex", "gpt-5.3-codex"),
                 ("gemini-3.8-flash", "gemini-3.8-flash-medium"), ("gemini-3.7-flash", "gemini-3.7-flash-medium"),
                 ("gemini-3.1-pro", "gemini-3.1-pro"), ("grok-4.7", "grok-4.7-medium"),
                 ("grok-4.5", "cursor-grok-4.5-medium"), ("kimi-k3", "kimi-k3-high"),
                 ("kimi-k2.7-code", "kimi-k2.7-code"), ("glm-5.2", "glm-5.2-high"),
                 ("muse-spark-1.3", "muse-spark-1.3-medium")],
  "COPILOT_CLI": [("claude-fable-5-1", "claude-fable-5.1"), ("claude-opus-5-5", "claude-opus-5.5"),
                  ("claude-sonnet-5", "claude-sonnet-5"), ("claude-haiku-4-5", "claude-haiku-4.5"),
                  ("gpt-6-astra", "gpt-6-astra"), ("gpt-6.1-sol", "gpt-6.1-sol"), ("gpt-6-luna", "gpt-6-luna"),
                  ("gpt-5.6-sol", "gpt-5.6-sol"), ("gpt-5.6-terra", "gpt-5.6-terra"), ("gpt-5.6-luna", "gpt-5.6-luna"),
                  ("gpt-5.4-mini", "gpt-5.4-mini"), ("gemini-3.8-flash", "gemini-3.8-flash"),
                  ("gemini-3.7-flash", "gemini-3.7-flash"), ("gemini-3.6-flash", "gemini-3.6-flash"),
                  ("gemini-3.5-flash", "gemini-3.5-flash"), ("gpt-5.4", "gpt-5.4"), ("gpt-5.3-codex", "gpt-5.3-codex"),
                  ("grok-4.5", "grok-4.5"),
                  ("kimi-k3", "kimi-k3"), ("kimi-k2.7-code", "kimi-k2.7-code")],
  "ANTIGRAVITY_CLI": [("gemini-3.8-flash", "gemini-3.8-flash", False), ("gemini-3.1-pro", "gemini-3.1-pro-high", False),
                      ("gemini-3.5-flash", "gemini-3.5-flash", False)],
  "GROK_CLI": [("grok-build", "grok-build"), ("grok-4.7", "grok-4.7"), ("grok-4.6", "grok-4.6"), ("grok-4.5", "grok-4.5")],
  "KIRO_CLI": [("kiro-auto", "auto", False, {"credit_multiplier": 1.0}),
               ("claude-opus-5-5", "claude-opus-5.5", False, {"credit_multiplier": 2.0}),
               ("claude-sonnet-5-5", "claude-sonnet-5.5", False, {"credit_multiplier": 1.3}),
               ("claude-haiku-4-5", "claude-haiku-4.5", False, {"credit_multiplier": 0.4, "context_window": 200_000}),
               ("gpt-5.6-sol", "gpt-5.6-sol", False, {"credit_multiplier": 4.4}),
               ("gpt-5.6-terra", "gpt-5.6-terra", False, {"credit_multiplier": 2.2}),
               ("gpt-5.6-luna", "gpt-5.6-luna", False, {"credit_multiplier": 1.1}),
               ("qwen3-coder-next", "qwen3-coder-next", False, {"credit_multiplier": 0.05})],
  "QWEN_CODE": [("qwen3.8-max", "qwen3.8-max", False), ("qwen3-coder-plus", "qwen3-coder-plus"),
                ("qwen3-coder-next", "qwen3-coder-next")],
  "KIMI_CODE": [("kimi-k3", "k3"), ("kimi-for-coding", "kimi-for-coding"),
                ("kimi-k2.7-code", "kimi-for-coding-highspeed", True, {"context_window": 256_000})],
  "MUSE_CODE": [("muse-spark-1.3", "muse-spark-1.3", False)],
  "AIDER": [("deepseek-v4-pro", "deepseek/deepseek-v4-pro", False), ("deepseek-flash", "deepseek/deepseek-flash", False),
            ("claude-sonnet-5-5", "anthropic/claude-sonnet-5-5", False), ("gpt-6.1-sol", "openai/gpt-6.1-sol", False)],
  "OPENCODE": [("deepseek-v4-pro", "deepseek/deepseek-v4-pro", False), ("deepseek-flash", "deepseek/deepseek-flash", False),
               ("glm-5.2", "zhipuai/glm-5.2", False), ("minimax-m3", "minimax/MiniMax-M3", False),
               ("kimi-k3", "moonshotai/kimi-k3", False), ("qwen3.8-max", "alibaba/qwen3.8-max", False),
               ("devstral-2", "mistral/devstral-2512", False),
               ("claude-sonnet-5-5", "anthropic/claude-sonnet-5-5", False), ("gpt-6.1-sol", "openai/gpt-6.1-sol", False)],
  "REASONIX": [("deepseek-v4-pro", "deepseek/deepseek-v4-pro", False), ("deepseek-flash", "deepseek/deepseek-v4-flash", False)],
}
# CLI-imposed context caps (Codex catalog reports 272K for every model).
CTX_CAP = {"CODEX_CLI": 272_000, "GROK_CLI": None}

PLANNERS = {"claude-fable-5-1", "claude-fable-5", "claude-opus-5-5", "gpt-6-astra"}

# Shared, model-intrinsic data: one entry per model, whichever CLIs offer it.
MODEL_KEYS = ["display_name", "vendor", "roles", "skills", "price_per_mtok", "context_window", "effective_context",
              "max_output", "speed", "tags", "description", "cache", "cache_rates", "long_context_pricing",
              "price_schedule", "price_note", "price_estimated", "skills_estimated", "released", "benchmarks"]
offered = {mid for items in P.values() for it in items for mid in [it[0]]}
models = {}
for mid, b in M.items():
    if mid not in offered:
        continue
    roles = ["executor"]
    if mid in PLANNERS: roles.insert(0, "planner")
    if b["skills"]["review"] >= 4.5: roles.append("reviewer")
    b = dict(b, roles=roles)
    models[mid] = {k: b[k] for k in MODEL_KEYS if k in b}

# Per-CLI listings: only what differs by channel.
offerings = {}
for prov, items in P.items():
    rows = []
    for it in items:
        mid, arg = it[0], it[1]
        verified = it[2] if len(it) > 2 else True
        over = it[3] if len(it) > 3 else {}
        b = M[mid]
        o = {"model": mid, "cli_model_arg": arg}
        ctx = over.get("context_window") or CTX_CAP.get(prov)
        if ctx and ctx < b["context_window"]:
            o["context_window"] = ctx
            o["effective_context"] = min(b["effective_context"], int(ctx * (0.7 if "context_window" in over else 0.8)))
        if "credit_multiplier" in over: o["credit_multiplier"] = over["credit_multiplier"]
        if not verified: o["cli_model_arg_verified"] = False
        rows.append(o)
    offerings[prov] = rows

# ---------------------------------------------------------------- providers
NODE_PROXY = {"env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"], "extra_env": {"NODE_USE_ENV_PROXY": "1"},
              "socks": False, "own_config_overrides": False, "forced_ok": True, "verified": False}
STD_PROXY = {"env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"], "extra_env": {}, "socks": False,
             "own_config_overrides": False, "forced_ok": True, "verified": False}
API_KEYS = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENAI_BASE_URL", "GEMINI_API_KEY", "DEEPSEEK_API_KEY",
            "OPENROUTER_API_KEY", "XAI_API_KEY", "MISTRAL_API_KEY", "DASHSCOPE_API_KEY", "MOONSHOT_API_KEY",
            "ZHIPUAI_API_KEY", "ZAI_API_KEY", "MINIMAX_API_KEY", "GROQ_API_KEY"]

def prov(billing, cache, ctx, conc, env_allow, proxy, quota=None, **extra):
    p = {"billing": billing, "cache": cache, "context": ctx, "concurrency_hint": conc,
         "env_allow": env_allow, "proxy": proxy}
    if quota: p["quota"] = quota
    p.update(extra)
    return p

NATIVE = lambda r=0.92, o=0.06: {"cli_auto_compact_ratio": r, "compact": "native", "compact_output_ratio": o}
BY_MODEL = {"mode": "by_model"}

providers = {
  "CLAUDE_CODE": prov("SUBSCRIPTION",
      {"mode": "explicit_prefix", "ttl_seconds": 300, "read_multiplier": 0.1, "write_multiplier": 1.25, "shared_baseline": True},
      NATIVE(0.92, 0.06), 2, ["ANTHROPIC_*", "CLAUDE_CODE_*"],
      {"env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"], "extra_env": {}, "socks": False,
       "own_config_overrides": True, "forced_ok": True, "verified": True},
      quota={"windows": ["5h_session", "7d_weekly"], "usage_command": "/usage", "usage_command_free": True}),
  "CODEX_CLI": prov("SUBSCRIPTION",
      {"mode": "automatic_prefix", "ttl_seconds": 1800, "read_multiplier": 0.1, "write_multiplier": 1.25, "shared_baseline": True},
      NATIVE(0.9, 0.05), 2, ["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_ORG_ID", "OPENAI_ORGANIZATION", "OPENAI_PROJECT", "CODEX_*"],
      STD_PROXY, quota={"windows": ["5h_session", "7d_weekly"], "usage_command": "/status", "usage_command_free": True}),
  "CURSOR_CLI": prov("SUBSCRIPTION", BY_MODEL, NATIVE(0.9, 0.06), 2, ["CURSOR_*"], NODE_PROXY,
      quota={"windows": ["monthly"]}),
  "COPILOT_CLI": prov("SUBSCRIPTION", BY_MODEL, NATIVE(0.9, 0.06), 2,
      ["COPILOT_*", "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"], NODE_PROXY,
      quota={"windows": ["monthly_premium_requests"]}),
  "ANTIGRAVITY_CLI": prov("SUBSCRIPTION",
      {"mode": "implicit", "hit_reliability": 0.6, "read_multiplier": 0.1, "write_multiplier": 1.0, "min_prefix_tokens": 4096, "shared_baseline": False},
      NATIVE(0.9, 0.06), 2, ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_APPLICATION_CREDENTIALS", "GOOGLE_CLOUD_*"], STD_PROXY,
      quota={"windows": ["per_model"], "usage_command": "/usage", "usage_command_free": True}),
  "GROK_CLI": prov("SUBSCRIPTION",
      {"mode": "automatic_prefix", "ttl_seconds": 300, "read_multiplier": 0.25, "write_multiplier": 1.0, "shared_baseline": True},
      NATIVE(0.9, 0.06), 2, ["XAI_API_KEY", "GROK_*"], STD_PROXY, quota={"windows": ["7d_weekly"]}),
  "KIRO_CLI": prov("SUBSCRIPTION", BY_MODEL, NATIVE(0.9, 0.06), 2, ["KIRO_*", "AWS_PROFILE", "AWS_REGION"], STD_PROXY,
      quota={"windows": ["monthly_credits"]}),
  "QWEN_CODE": prov("PAY_AS_YOU_GO",
      {"mode": "implicit", "hit_reliability": 0.7, "read_multiplier": 0.2, "write_multiplier": 1.0, "shared_baseline": True},
      NATIVE(0.9, 0.06), 3, ["DASHSCOPE_API_KEY", "BAILIAN_CODING_PLAN_API_KEY", "OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL", "QWEN_*"],
      NODE_PROXY, quota={"windows": ["5h_requests", "7d_requests", "monthly_requests"], "note": "Coding Plan 按请求次数计额"}),
  "KIMI_CODE": prov("SUBSCRIPTION",
      {"mode": "automatic_prefix", "ttl_seconds": 600, "read_multiplier": 0.1, "write_multiplier": 1.0, "shared_baseline": True},
      NATIVE(0.9, 0.06), 2, ["KIMI_*", "MOONSHOT_API_KEY"], STD_PROXY, quota={"windows": ["5h_session", "7d_weekly"]}),
  "MUSE_CODE": prov("PAY_AS_YOU_GO",
      {"mode": "automatic_prefix", "ttl_seconds": 300, "read_multiplier": 0.12, "write_multiplier": 1.0, "shared_baseline": True},
      NATIVE(0.9, 0.06), 3, ["META_API_KEY", "MUSE_*"], STD_PROXY),
  "AIDER": prov("PAY_AS_YOU_GO", BY_MODEL, {"cli_auto_compact_ratio": None, "compact": "none", "compact_output_ratio": 0}, 4,
      API_KEYS + ["AIDER_*"], {"env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"], "extra_env": {}, "socks": False,
                              "own_config_overrides": False, "forced_ok": True, "verified": True}),
  "OPENCODE": prov("PAY_AS_YOU_GO", BY_MODEL, NATIVE(0.9, 0.06), 4, API_KEYS + ["OPENCODE_*"], STD_PROXY),
  "REASONIX": prov("PAY_AS_YOU_GO", BY_MODEL, NATIVE(0.9, 0.06), 4, API_KEYS + ["REASONIX_*"], STD_PROXY),
}
for pid, keys in [("QODER_CLI", ["QODER_*"]), ("AMP", ["AMP_API_KEY", "AMP_*"]),
                  ("CRUSH", API_KEYS + ["CRUSH_*"]), ("GOOSE", API_KEYS + ["GOOSE_*"]),
                  ("CLINE", API_KEYS + ["CLINE_*"]), ("DROID", ["FACTORY_API_KEY", "FACTORY_*"]),
                  ("AUGGIE", ["AUGMENT_SESSION_AUTH", "AUGMENT_*"]), ("TRAE_CLI", ["TRAE_*"])]:
    billing = "PAY_AS_YOU_GO" if pid in ("CRUSH", "GOOSE", "CLINE") else "SUBSCRIPTION"
    providers[pid] = prov(billing, BY_MODEL, NATIVE(0.9, 0.06), 2, keys, STD_PROXY)

for pid, rows in offerings.items():
    assert pid in providers, pid
    providers[pid]["models"] = rows

# ---------------------------------------------------------------- usage profiles
# (turns, calls_per_turn, growth_per_turn, output_per_turn) for S; M/L scale from it.
UP_BASE = {
  "planning":    (1, 6, 12000, 6000),  "frontend-ui": (1, 8, 10000, 3000),
  "backend-api": (1, 9, 11000, 3500),  "data-model":  (1, 7, 9000, 3000),
  "algorithm":   (1, 6, 8000, 4000),   "integration": (1, 12, 14000, 3000),
  "refactor":    (1, 12, 16000, 4500), "infra":       (1, 10, 9000, 2500),
  "testing":     (1, 6, 8000, 3000),   "docs":        (1, 4, 6000, 4000),
  "review":      (1, 8, 14000, 2500),  "general":     (1, 7, 9000, 3000),
}
usage_profiles = {}
for d, (t, c, g, o) in UP_BASE.items():
    usage_profiles[d] = {
        "S": {"turns": t, "calls_per_turn": c, "growth_per_turn": g, "output_per_turn": o},
        "M": {"turns": t + 1, "calls_per_turn": round(c * 1.7), "growth_per_turn": round(g * 1.8, -3), "output_per_turn": round(o * 2, -3)},
        "L": {"turns": t + 2, "calls_per_turn": round(c * 2.5), "growth_per_turn": round(g * 2.5, -3), "output_per_turn": round(o * 3, -3)},
    }

# ---------------------------------------------------------------- rule tables
triage_rules = [
  {"id": "multi-domain", "toward": "PLAN", "weight": 3, "kind": "domain_count", "min": 2},
  {"id": "many-files", "toward": "PLAN", "weight": 2, "kind": "file_count", "min": 4},
  {"id": "feature-zh", "toward": "PLAN", "weight": 2, "kind": "keyword",
   "keywords": ["实现", "开发", "搭建", "新增功能", "重构整个", "设计一个", "从零", "迁移到", "整个项目", "模块"]},
  {"id": "feature-en", "toward": "PLAN", "weight": 2, "kind": "regex",
   "pattern": "(?i)\\b(implement|build|scaffold|create an? (app|service|module|feature)|migrate (to|from)|refactor (the )?(whole|entire)|from scratch|end[- ]to[- ]end)\\b"},
  {"id": "single-issue-zh", "toward": "DIRECT", "weight": 3, "kind": "keyword",
   "keywords": ["这个报错", "报错", "解释", "改一下", "这个函数", "这一行", "为什么", "是什么意思", "修一下", "拼写"]},
  {"id": "single-issue-en", "toward": "DIRECT", "weight": 3, "kind": "regex",
   "pattern": "(?i)\\b(this error|explain|what does|why does|fix (this|the) (typo|bug|line|function)|rename|stack ?trace)\\b"},
  {"id": "pinpoint-location", "toward": "DIRECT", "weight": 2, "kind": "regex",
   "pattern": "[\\w./-]+\\.[a-zA-Z]{1,6}:\\d+"},
  {"id": "short-message", "toward": "DIRECT", "weight": 1, "kind": "max_chars", "max": 80},
]
domain_keywords = {
  "frontend-ui": ["组件", "样式", "页面", "界面", "交互", "css", "tailwind", "react", "vue", "svelte", "component", "ui", "layout", "a11y", "accessibility"],
  "backend-api": ["接口", "api", "服务端", "鉴权", "endpoint", "handler", "route", "auth", "middleware", "grpc", "rest"],
  "data-model": ["数据库", "表结构", "迁移", "查询", "sql", "schema", "migration", "index", "orm", "sqlite", "postgres"],
  "algorithm": ["算法", "性能", "复杂度", "优化", "algorithm", "performance", "complexity", "optimize", "concurrency"],
  "integration": ["联调", "合并冲突", "集成", "integration", "merge conflict", "wire up", "end-to-end"],
  "refactor": ["重构", "迁移", "重命名", "拆分", "refactor", "rename", "extract", "restructure", "upgrade"],
  "infra": ["构建", "部署", "ci", "docker", "k8s", "pipeline", "build", "deploy", "github actions", "terraform", "nginx"],
  "testing": ["测试", "单测", "用例", "覆盖率", "test", "spec", "coverage", "e2e", "mock"],
  "docs": ["文档", "注释", "readme", "docs", "documentation", "changelog", "comment"],
  "review": ["评审", "审查", "review", "audit", "检查一下"],
}

usage_patterns = {
  "CLAUDE_CODE": [
    {"id": "session-percent", "kind": "percent_used", "window": "5h_session",
     "pattern": "(?i)used\\s+(?P<percent>\\d{1,3})%\\s+of\\s+your\\s+session\\s+limit(?:\\s*[·∙•]\\s*resets\\s+(?P<reset>[^\\n·]+))?"},
    {"id": "weekly-percent", "kind": "percent_used", "window": "7d_weekly",
     "pattern": "(?i)used\\s+(?P<percent>\\d{1,3})%\\s+of\\s+your\\s+weekly(?:\\s+\\w+)?\\s+limit(?:\\s*[·∙•]\\s*resets\\s+(?P<reset>[^\\n·]+))?"},
    {"id": "session-hit", "kind": "limit_reached", "window": "5h_session",
     "pattern": "(?i)(?:hit|reached)\\s+your\\s+(?:session|5-hour)\\s+limit(?:\\s*[·∙•]\\s*resets\\s+(?P<reset>[^\\n·]+))?"},
    {"id": "weekly-hit", "kind": "limit_reached", "window": "7d_weekly",
     "pattern": "(?i)(?:hit|reached)\\s+your\\s+weekly(?:\\s+(?P<model>\\w+))?\\s+limit(?:\\s*[·∙•]\\s*resets\\s+(?P<reset>[^\\n·]+))?"},
    {"id": "model-hit", "kind": "limit_reached", "window": "per_model",
     "pattern": "(?i)hit\\s+your\\s+(?P<model>opus|sonnet|fable)\\s+limit"},
    {"id": "auto-continue", "kind": "limit_reached", "window": "5h_session",
     "pattern": "(?i)usage\\s+limit\\s+reached\\s*[·∙•]\\s*continuing\\s+automatically\\s+at\\s+(?P<reset>[^\\n·]+)"},
    {"id": "usage-panel", "kind": "percent_used", "window": "auto", "source": "/usage",
     "pattern": "(?im)^\\s*(?P<label>current session|current week[^\\n]*)\\s*$[\\s\\S]{0,200}?(?P<percent>\\d{1,3})%\\s+used(?:[\\s\\S]{0,80}?resets\\s+(?P<reset>[^\\n]+))?"},
  ],
  "CODEX_CLI": [
    {"id": "limit-hit", "kind": "limit_reached", "window": "auto",
     "pattern": "(?i)you'?ve\\s+hit\\s+your\\s+usage\\s+limit[^\\n]*?(?:try\\s+again\\s+(?:in\\s+(?P<reset_in>[^.\\n]+)|at\\s+(?P<reset>[^.\\n]+)))?"},
    {"id": "status-percent", "kind": "percent_used", "window": "auto", "source": "/status",
     "pattern": "(?im)(?P<label>5h limit|weekly limit)\\s*:?[^\\n]*?(?P<percent>\\d{1,3})%\\s+used(?:[^\\n]*?resets\\s+(?P<reset>[^)\\n]+))?"},
  ],
  "CURSOR_CLI": [
    {"id": "limit-hit", "kind": "limit_reached", "window": "monthly", "verified": False,
     "pattern": "(?i)(?:you'?ve\\s+hit\\s+your\\s+usage\\s+limit|usage\\s+limit\\s+(?:reached|exceeded))[^\\n]*?(?:resets?\\s+(?:on\\s+)?(?P<reset>[^.\\n]+))?"},
  ],
  "COPILOT_CLI": [
    {"id": "premium-exhausted", "kind": "limit_reached", "window": "monthly_premium_requests", "verified": False,
     "pattern": "(?i)(?:exceeded|used\\s+all)[^\\n]*?premium\\s+request[^\\n]*?(?:resets?\\s+(?:on\\s+)?(?P<reset>[^.\\n]+))?"},
    {"id": "rate-limited", "kind": "rate_limited", "verified": False,
     "pattern": "(?i)rate\\s+limit(?:ed)?[^\\n]*?(?:try\\s+again\\s+in\\s+(?P<reset_in>[^.\\n]+))?"},
  ],
  "ANTIGRAVITY_CLI": [
    {"id": "quota-percent", "kind": "percent_used", "window": "per_model", "source": "/usage", "verified": False,
     "pattern": "(?im)^\\s*(?P<model>[^:\\n]+):\\s*(?P<percent>\\d{1,3})%\\s+used(?:\\s*\\(resets\\s+(?P<reset>[^)]+)\\))?"},
    {"id": "resource-exhausted", "kind": "limit_reached", "window": "per_model",
     "pattern": "(?i)\\b(?:resource[_ ]exhausted|429)\\b[^\\n]*"},
  ],
  "GROK_CLI": [
    {"id": "free-exhausted", "kind": "limit_reached", "window": "7d_weekly",
     "pattern": "(?i)reached\\s+your\\s+free\\s+grok\\s+build\\s+usage\\s+limit"},
    {"id": "plan-rate-limit", "kind": "limit_reached", "window": "7d_weekly",
     "pattern": "(?i)hit\\s+the\\s+rate\\s+limit\\s+for\\s+your\\s+plan"},
    {"id": "team-rate-limit", "kind": "rate_limited",
     "pattern": "(?i)hit\\s+your\\s+team'?s\\s+api\\s+rate\\s+limit"},
    {"id": "overloaded", "kind": "overloaded",
     "pattern": "(?i)model\\s+is\\s+temporarily\\s+overloaded"},
  ],
  "REASONIX": [
    {"id": "quota-reached", "kind": "limit_reached",
     "pattern": "(?i)\\b(?:rate\\s+limit\\s+exceeded|quota\\s+(?:reached|exceeded)|insufficient\\s+credits?|余额不足)\\b(?:.*?(?:resets?\\s+(?:in|at)\\s+(?P<reset>[^.\\n]+)))?"},
  ],
  "_generic": [
    {"id": "http-429", "kind": "rate_limited",
     "pattern": "(?i)\\b(?:429|too\\s+many\\s+requests|rate[_ ]limit(?:ed|_exceeded)?)\\b(?:[^\\n]*?(?:retry|try\\s+again)\\s+(?:after|in)\\s+(?P<reset_in>[^.\\n]+))?"},
    {"id": "insufficient-balance", "kind": "budget_exhausted",
     "pattern": "(?i)\\b(?:insufficient[_ ](?:balance|quota|credits?)|402\\s+payment\\s+required|余额不足)\\b"},
  ],
}
time_formats = {
  "relative": "(?i)(?:(?P<d>\\d+)\\s*d(?:ays?)?\\s*)?(?:(?P<h>\\d+)\\s*h(?:ours?)?\\s*)?(?:(?P<m>\\d+)\\s*m(?:in(?:utes?)?)?)?",
  "clock": "(?i)(?:(?P<dow>mon|tue|wed|thu|fri|sat|sun)\\w*\\s+)?(?P<hour>\\d{1,2})(?::(?P<minute>\\d{2}))?\\s*(?P<ampm>am|pm)?(?:\\s*\\((?P<tz>[^)]+)\\))?",
}

noise_rules = [
  {"id": "braille-spinner", "action": "drop", "pattern": "^\\s*[⠀-⣿]\\s*.{0,80}$"},
  {"id": "ascii-spinner", "action": "drop", "pattern": "^\\s*[|/\\\\-]\\s*(?:\\S.{0,60})?$"},
  {"id": "dots-spinner", "action": "drop", "pattern": "^\\s*[◐◓◑◒●○◉◎✶✷✸✹✺✻✼✽·]\\s*(?:thinking|working|loading|running|processing|思考中|处理中)?[.…]*\\s*$"},
  {"id": "progress-bar", "action": "drop", "pattern": "^\\s*\\[?[=#>\\-\\s█▉▊▋▌▍▎▏░▒▓]*\\]?\\s*\\d{1,3}%"},
  {"id": "byte-progress", "action": "drop", "pattern": "^\\s*\\d+(?:\\.\\d+)?\\s*[KMG]i?B\\s*/\\s*\\d+(?:\\.\\d+)?\\s*[KMG]i?B\\b"},
  {"id": "npm-progress", "action": "drop", "pattern": "^\\s*(?:⸨[░▒▓█#\\s]*⸩|\\(#{2,}\\.*\\))"},
  {"id": "cargo-progress", "action": "drop", "pattern": "^\\s*Building\\s+\\[[=>\\s]+\\]\\s+\\d+/\\d+"},
  {"id": "pip-progress", "action": "drop", "pattern": "^\\s*[━╸╺\\s]+\\s+\\d+(?:\\.\\d+)?/\\d+(?:\\.\\d+)?\\s*[kMG]?B"},
  {"id": "elapsed-only", "action": "drop", "pattern": "^\\s*\\(?\\d+(?:\\.\\d+)?s\\)?\\s*$"},
  {"id": "blank-run", "action": "collapse", "pattern": "^\\s*$"},
]

gate_allowlist = [
  "^(?:npm|pnpm|yarn|bun)\\s+(?:run\\s+)?(?:test|build|lint|typecheck|check|format:check)(?:\\s+--?[\\w:=.-]+)*$",
  "^npx\\s+(?:tsc|vitest|jest|eslint|prettier\\s+--check|playwright\\s+test)(?:\\s+[\\w:=./-]+)*$",
  "^cargo\\s+(?:test|build|check|clippy|fmt\\s+--check)(?:\\s+--?[\\w:=.-]+)*$",
  "^(?:python3?\\s+-m\\s+)?pytest(?:\\s+[\\w:=./-]+)*$",
  "^(?:ruff\\s+check|mypy|pyright)(?:\\s+[\\w:=./-]+)*$",
  "^go\\s+(?:test|build|vet)(?:\\s+[\\w:=./-]+)*$",
  "^(?:mvn|\\./mvnw)\\s+(?:-q\\s+)?(?:test|verify|compile|package)(?:\\s+-[\\w:=.-]+)*$",
  "^(?:gradle|\\./gradlew)\\s+(?:test|build|check|assemble)(?:\\s+--?[\\w:=.-]+)*$",
  "^dotnet\\s+(?:test|build)(?:\\s+[\\w:=./-]+)*$",
  "^swift\\s+(?:test|build)(?:\\s+--?[\\w:=.-]+)*$",
  "^(?:bundle\\s+exec\\s+)?(?:rspec|rake\\s+test)(?:\\s+[\\w:=./-]+)*$",
  "^(?:php\\s+)?(?:vendor/bin/)?phpunit(?:\\s+[\\w:=./-]+)*$",
  "^make\\s+(?:test|build|check|lint|all)$",
  "^deno\\s+(?:test|check|lint)(?:\\s+[\\w:=./-]+)*$",
]

cli_dirs = ["~/.claude/**", "~/.claude.json", "~/.codex/**", "~/.cursor/**", "~/.copilot/**",
            "~/.config/github-copilot/**", "~/.config/opencode/**", "~/.local/share/opencode/**", "~/.aider*",
            "~/.antigravity/**", "~/.config/antigravity/**", "~/.gemini/**", "~/.grok/**", "~/.kiro/**",
            "~/.qwen/**", "~/.kimi-code/**", "~/.qoder/**", "~/.config/amp/**", "~/.local/share/amp/**",
            "~/.local/share/crush/**", "~/.config/crush/**", "~/.config/goose/**", "~/.cline/**",
            "~/.factory/**", "~/.augment/**", "~/.config/muse/**", "~/.trae/**", "~/.reasonix/**"]
generic_creds = ["~/.ssh/**", "~/.aws/**", "~/.config/gcloud/**", "~/.azure/**", "~/.kube/**",
                 "~/.docker/config.json", "~/.npmrc", "~/.pypirc", "~/.netrc", "~/.git-credentials",
                 "~/.config/gh/**", "~/.gnupg/**", "~/.cargo/credentials*", "~/.terraform.d/credentials*",
                 "~/Library/Keychains/**", "~/Library/Application Support/Google/Chrome/**",
                 "~/Library/Application Support/Firefox/**", "~/Library/Safari/**",
                 "~/.config/google-chrome/**", "~/.mozilla/**", "%APPDATA%/Microsoft/Credentials/**",
                 "{tokage_app_data}/**"]

command_policy = {
  "read_only": ["ls", "pwd", "cat", "head", "tail", "wc", "rg", "grep", "find", "fd", "tree", "file", "stat",
                "diff", "sort", "uniq", "cut", "jq", "which", "echo", "date", "env", "printenv",
                "git status", "git diff", "git log", "git show", "git branch", "git blame", "git rev-parse",
                "git ls-files", "git remote -v", "node --version", "python --version", "cargo --version",
                "npm ls", "cargo tree", "go list"],
  "sensitive": ["rm", "mv", "cp -r", "chmod", "chown", "ln", "git reset", "git checkout", "git restore",
                "git clean", "git push", "git rebase", "git commit", "git merge", "git stash drop",
                "npm install", "npm i", "pnpm add", "pnpm install", "yarn add", "bun add", "pip install",
                "uv pip install", "cargo install", "cargo add", "go install", "go get", "brew install",
                "docker", "podman", "kubectl", "terraform", "curl", "wget", "ssh", "scp", "rsync",
                "nc", "open", "xdg-open", "kill", "pkill", "eval", "bash -c", "sh -c", "zsh -c",
                "python -c", "node -e", "perl -e", "ruby -e", "read:.env*", "write:.env*"],
  "forbidden": ["sudo", "su", "doas", "security", "launchctl", "systemctl", "crontab", "dd", "mkfs",
                "diskutil eraseDisk", "shutdown", "reboot", "curl * | sh", "curl * | bash", "wget * | sh",
                "git push --force <default-branch>", "git push -f <default-branch>",
                "ps e", "ps eww", "read:/proc/*/environ", "write:<outside-workspace>",
                "rm -rf /", "rm -rf ~", "chmod -R 777 /"],
  "forbidden_paths": cli_dirs + generic_creds,
}

credential_env = {
  "names": ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "ANTHROPIC_CUSTOM_HEADERS",
            "CLAUDE_CODE_OAUTH_TOKEN", "OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_ORG_ID", "OPENAI_ORGANIZATION",
            "OPENAI_PROJECT", "CODEX_API_KEY", "CURSOR_API_KEY", "CURSOR_API_ENDPOINT", "COPILOT_GITHUB_TOKEN",
            "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "GEMINI_API_KEY",
            "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL", "GOOGLE_APPLICATION_CREDENTIALS", "DEEPSEEK_API_KEY",
            "OPENROUTER_API_KEY", "GROQ_API_KEY", "MISTRAL_API_KEY", "XAI_API_KEY", "COHERE_API_KEY",
            "DASHSCOPE_API_KEY", "BAILIAN_CODING_PLAN_API_KEY", "MOONSHOT_API_KEY", "KIMI_API_KEY",
            "ZHIPUAI_API_KEY", "ZAI_API_KEY", "MINIMAX_API_KEY", "AMP_API_KEY", "FACTORY_API_KEY",
            "AUGMENT_SESSION_AUTH", "META_API_KEY", "AZURE_OPENAI_API_KEY", "AZURE_OPENAI_ENDPOINT",
            "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_BEARER_TOKEN_BEDROCK"],
  "suffixes": ["_API_KEY", "_APIKEY", "_TOKEN", "_SECRET", "_SECRET_KEY", "_ACCESS_KEY", "_ACCESS_KEY_ID",
               "_PASSWORD", "_BASE_URL"],
}
redaction_patterns = ["sk-ant-[A-Za-z0-9_-]{20,}", "sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{20,}", "sk-sp-[A-Za-z0-9_-]{16,}",
                      "ghp_[A-Za-z0-9]{36}", "github_pat_[A-Za-z0-9_]{60,}", "gh[osu]_[A-Za-z0-9]{36}",
                      "AKIA[0-9A-Z]{16}", "AIza[0-9A-Za-z_-]{35}", "xai-[A-Za-z0-9]{40,}", "glpat-[A-Za-z0-9_-]{20}"]

COMMON_W = ["{worktree}", "{tmp}", "~/.npm/**", "~/.cache/**", "~/.cargo/registry/**", "~/.cargo/git/**",
            "~/go/pkg/mod/**", "~/.gradle/caches/**", "~/.m2/repository/**", "~/Library/Caches/**"]
COMMON_R = ["{project}", "/usr/**", "/opt/homebrew/**", "/Library/Developer/**", "/System/**",
            "~/.rustup/**", "~/.nvm/**", "~/.volta/**", "~/.pyenv/**", "~/.local/bin/**", "~/.gitconfig"]
def sbx(own, keychain=False, extra_read=()):
    return {"write": COMMON_W + own, "read": COMMON_R + own + list(extra_read),
            "deny": ["{other_cli_dirs}", "{forbidden_paths}", "{tokage_app_data}/**"], "keychain": keychain}
sandbox_profiles = {
  "CLAUDE_CODE": sbx(["~/.claude/**", "~/.claude.json"], keychain=True),
  "CODEX_CLI": sbx(["~/.codex/**"]),
  "CURSOR_CLI": sbx(["~/.cursor/**", "~/.config/cursor/**"], keychain=True),
  "COPILOT_CLI": sbx(["~/.copilot/**", "~/.config/github-copilot/**"], keychain=True),
  "ANTIGRAVITY_CLI": sbx(["~/.antigravity/**", "~/.config/antigravity/**", "~/.gemini/**"], keychain=True),
  "GROK_CLI": sbx(["~/.grok/**"]),
  "KIRO_CLI": sbx(["~/.kiro/**"], keychain=True),
  "QWEN_CODE": sbx(["~/.qwen/**"]),
  "KIMI_CODE": sbx(["~/.kimi-code/**"]),
  "MUSE_CODE": sbx(["~/.config/muse/**"]),
  "AIDER": sbx(["~/.aider*"]),
  "OPENCODE": sbx(["~/.config/opencode/**", "~/.local/share/opencode/**", "~/.local/state/opencode/**"]),
  "REASONIX": sbx(["~/.reasonix/**"]),
  "QODER_CLI": sbx(["~/.qoder/**"], keychain=True),
  "AMP": sbx(["~/.config/amp/**", "~/.local/share/amp/**"]),
  "CRUSH": sbx(["~/.config/crush/**", "~/.local/share/crush/**"]),
  "GOOSE": sbx(["~/.config/goose/**", "~/.local/share/goose/**"], keychain=True),
  "CLINE": sbx(["~/.cline/**"]),
  "DROID": sbx(["~/.factory/**"]),
  "AUGGIE": sbx(["~/.augment/**"]),
  "TRAE_CLI": sbx(["~/.trae/**"]),
}

pack = {
  "schema_version": 1,
  "pack_version": "builtin-0.0.1",
  "generated_at": "2026-10-04",
  "domains": D,
  "success_model": {"a": 1.6, "b": 1.2, "prior_strength": 6,
                    "req": {"1": {"S": 2.0, "M": 2.5, "L": 3.0}, "2": {"S": 3.0, "M": 3.5, "L": 4.0},
                            "3": {"S": 4.0, "M": 4.3, "L": 4.5}},
                    "planner_min_skill": 5, "p_min": {"default": 0.8, "difficulty_3": 0.85}},
  "providers": providers,
  "models": models,
  "usage_profiles": usage_profiles,
  "triage_rules": triage_rules,
  "domain_keywords": domain_keywords,
  "usage_patterns": usage_patterns,
  "time_formats": time_formats,
  "noise_rules": noise_rules,
  "gate_allowlist": gate_allowlist,
  "command_policy": command_policy,
  "credential_env": credential_env,
  "redaction_patterns": redaction_patterns,
  "sandbox_profiles": sandbox_profiles,
  "sources": [
    "https://platform.claude.com/docs/en/about-claude/pricing",
    "https://platform.claude.com/docs/en/about-claude/models/overview",
    "https://code.claude.com/docs/en/model-config",
    "https://developers.openai.com/api/docs/pricing",
    "https://developers.openai.com/api/docs/guides/prompt-caching",
    "https://ai.google.dev/gemini-api/docs/pricing",
    "https://ai.google.dev/gemini-api/docs/caching",
    "https://docs.x.ai/docs/models",
    "https://github.com/xai-org/grok-build",
    "https://api-docs.deepseek.com/quick_start/pricing",
    "https://www.alibabacloud.com/help/en/model-studio/qwen3-coder-plus",
    "https://www.alibabacloud.com/help/en/model-studio/coding-plan",
    "https://www.kimi.com/blog/kimi-k3",
    "https://www.kimi.com/code/docs/en/kimi-code/models",
    "https://cursor.com/docs/models/cursor-composer-2-5",
    "https://kiro.dev/docs/models/",
    "https://dev.meta.ai/docs/muse-code/configuration",
    "https://www.minimax.io/blog/minimax-m3",
    "https://models.dev/models/zhipuai/glm-5.2/",
    "https://mistral.ai/news/devstral-2-vibe-cli/",
  ],
}
# Sanity: every regex compiles under Python's engine (close to Rust `regex` for these patterns).
import re
def chk(p):
    re.compile(p.replace("(?P<", "(?P<"))
for r in triage_rules:
    if "pattern" in r: chk(r["pattern"])
for v in usage_patterns.values():
    for r in v: chk(r["pattern"])
for p in time_formats.values(): chk(p)
for r in noise_rules: chk(r["pattern"])
for p in gate_allowlist + redaction_patterns: chk(p)

out = sys.argv[1]
with open(out, "w", encoding="utf-8") as f:
    json.dump(pack, f, ensure_ascii=False, indent=2)
    f.write("\n")
print(len(models), "models,", sum(map(len, offerings.values())), "offerings,", len(providers), "providers")
