# 11 · 指令授权与凭据隔离 (Command Authorization & Credential Isolation)

返回 [README](../../README.md)

> 现状（2026-10-04）：本章尚未实现。当前只有 Plan（只读参数）/ Direct 两档，Direct 下 Claude Code 与 Copilot 仅预授权 `READONLY_SHELL` 只读命令，Codex 依赖自身 `workspace-write` 沙箱且 `exec` 模式不询问，Cursor 未传权限参数。子进程继承完整登录 Shell 环境（含其他厂商的 Key）。实施计划见 [TODO](../TODO.md) P1.m1、P1.m2 与 P2。

## 1. 职责

1. 模型在 CLI 内发起的命令与文件操作，按用户选择的授权模式放行、询问或拒绝。
2. 维护分级指令名单，随定义包下发，用户可按项目覆盖。
3. 以系统级沙箱把每个 CLI 进程树限制在允许的文件与网络范围内。
4. 阻止模型读取 API Key、Base URL 等凭据与配置；保证 Tokage 自身只读取非密钥字段。
5. 记录每次授权决定，供审计与「始终允许」复用。

命令在 CLI 内部执行，Tokage 不在其执行路径上。因此本章的手段按强度分为四层，越靠前越可靠：

| 层 | 手段 | 性质 |
|----|------|------|
| L1 系统级沙箱 | 包裹整个 CLI 进程树（§5） | 安全边界，绕不过 |
| L2 审批回调 | CLI 执行前回调 Tokage，由用户决定（§4.2） | 精确，依赖 CLI 支持 |
| L3 预授权名单 | 启动时把 allow / deny 规则传给 CLI（§3） | 体验层，按前缀匹配，可被变形写法绕过，不作为安全边界 |
| L4 事后兜底 | git 快照、范围校验、一键还原（[09 §4](../09-guardrails/README.md)） | 只能补救 |

## 2. 授权模式

| 模式 | 自动放行 | 询问 | 拒绝 |
|------|---------|------|------|
| `ask_always` 每次询问 | 只读 | 其余全部（含文件写入） | 红线 |
| `ask_sensitive` 敏感询问（默认） | 只读、项目常规、`write_scope` 内的文件编辑 | 敏感 | 红线 |
| `full_auto` 全自动 | 除红线外全部 | 无 | 红线 |

- 模式只作用于 `EXECUTOR` / `TESTER`。`PLANNER` / `REVIEWER` 在任何模式下只读；`TESTER` 只放行测试类命令。
- 作用域：全局默认 → 项目覆盖 → 会话临时覆盖。

`full_auto` 的额外约束：

- 开启时二次确认，确认框列出风险：模型可在工作区内删除、改写、联网安装依赖，Tokage 只保证红线与沙箱。
- 只对当前项目生效；应用重启后回到 `ask_sensitive`，可选 1 小时 / 当天 / 本会话的时限。
- 必须在 worktree 中执行；每轮开始前记录 git 快照（含未跟踪文件清单）。
- 仅对具备 L1 沙箱的 CLI 开放；不具备的 CLI 在选择器中置灰并说明原因。Windows v1 不开放。
- 顶栏常驻警示条，显示剩余时限与「立即退出全自动」。
- 实现为「宽 allow 名单 + 红线 deny + 沙箱」，不使用 CLI 的跳过全部权限参数（R8）。

## 3. 指令名单

### 3.1 分级

| 级别 | 示例 | 来源 |
|------|------|------|
| `read_only` | `ls` `pwd` `cat` `head` `tail` `wc` `rg` `grep` `git status/diff/log/show/branch` | 定义包 |
| `project` | 项目摘要提取的构建 / 测试 / lint 命令，如 `npm test` `cargo build` | RepoDigest（[02 §3](../02-planning-engine/README.md)）+ 用户确认 |
| `sensitive` | `rm` `mv` `cp -r` `chmod` `git reset/checkout/clean/push/rebase` `npm/pip/cargo install` `docker` `curl` `wget` `ssh`、读写项目内 `.env*` | 定义包 |
| `forbidden` | `sudo` `su`、写工作区外路径、`curl … \| sh`、`git push --force` 到默认分支、读凭据路径（§6.1）、`security`（钥匙串）、`env` / `printenv` 以外的进程环境读取（`ps e`、`/proc/*/environ`） | 定义包，用户只能追加 |

### 3.2 匹配

Tokage 自行审批（L2、「拒绝后续接」）时：

- 按 shell 语法解析为命令段；`&&` `||` `;` `|` `$()` 反引号中的每一段都须放行才放行。
- 解析失败、含 `eval` / `bash -c` / `python -c` 等内联解释器的，一律按 `sensitive`。
- 路径敏感：`rm` 等命令解析目标路径，落在 `write_scope` 内且被 git 跟踪的文件视为可恢复（`ask_sensitive` 下仍询问，但提示「可还原」）；目标在工作区外即 `forbidden`。

传给 CLI 的 L3 规则只是名单的近似投影，不保证与 Tokage 的解析一致。

### 3.3 定义包字段

```json
"command_policy": {
  "read_only": ["ls", "pwd", "cat", "git status", "..."],
  "sensitive": ["rm", "mv", "git push", "npm install", "..."],
  "forbidden": ["sudo", "su", "security", "..."],
  "forbidden_paths": ["~/.ssh/**", "~/.aws/**", "..."]
}
```

本地覆盖存于 `command_rules`（[08 schema](../08-storage/schema.md)），`forbidden` 只能追加，不能删除。

## 4. 各 CLI 的落地方式

### 4.1 能力位

`AdapterMeta.caps` 新增：

| 能力位 | 含义 |
|--------|------|
| `APPROVAL_CALLBACK` | 支持执行前回调外部审批 |
| `TOOL_RULES` | 支持启动时传入 allow / deny 规则 |
| `DENIAL_EVENT` | 结构化输出中能识别「工具被拒绝」 |
| `SHELL_WRAP` | 可替换 / 包装模型命令所用的 shell 或其环境 |
| `OS_SANDBOX` | 可被 Tokage 的系统级沙箱包裹且正常工作 |
| `OWN_SANDBOX` | CLI 自带沙箱 |

### 4.2 现状与方案

| CLI | 现状 | L2 | L3 | 待验证 |
|-----|------|----|----|--------|
| Claude Code | Plan：`--permission-mode plan`；Direct：`acceptEdits` + `READONLY_SHELL` | `--permission-prompt-tool`，由 Tokage 内置 MCP 审批工具 | `--allowedTools` / `--disallowedTools` | MCP 审批工具在 `-p` 下的行为与超时 |
| Codex CLI | `-c sandbox_mode=read-only / workspace-write`，`exec` 不询问 | `exec` 无；`app-server` 协议有审批事件 | 无命令级规则 | `app-server` 稳定性；与外层沙箱能否嵌套 |
| Cursor CLI | Plan：`--mode plan`；Direct 未传权限参数 | 待调研 | 规则写在 `.cursor/cli.json`，与「不改配置」冲突 | headless 默认行为（是否直接执行 shell） |
| Copilot CLI | Plan：`--plan` + deny write/shell；Direct：allow write + 只读 shell | 待调研（ACP） | `--allow-tool` / `--deny-tool` | 被拒事件格式 |

在全部能力确认前的过渡要求：

- Codex Direct 改为默认询问路径：优先 `app-server` 审批，否则降级为 `read-only` + 「拒绝后续接」。
- Cursor Direct 在验证前视为不具备授权能力，只允许 `ask_always` 搭配 `--mode plan`，或由用户在 CLI 面板显式接受风险。

### 4.3 拒绝后续接（通用降级）

适用于不支持 `APPROVAL_CALLBACK` 但支持 `TOOL_RULES` + `DENIAL_EVENT` 的 CLI：

```
本轮以当前名单启动 → CLI 拒绝某命令并输出被拒事件
  → Tokage 暂停该任务，在审批队列展示命令
  → 用户批准：命令加入本会话名单，以 resume 续接本轮（「已批准，请继续」）
  → 用户拒绝：以 resume 告知模型该命令被拒并要求换方案
```

名单变化即启动参数变化，续接后可能失去部分缓存前缀，审批界面提示该成本。

## 5. 系统级沙箱

### 5.1 平台

| 平台 | 实现 | 文件 | 网络 |
|------|------|------|------|
| macOS | `sandbox-exec` + 每个 CLI 一份 SBPL 画像 | 按路径读 / 写 | 只放行到 Tokage 本地代理端口（见 [12](../12-network-proxy/README.md)） |
| Linux | Landlock（spawn 前在子进程内施加），旧内核回退 bubblewrap | 按路径读 / 写 | 网络命名空间或 Landlock 端口规则 |
| Windows | v1 不提供 | — | — |

沙箱包裹整个 CLI 进程，CLI 进程内的读写工具与模型派生的子进程同受约束。

### 5.2 沙箱画像

每个 CLI 一份，随定义包下发（`sandbox_profiles`）：

| 类别 | 内容 |
|------|------|
| 可写 | 当前 worktree、系统临时目录、该 CLI 自身的会话 / 缓存目录、包管理缓存 |
| 可读 | 项目、系统运行库与工具链、该 CLI 自身的配置与凭据 |
| 拒绝 | 其他 CLI 的目录、§6.1 通用凭据路径、Tokage 数据目录 |
| 钥匙串 | 仅授予需要的 CLI（如 macOS 上的 Claude Code） |

Codex 自带沙箱只限制写入，不限制读取，因此同样需要外层画像。若 macOS 上无法嵌套沙箱，Codex 只依赖 §6 的环境裁剪与 shell 包装，剩余风险在 CLI 面板标出；不以关闭 Codex 自身沙箱换取外层沙箱。

## 6. 凭据隔离

### 6.1 凭据路径

随定义包下发（`forbidden_paths`），用户只能追加：

| 类别 | 路径 |
|------|------|
| AI CLI | `~/.claude/` `~/.claude.json` `~/.codex/` `~/.cursor/` `~/.copilot/` `~/.config/github-copilot/` `~/.config/opencode/` `~/.local/share/opencode/` `~/.aider*` 等白名单 CLI 的配置目录 |
| 通用 | `~/.ssh/` `~/.aws/` `~/.config/gcloud/` `~/.azure/` `~/.kube/` `~/.docker/config.json` `~/.npmrc` `~/.netrc` `~/.git-credentials` `~/.config/gh/` |
| 系统 | 钥匙串、浏览器配置目录 |
| Tokage | `app_data_dir()` |

项目内 `.env*` 不在红线内，按 `sensitive` 处理。

### 6.2 模型读取凭据的途径与对策

CLI 进程本身必须能读自己的凭据，而模型的工具运行在该 CLI 中，所以「同一 CLI 的凭据」无法由进程级沙箱单独解决。对策是让模型执行的命令比 CLI 本身权限更小：

| 途径 | 例子 | 对策 | 强度 |
|------|------|------|------|
| 读其他 CLI 的凭据 | Claude 读 `~/.codex/auth.json` | L1 沙箱拒绝 | 强 |
| 读其他厂商的环境变量 | Claude 进程里 `echo $OPENAI_API_KEY` | 子进程环境按厂商裁剪（§6.3） | 强 |
| shell 读本 CLI 配置 | `cat ~/.claude/settings.json` | shell 包装：模型命令在更严格的子沙箱中运行，拒绝凭据路径 | 强（需 `SHELL_WRAP`） |
| shell 读本 CLI 环境变量 | `env`、`echo $ANTHROPIC_API_KEY` | shell 包装：执行前剥离 `*_API_KEY` `*_TOKEN` `*_SECRET` `*_BASE_URL` 等 | 强（需 `SHELL_WRAP`） |
| CLI 内置读文件工具 | Claude Read 工具读 `~/.claude/**` | L3 deny 规则 | 中 |
| 钥匙串 | `security find-generic-password` | 沙箱拒绝 + `forbidden` 命令 | 较强 |
| 读其他进程环境 | `ps eww`、`/proc/<pid>/environ` | Linux 沙箱拒绝；macOS 待验证 | 待验证 |

`SHELL_WRAP` 的候选实现（待逐个验证）：Codex `-c shell_environment_policy.*`；Claude Code 的 shell 相关环境变量或 `SHELL` 指向 Tokage 包装器；Cursor / Copilot 待调研。不具备 `SHELL_WRAP` 的 CLI 在面板上标「凭据隔离：部分」。

### 6.3 子进程环境组装

替代当前「完整登录环境透传」：

```
登录 Shell 环境
  → 移除全部已知厂商凭据变量（定义包 `credential_env` 列表 + 通配 *_API_KEY / *_TOKEN / *_SECRET）
  → 加回该 CLI 自身需要的变量（定义包 `providers.<id>.env_allow`）
  → 代理变量按 [12](../12-network-proxy/README.md) 处理
  → 移除宿主会话变量（现有 `env_remove`，如 CLAUDE_CODE_*）
```

### 6.4 输出兜底

- 落库与展示前脱敏：按内存中已知 Key 的值精确替换，并匹配常见格式（`sk-ant-` `sk-` `ghp_` `github_pat_` `AKIA` 等）。
- 每轮结束扫描 diff，Key 值写入项目文件时告警并阻止「提交」类操作。
- 脱敏只保证 Key 不进入 Tokage 的界面与数据库，无法撤销模型已看到的内容；本限制写在设置页说明中。

## 7. Tokage 自身的读取边界（R11）

Tokage 进程（不是模型）为展示与计费识别读取 CLI 配置：

| 允许读取 | 用途 | 处理 |
|---------|------|------|
| Base URL | CLI 面板展示 | 去掉 userinfo、query、fragment；路径中疑似 token 的长随机段打码 |
| 认证方式、套餐、账号类型 | 计费类型识别（[06 §2](../06-usage-melting-engine/README.md)） | 只取声明字段 |
| Key 变量名、凭据文件是否存在 | 登录态判断 | 不取值 |

| 禁止 | |
|------|-|
| 读取 Key / Token 的值 | 解析 JSON 时读到即丢弃，不进入任何结构体字段 |
| 持久化、写日志、传给前端 | 包括调试日志与崩溃报告 |
| 发送到任何网络端点 | Tokage 除定义包更新外无外网请求 |

### 7.1 来源提醒

Tokage 开源且不上传数据，但无法阻止第三方修改源码后分发。界面在以下位置提醒用户核对来源：

- 首次启动的说明页。
- CLI 面板中展示 Base URL / 账号信息的区域（常驻一行小字）。
- 设置 → 关于。

建议措辞：

> Tokage 会在本机读取 CLI 配置中的 Base URL 与账号类型用于展示，不读取 API Key 的值，不保存、不上传。实现见源码 [src-tauri/src/usage.rs](https://github.com/iwpz/tokage/blob/main/src-tauri/src/usage.rs)。
> 以上承诺只对官方发布或你自行从 [github.com/iwpz/tokage](https://github.com/iwpz/tokage) 编译的版本成立。请勿使用来源不明的安装包。

设计要点：

- 源码链接指向与当前版本对应的 tag（`/blob/v{version}/...`）；开发构建指向构建时的 commit。链接由构建脚本注入，打开方式遵守 [10 §7](../10-frontend/README.md)。
- 「关于」页展示版本、commit、构建类型（官方发布 / 本地构建），以及官方安装包的校验方式：GitHub Releases 发布的 SHA-256、macOS 公证 Team ID、Windows 签名主体。
- 能力边界：被篡改的副本可以删掉这段提醒，应用无法自证未被修改。提醒的作用是告诉用户如何核对，而不是证明安全；可信来源依赖官方签名与校验值。

## 8. 审批界面与审计

- 并行任务的待审批项进入统一队列：任务名、CLI / 模型、完整命令、解析出的风险级别与原因、目标路径是否可恢复。
- 操作：允许一次 / 本会话始终允许 / 写入项目名单 / 拒绝 / 拒绝并说明。
- 等待审批期间暂停该轮的空闲超时计时（[09 §2](../09-guardrails/README.md)），硬超时可配置是否暂停。
- 每次决定写入 `approvals`（[08 schema](../08-storage/schema.md)）。

## 9. 配置项

| 键 | 默认 | 说明 |
|----|------|------|
| `auth.mode` | `ask_sensitive` | `ask_always` / `ask_sensitive` / `full_auto` |
| `auth.full_auto_ttl` | `session` | `1h` / `today` / `session` |
| `auth.pause_idle_timeout_on_prompt` | `true` | |
| `sandbox.enabled` | `true` | 只能按 CLI 关闭，关闭后该 CLI 不可用 `full_auto` |
| `credentials.redact_output` | `true` | 不可关闭 |
