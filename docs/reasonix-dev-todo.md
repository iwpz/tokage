# Reasonix Provider 开发清单

> 产品：[Reasonix](https://reasonix.io/)（`npm i -g reasonix` / `brew install esengine/reasonix/reasonix`）  
> Tokage 侧：新增隐藏白名单 CLI（`featured: false`），用户从「添加 CLI」下拉主动加入后可用。  
> 调度路径：`reasonix run … --output-format stream-json`（不做 ACP host）。

## 总原则

- 不写 Reasonix 配置 / 不代管 API Key（只探测；引导 `reasonix setup`）。
- R8：不默认开启 `--yolo` / `bypassPermissions`。
- Key 真值在 `~/.reasonix/.env`；沙箱需放行该目录。
- 模型目录以用户 `config.toml` / `doctor` 为准，知识包只提供 DeepSeek 等常见条目做匹配。
- 目录信任是用户动作：`reasonix trust --dir`（Tokage 不写 trust；`trust_args` 为空）。
- 余额：只走 CLI 官方 `/status`（PTY slash），不向 vendor 发 HTTP；全量探测不默认跑，CLI 卡片手动刷新时执行（docs/06 §4.1）。

---

## P0 — 可探测 + 可调度（已完成）

目标：加入白名单与 Adapter，能 headless 派发任务，默认隐藏。

- [x] `ProviderId::Reasonix` / TS `REASONIX`
- [x] `providers.rs` 注册（`featured: false`，exe `reasonix`）
- [x] 自定义探测：`reasonix doctor --json`（模型 + `key_present`）；缺省回退读 `~/.reasonix/config.toml` + `.env` 变量名
- [x] Adapter：`reasonix run --output-format stream-json` + `--model` / `--resume` / `--copy` / `--permission-mode` / `--dir`
- [x] eventwire 解析（`kind`）+ 最终 `type: result` → Thread / Text / Tool / Denial / Done+Usage
- [x] fixture `reasonix.jsonl` + 解析 / usage / launch / terminal_resume 单测
- [x] `install.rs`（npm / Homebrew）
- [x] `child_env`：`REASONIX_*` + 从 config 收集的 `api_key_env` 名
- [x] 知识包最小条目（DeepSeek flash/pro）+ sandbox `~/.reasonix/**` + forbidden_paths
- [x] 前端：`ProviderId`、mock、标签名（Logo 可无，走字母兜底）

验收：

1. CLI 设置里 Reasonix 出现在「添加 CLI」下拉，默认不在主列表（未安装且未添加时）。
2. 本机已装且 `setup` 过：探测为 READY，模型来自 doctor/config。
3. 对话 / 流水线可用该 Provider 启动 turn；fixture 单测全绿。

---

## P1 — 用量 / 沙箱 / 匹配打磨（已完成）

- [x] `doctor billing --json` → Account 计费通道（PAYG）；不把 `billing_currency` 当成套餐名
- [x] turn `cost_quote` 完整入库（优先 USD identity / valuations.USD；CNY selected 不误入 `cost_usd`）
- [x] 录制真实 CLI 样本（2.31.0）并收紧 `AdapterMeta`：`tested: 2.31.0`，`versions: >= 1.0.0`
- [x] 匹配包：`deepseek/deepseek-flash` 实测核对，`cli_model_arg_verified: true`
- [x] 配额 / 余额不足 usage_patterns（limit / budget / 429）+ Denial 引导 trust
- [x] `trust_args` 为空（CLI 无等价 flag）；未信任目录靠 `permission.untrusted_folder` Denial + `reasonix trust --dir`
- [x] 原生会话透视：读 `~/.reasonix/projects/<slug>/sessions/*.jsonl.meta`（`NATIVE_SESSIONS`）

---

## P2 — 审批 / 余额 / 高级能力（已完成）

- [x] CLI 管理显示余额：卡片「重新探测」时跑 PTY `/status`，解析 `balance ¥…` → `Account.balance`（全量扫描只读缓存）
- [x] 知识包 `usage_command: /status` + `query_provider_quota` 接线
- [x] headless 审批：`read-only` / `auto` / `acceptEdits`（不做 ACP `APPROVAL_CALLBACK`）
- [x] Tokage `fork` ↔ `--copy` 对齐（单测覆盖）
- [x] Provider Logo：官方 `https://reasonix.io/favicon.svg` → `src/assets/providers/reasonix.svg`

验收：

1. Ready 后点该 CLI 卡片刷新：Account 行出现「余额 ¥…」（或 `$…`）。
2. 全量「刷新」不因 Reasonix 而卡在 PTY（余额用上次缓存）。
3. `cargo test -p tokage-native --lib reasonix` 全绿。

---

## 明确不做（当前）

- 不接入 Reasonix Studio 桌面会话（与 CLI 会话目录不互通；CLI 本身已是 2.x）
- 不把 Reasonix 当 API 中转；它是本地 agent CLI
- 不在 Tokage 设置页内嵌完整 `reasonix setup` UI（P0 只给复制命令）
- 不做 ACP host / `APPROVAL_CALLBACK`（headless 用 permission-mode）
- 不做 `--events-jsonl` 遥测接入
