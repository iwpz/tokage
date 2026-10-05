# 11 · 指令授权与凭据隔离 (Command Authorization & Credential Isolation)

返回 [README](../../README.md)

> 现状（2026-10-05）：P1 与 P2 阶段已全部完成并验收。P2.m1 指令名单与匹配器、P2.m2 授权模式与安全设置、P2.m3 审批通道与拒绝后续接、P2.m4 macOS Seatbelt 沙箱、P2.m5 凭据隔离补强（Shell 包装、输出脱敏、Diff 告警）、P2.m6 全自动模式约束与快照、P2.m7 Linux Landlock 与 Bubblewrap 沙箱全部落地，单元测试与 FakeAdapter 场景测试（R13–R17）全绿。详见 [TODO](../TODO.md)。

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

- 模式只作用于 `EXECUTOR` / `TESTER`。`PLANNER` / `REVIEWER` 在任何模式下只读（只放行已解析的 `read_only`，其余拒绝）；`TESTER` 拒绝 `sensitive`，`project` 级（含测试命令）按模式处理。
- 未解析（`resolved: false`）的命令在任何模式下都询问，`full_auto` 也不例外。
- 文件编辑（CLI 自带编辑工具，不经 shell）按 `Policy::classify_edit` 判定：工作区与 `write_scope` 内为 `project`；命中 `write:` 规则（如 `.env*`）、写入 `.git`、超出 `write_scope` 为 `sensitive`；工作区外或凭据路径为 `forbidden`。
- 作用域：全局默认 → 项目覆盖 → 会话临时覆盖，窄的覆盖宽的（`auth::Layers::resolve`）。全局与项目只能存 `ask_always` / `ask_sensitive`；会话覆盖只在内存中，重启即失效。`full_auto` 在该 CLI 不可用时跳过该层，取下一层的模式。

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

模式语法（定义包与 `command_rules` 相同）：

| 写法 | 含义 |
|------|------|
| `git push` | 命令名 + 前缀位置参数；其余参数不限 |
| `git push --force <default-branch>` | 须带该标志；`<default-branch>` 匹配推送目标为默认分支（含 `+ref`、`HEAD:main`、无 refspec 时的当前分支） |
| `node --version` | 放行级别的带标志模式只匹配恰好这些标志，多出参数即不匹配 |
| `curl * \| sh` | 同一管道中左命令在前、右命令在后 |
| `read:<glob>` / `write:<glob>` | 路径规则；不含 `/` 的 glob 匹配文件名，相对路径相对工作区根 |
| `write:<outside-workspace>` | 写入工作区外 |

判定规则：

- 红线命中即 `forbidden`；否则用户规则优先（更具体的胜出），再取定义包中最高级别；未知命令按 `sensitive`。
- 定义包 `read_only` 只在内置包生效；自定义包的 `read_only` 忽略，`sensitive` / `forbidden` / `forbidden_paths` 追加。
- 添加规则时校验：低于 `forbidden` 的规则不能覆盖任何红线命令、管道模式或凭据路径。
- 剥离包装命令后再匹配：`env` `command` `exec` `nohup` `nice` `timeout` `time` `xargs` `stdbuf` 等；`find -exec`、`fd -x`、`watch`、`trap`、`eval`、`bash -c` 与 here-document 中的命令逐条判定。
- 引号、转义、`$'…'`、花括号展开先还原；变量、命令替换得不到静态值时，命令名按通配比对红线，其余记为「未解析」（`resolved: false`），授权模式对未解析命令始终询问。
- 远程脚本：取网络内容的命令（`curl` `wget` 等）其输出以管道、`$(…)`、`<(…)` 进入解释器即 `forbidden`。
- 路径按词法规范化（`..`、`//`、`$HOME`、`~`），不解析符号链接（由沙箱兜底）；读凭据路径即 `forbidden`，递归命令的起点覆盖凭据目录时为 `sensitive`；写入目标（重定向、`rm` `cp` `mv` `tee` `sed -i` `curl -o` `tar -x` 等）在工作区外即 `forbidden`，`cd` 后的相对路径按新目录解析。
- 结果附带原因列表、写入路径与「可还原」判断（目标均被 git 跟踪）。

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
| Codex CLI | Plan / Direct 均为 `-c sandbox_mode=read-only`（P1.m1c 临时降级），`exec` 不询问 | `exec` 无；`app-server` 协议有审批事件 | 无命令级规则 | `app-server` 稳定性；与外层沙箱能否嵌套 |
| Cursor CLI | Plan：`--mode plan`；Direct 不传权限参数，依赖 CLI 默认白名单（见下方实测），配置不安全时 Tokage 拒绝 Direct | 待调研 | 规则写在 `.cursor/cli.json`，与「不改配置」冲突 | headless 默认行为（是否直接执行 shell） |
| Copilot CLI | Plan：`--plan` + deny write/shell；Direct：allow write + 只读 shell | 待调研（ACP） | `--allow-tool` / `--deny-tool` | 被拒事件格式 |

实测记录（2026-10-04，P1.m1d）：

| CLI | 版本 | 条件 | 结果 |
|-----|------|------|------|
| Cursor CLI | 2026.10.01 | `-p --output-format stream-json --trust`，默认配置（`approvalMode: "allowlist"`，`permissions.allow: ["Shell(ls)"]`；全新 HOME 生成的默认值相同） | `rm` 未执行、也不询问；`shellToolCall.result` 为 `{"rejected": {"command", "reason", "isReadonly"}}`，可作为 `DENIAL_EVENT` |
| Cursor CLI | 同上 | 另加 `--force` 搭配 `--mode ask` / `--mode plan` | 模型未调用 shell，文件保留 |
| Codex CLI | 0.160.0 | `exec -s workspace-write -c approval_policy="on-request"` | `rm` 直接执行，无审批事件 |
| Codex CLI | 同上 | `approval_policy="untrusted"` | 已不支持，CLI 报错退出 |
| Codex CLI | 同上 | `exec -s read-only` | 编辑与 `rm` 均被沙箱拒绝（Operation not permitted） |

结论：Cursor headless 的安全性取决于 `~/.cursor/cli-config.json` 与项目 `.cursor/cli.json` 中的 `approvalMode` 与 `permissions.allow`。Tokage 在探测与每轮启动前只读这两个字段：`approvalMode` 不是 `allowlist`，或白名单含非只读的 `Shell(...)` 规则（如 `Shell(*)`、`Shell(rm)`）时，Cursor 不参与 Direct（选择器排除、CLI 面板说明原因）。Tokage 不传 `--force` / `--yolo`。

在全部能力确认前的过渡要求：

- Codex Direct：经 P2.m3d 评估，Codex 的 `app-server` 目前仍为实验性功能（`[experimental]`）且设计为全局共享常驻守护进程，与 Tokage 基于单轮独立进程组隔离和外层系统沙箱包裹（P2.m4）的设计架构冲突；因此确定采用统一的稳态方案——以 `exec -s read-only` 启动，沙箱拦截越权操作生成被拒事件接入「拒绝后续接」，用户审批后切换为 `-s workspace-write` 并通过 `resume` 续接本轮。
- Cursor Direct：默认白名单下可用；配置会让命令免询问执行时拒绝 Direct（P1.m1e）。接入拒绝后续接见 P2.m3e。

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

**P2.m4 实现与验证记录（macOS Seatbelt / SBPL）**：
- **SBPL 画像生成（`tokage_native::sandbox`）**：
  - 基础策略基于 `(version 1) (allow default)`，写入严格受限 `(deny file-write*)`。
  - 写入白名单放行 `/dev/null`、`/dev/zero`、`/dev/tty`、`/dev/fd`，以及 `{worktree}`、`{tmp}`（`/tmp`、`/private/tmp`、系统临时目录）与各 CLI 自身的缓存目录。
  - 凭证红线强阻断：`{forbidden_paths}`、`{other_cli_dirs}`、`{tokage_app_data}/**` 注入为 `(deny file-read* file-write* ...)`。CLI 自身声明在 `read` 或 `write` 中的目录（如 Claude Code 的 `~/.claude/**`）自动排除出 deny 名单，保证本 CLI 正常加载。
  - 钥匙串策略（P2.m4c）：默认阻断钥匙串读取（`~/Library/Keychains`、`/Library/Keychains`）与 `(deny mach-lookup (global-name "com.apple.SecurityServer"))`；仅在画像显式声明 `keychain: true` 时（如 Claude Code OAuth）放行。
  - 进程包装（`chat::run_cli`）：拥有 `Caps::OS_SANDBOX` 的 CLI（Claude Code、Cursor CLI、Copilot CLI）在 macOS 上由 `/usr/bin/sandbox-exec -p <sbpl>` 包装启动。
- **Codex 沙箱嵌套实测结论（P2.m4e）**：
  - 实测在 macOS Seatbelt 沙箱内执行 Codex 读取命令时，Codex 尝试在其内部派生 tool 子沙箱失败并报错：`The command couldn't run because the sandbox failed: Operation not permitted.`
  - 证明 Apple Seatbelt 严格禁止无内核特殊 entitlement 的进程在已沙箱化的进程树内嵌套创建第二层 Seatbelt 沙箱。
  - 按设计结论：Codex 维持 `Caps::OWN_SANDBOX`，**不**被外层 `sandbox-exec` 包装；依赖 Codex 自带沙箱限制写入，读取与环境风险依赖 §6 环境裁剪（R16）与 shell 包装隔离。
- **P2.m7 实现与验证记录（Linux Landlock 与 Bubblewrap 兜底）**：
  - **Landlock 画像与子进程限制（`tokage_native::sandbox::landlock`，P2.m7a）**：
    - 在 Linux 5.13+ 内核上，利用非特权 Landlock LSM 系统调用（`sys_landlock_create_ruleset`、`sys_landlock_add_rule`、`sys_landlock_restrict_self`）在子进程 `pre_exec` 阶段施加沙箱。
    - 规则集声明全面接管文件系统读、写、执行与截断权限（`handled_access_fs`）。
    - 允许只读 / 执行访问系统目录（`/bin`、`/sbin`、`/usr`、`/lib`、`/lib64`、`/etc`、`/proc`、`/sys`、`/dev`）与项目根目录。
    - 允许读写访问当前 worktree、系统临时目录（`/tmp`、`/var/tmp`）、标准设备（`/dev/null`、`/dev/zero`、`/dev/tty`）及 CLI 自身缓存（如 `~/.claude`、`~/.npm`）。
    - 由于 Landlock 采用 Capability 默认全拒绝（default-deny）模型，未被授予读取权限的 `$HOME` 其余目录（含 `~/.ssh`、`~/.aws`、`~/.codex` 等全部 `forbidden_paths` 与 Tokage 数据目录）均被 Linux 内核直接拦截（返回 `EACCES` 权限拒绝）。
  - **Bubblewrap 兜底包装（`tokage_native::sandbox::bwrap`，P2.m7b）**：
    - 当系统未启用 Landlock（内核旧版本或未开启 LSM）时，自动探测并回退至 `bwrap` 启动 CLI。
    - 根目录只读挂载（`--ro-bind / /`），worktree 与临时目录读写绑定（`--bind {worktree} {worktree}`，`--bind-try {tmp} {tmp}`）。
    - 凭据红线与其它 CLI 目录通过 `--tmpfs <dir>`（挂载空 tmpfs 目录遮蔽）及 `--ro-bind-try /dev/null <file>`（映射至空设备）进行强遮蔽，完全阻断外部凭据读取。
    - CLI 自身的配置路径（如 `~/.claude`）保留读写绑定以保证正常运行。
  - **跨平台统一包装（`tokage_native::sandbox::create_sandboxed_command`）**：
    - macOS 使用 `sandbox-exec -p <sbpl>`，Linux 优先选用 `bwrap` 或内核 `Landlock`，无沙箱能力时安全降级。
    - 拦截用例验证：在 Linux 与 macOS 上，FakeAdapter 越权读取凭据路径（3 项红线被阻断）与全自动模式越界写入（工作区外 `rm` 被拦截、外部文件毫发无损）的测试均保持全绿。

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
| 读其他进程环境 | `ps eww`、`/proc/<pid>/environ` | Linux 沙箱拒绝；macOS 实测验证：BSD `ps` 默认抑制跨进程环境变量输出，macOS 无 `/proc` 文件系统，mach `task_for_pid` 受系统 SIP 限制且被 Seatbelt 沙箱严密阻断，同用户普通进程无法读取他人环境 | 强 |

`SHELL_WRAP` 的实现（P2.m5a 已落地，Claude Code、Codex CLI、Copilot CLI 均具备 `Caps::SHELL_WRAP`）：
- **Claude Code**: Bash 工具遵循 `$SHELL`，Tokage 在启动前生成包装脚本 `/tmp/tokage/tokage-shell-wrapper.sh`（自动 `unset` 所有已知厂商凭据与通配 `*_API_KEY`、`*_TOKEN`、`*_SECRET` 等变量），并将 `SHELL` 指向该脚本。
- **Codex CLI**: 原生参数支持，注入 `-c shell_environment_policy.inherit="core"` 剥离非核心系统变量。
- **Copilot CLI**: 原生参数支持，注入 `--secret-env-vars=<CREDENTIAL_ENV>`，Copilot 自动从 shell 及 MCP 子进程中剔除并在输出中打码。

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

- 落库与展示前脱敏（`tokage-native::chat::redact::Redactor`）：
  - 启动本轮时提取 `env.vars` 中全部已知凭据值（长度 ≥ 6），按长度倒序精确替换为 `[REDACTED]`。
  - 加载定义包 `builtin.json` 中的 10 类正则脱敏规则（`redaction_patterns`，涵盖 `sk-ant-`、`sk-`、`ghp_`、`github_pat_`、`AKIA`、`AIza` 等标准 API Key 格式）编译为正则统一替换。
  - 所有流式文本、思考块、工具详情、工具输出、文件变更 Diff、审批请求与错误消息在发布到 `Bus`、推向前端或存入 SQLite 前均经过脱敏。
- 每轮结束扫描 diff：若在 `FileChange` 的新增内容中检测到敏感 Key，立即发出 `EventKind::Warning` 警示事件并在 UI 顶部高亮显示。
- 脱敏只保证 Key 不进入 Tokage 的界面与数据库，无法撤销模型已看到的内容；本限制写在设置页说明中。

## 7. Tokage 自身的读取边界（R11）

Tokage 进程（不是模型）为展示与计费识别读取 CLI 配置：

| 允许读取 | 用途 | 处理 |
|---------|------|------|
| Base URL | CLI 面板展示 | 去掉 userinfo、query、fragment；路径中疑似 token 的段打码为 `***`（`usage::display_url`） |
| 认证方式、套餐、账号类型 | 计费类型识别（[06 §2](../06-usage-melting-engine/README.md)） | 只取声明字段 |
| Key 变量名、凭据文件是否存在 | 登录态判断 | 不取值 |

| 禁止 | |
|------|-|
| 读取 Key / Token 的值 | 解析 JSON 时读到即丢弃，不进入任何结构体字段 |
| 持久化、写日志、传给前端 | 包括调试日志与崩溃报告 |
| 发送到任何网络端点 | Tokage 除定义包更新外无外网请求 |

疑似 token 的路径段：以常见 Key 前缀开头（`sk-` `ghp_` `github_pat_` `AIza` `AKIA` 等），或长度 ≥ 32，或长度 ≥ 16 且同时含 3 个以上字母与数字。`v1`、`openai`、`chat-completions` 这类可读段保留。

测试 `usage::tests::secrets_are_dropped_on_read` 把 Key 写进环境变量、`settings.json`、`auth.json`、`config.toml` 与 Base URL 的 userinfo / 路径 / query，断言 `Account` 的 IPC JSON 与 `Debug` 输出中均不出现这些值。

### 7.1 来源提醒

Tokage 开源且不上传数据，但无法阻止第三方修改源码后分发。界面在以下位置提醒用户核对来源：

- 首次启动的说明页（`WelcomeNotice`，确认后才弹出工作区信任对话框）。
- CLI 面板中展示 Base URL / 账号信息的区域（常驻一行小字）。
- 设置 → 关于。

三处共用 `SourceNotice` 组件。

建议措辞：

> Tokage 会在本机读取 CLI 配置中的 Base URL 与账号类型用于展示，不读取 API Key 的值，不保存、不上传。实现见源码 [src-tauri/crates/tokage-native/src/usage.rs](https://github.com/iwpz/tokage/blob/main/src-tauri/crates/tokage-native/src/usage.rs)。
> 以上承诺只对官方发布或你自行从 [github.com/iwpz/tokage](https://github.com/iwpz/tokage) 编译的版本成立。请勿使用来源不明的安装包。

设计要点：

- 源码链接指向与当前版本对应的 tag（`/blob/v{version}/...`）；本地构建指向构建时的 commit，不在 git 仓库中构建时指向默认分支。`build.rs` 注入 `TOKAGE_COMMIT` 与 `TOKAGE_BUILD_KIND`；只有发布流水线设置 `TOKAGE_OFFICIAL_BUILD=1` 时构建类型为「官方发布」。前端经 `build_info` 命令读取，打开方式遵守 [10 §7](../10-frontend/README.md)。
- 「关于」页展示版本、commit、构建类型（官方发布 / 本地构建），以及官方安装包的校验方式：GitHub Releases 发布的 SHA-256、macOS 公证 Team ID、Windows 签名主体。发布渠道确定前三项显示为占位（P8.m6e 补全）。
- 能力边界：被篡改的副本可以删掉这段提醒，应用无法自证未被修改。提醒的作用是告诉用户如何核对，而不是证明安全；可信来源依赖官方签名与校验值。

## 8. 审批界面与审计

- 并行任务的待审批项进入统一队列：任务名、CLI / 模型、完整命令、解析出的风险级别与原因、目标路径是否可恢复。
- 操作：允许一次 / 本会话始终允许 / 写入项目名单 / 拒绝 / 拒绝并说明。
- 等待审批期间暂停该轮的空闲超时计时（[09 §2](../09-guardrails/README.md)），硬超时可配置是否暂停。
- 每次决定写入 `approvals`（[08 schema](../08-storage/schema.md)）。

## 9. 配置项

| 键 | 默认 | 说明 |
|----|------|------|
| `settings.authMode` | `ask_sensitive` | 全局默认：`ask_always` / `ask_sensitive` |
| `settings.authProjectModes` | `{}` | 按项目 id 覆盖；未设置即继承 |
| `auth.full_auto_ttl` | `session` | `1h` / `today` / `session` |
| `auth.pause_idle_timeout_on_prompt` | `true` | |
| `sandbox.enabled` | `true` | 只能按 CLI 关闭，关闭后该 CLI 不可用 `full_auto` |
| `credentials.redact_output` | `true` | 不可关闭 |
