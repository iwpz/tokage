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

## 2026-10-13 实机复核

- [x] DeepSeek Flash 在 Tokage 执行路径中完成文件读取、真实回复、resume、copy 和原生会话检索。
- [x] 将仅存在凭据的状态改为 UNKNOWN_AUTH；显式小请求验证通过后才显示 READY；配置和密钥变更失效。
- [x] 修复流式片段换行、最终文本回退、失败写工具误报变更，以及认证 JSON 的 stderr 警告干扰。
- [x] 修复 `/status` 的 PTY Enter 字符；实机读取人民币余额，余额不转化为虚构配额窗口。
- [x] 新增独立 DeepSeek live 脚本及本地认证失败回归测试。
- [x] macOS 外层 Seatbelt 与 Reasonix 自带 Bash 沙箱冲突于 2026-10-14 修复并验收，详见下节；完整桌面流水线仍未验收。上述历史「已完成」仅代表接入代码，不代表所有实机路径已通过。
- [ ] 未改动用户目录 trust 或 sandbox 配置；待验收可信目录写操作、余额不足、异常网络以及其他平台。


### 2026-10-13：认证状态跨进程修复

- [x] 成功验证保存到 Tokage 私有数据目录，24 小时有效；重启后无需重复模型请求。
- [x] 指纹只绑定 Reasonix 配置 / 凭据 / 网络覆盖，排除 PWD、TERM、SHLVL、PATH 等 shell 元数据。
- [x] 不保存密钥；Unix 文件权限 0600、原子替换；配置变更、认证失败、过期或损坏时撤销 / 不接受验证。
- [x] 实机 DeepSeek 默认模型验证与全新进程 READY 检测通过。首次未验证依然显示 UNKNOWN_AUTH，不把配置存在当成认证成功。

### 2026-10-14：macOS Bash 沙箱与执行角色链

- [x] 真实复现 Bash 工具返回 sandbox unavailable，但对话整体仍完成；严格验收要求工具成功及实际输出，不用最终回复中的标记代替命令执行。
- [x] turn 私有 IPC broker 代启工具进程，保留 Reasonix 主进程原有 Seatbelt；工具作为宿主的独立子进程应用单个合并 profile，避免嵌套 sandbox_apply。
- [x] 写范围取 Tokage / 原生 SBPL 交集，禁读规则与网络拒绝继续生效；固定用户配置的禁读 / 网络边界，防止客户端伪造 profile 放宽限制。工具额外拒绝读取 Reasonix .env / credentials；临时 wrapper 受 OS 写保护。
- [x] 不修改用户 config.toml、API Key 或 trust；不启用 yolo / bypassPermissions；未识别的 SBPL 规则拒绝执行。Linux / Windows 保留原启动路径。
- [x] 本机 Reasonix 2.31.0 + DeepSeek Flash：Bash 实际输出、Planner 读取、Executor 实际修复 m.py 与 Python 两组断言、同 session resume 及独立 Reviewer 只读审查通过。
- [x] 本地真实内核回归覆盖写范围交集、只读工作区、原生 / 自定义禁读、凭据禁读、wrapper 不可修改、网络拒绝、客户端断开和 turn 取消；命令进程组纳入已有崩溃恢复登记。
- [x] `scripts/run-live-reasonix.sh` 增加独立 Bash 与执行角色链验收入口。
- [x] 最终验证：native 全量 228 项通过、17 项保持 ignored（含 opt-in 实机测试和 IPC helper）；独立 live 脚本 4 项全部通过；桌面 cargo check / build 及实际桌面二进制 helper 的参数、stdout、stderr、退出码转发通过。全量首次 PTY 回显测试波动，单独与全量复跑均通过，未修改 PTY 实现。
- [ ] 完整桌面 DAG / worktree / commit / merge、已有目录 trust 失败 / 成功路径及其他平台实机验收仍需单独完成。
