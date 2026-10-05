# Tokage 开发进度与里程碑

返回 [README](../README.md) · 更新于 2026-10-05

本文对照 00–12 设计文档，把后续开发拆成阶段（P1–P8）→ 里程碑（m1 / m2 …）→ 子里程碑（m1a / m1b …）。条目编号写作 `P2.m3a`，其他文档引用时使用该编号。

## 使用约定

- 每个条目是一个可独立交付的任务。完成后把 `- [ ]` 改为 `- [x]`（渲染为 ✓）。
- 里程碑内全部条目完成、且「验收」条目通过后，在里程碑标题末尾加 ✓。阶段同理。
- 每开发完一个子里程碑 / 子 stage（如 P5.m4、P5.m5a 等），都必须及时同步 TODO 文档更新状态，并创建对应的独立 git commit。
- 开发中发现新的工作项，追加到对应里程碑；不确定归属的放到「待决问题」。
- 同一里程碑内的条目默认可并行；标注「依赖」的必须等依赖项打 ✓。
- 每个阶段结束时应用必须保持可用，现有对话功能无回归。

## 一、现状总览

目前是一个可用的多 CLI 对话客户端：探测本机 CLI、选模型、在项目中以 Plan / Direct 模式对话、查看工具步骤与 Diff、按会话建 worktree、从自定义 URL 加载定义包并查看。设计核心闭环（分流 → 规划 → 匹配 → 编排 → 验收 → 学习）尚未实现。

| 模块 | 进度 | 主要阶段 | 说明 |
|------|------|---------|------|
| 00 总体架构 | 部分 | P1 | 已拆出 core / native / storage 三个 crate，`DomainEvent` 总线接入；IPC 类型手写（不采用 specta，理由见 00 §7） |
| 01 原生环境 | 部分 | P1 / P4 | 探测、`ProviderAdapter` trait 与 4 个 headless Adapter、能力位与版本区间、`FakeAdapter` 完成；无 PTY、原生会话透视 |
| 02 规划引擎 | 部分 | P1 / P5 | 仅 Plan 模式提示词 + 只读权限；计划写入项目内 `.tokage/plans/`；无分流、摘要、计划结构 |
| 03 编排引擎 | 未开始 | P7 | |
| 04 模型匹配 | 部分 | P6 | 模型目录聚合、自定义 URL 定义包加载与查看已完成；内置包、分层、分配未开始 |
| 05 上下文引擎 | 部分 | P8 | 仅有续接 / 换模型时回放历史 |
| 06 用量与熔断 | 部分 | P3 / P8 | 计费类型、套餐、Codex 额度可读；无被动额度采集，用量不落库，无熔断 |
| 07 会话树 | 部分 | P7 | 项目 → 会话两级；无计划 / 任务 / 执行窗口层级 |
| 08 存储 | 部分 | P1 | SQLite 首批表、`command_rules`、迁移与备份、旧数据导入完成；计划 / 任务等表随后续阶段建立 |
| 09 防护 | 部分 | P2–P3 | 进程组回收、无 shell 拼接、R11 读取边界、R13–R17 授权与沙箱防护、双层超时监督（R2）、输出过滤流水线（R1）、角色权限与只读改动告警（R7）、进程卫生与崩溃残留清理（R3）完成 |
| 10 前端 | 部分 | 各阶段 | 对话工作区完成；无计划审阅、流水线、用量看板 |
| 11 指令授权与凭据隔离 | 完成 | P2 | 指令名单与匹配器、授权模式判定与设置页、审批通道与拒绝后续接、macOS 与 Linux 系统级沙箱、凭据隔离补强与全自动模式全部完成 |
| 12 网络代理 | 未开始 | P3 | 登录环境中的代理变量原样透传 |

P1.m1 已止血：子进程环境按厂商裁剪；Codex Direct 暂为只读；Cursor 默认白名单下不执行非只读命令，配置放宽时 Tokage 停用其 Direct。

P1.m2 已完成：Base URL 规范化与打码、R11 泄露测试、构建来源注入、首次启动说明页、CLI 面板与「关于」页来源提醒。

P1.m3 已完成：数据存于 SQLite（WAL、单写线程、读连接池、refinery 迁移与启动前备份），localStorage 旧数据首次启动一次性导入，失败时回退并可重试。

P1.m4 已完成：4 个 CLI 经 `ProviderAdapter` 注册表启动，`AdapterMeta` 声明能力位与已测版本区间；超出区间标记 `UNSUPPORTED_VERSION` 仍可对话，无法解析的输出以纯文本展示；`FakeAdapter` 覆盖回放、卡死、刷屏、孙进程、越界写入、读凭据路径、打印环境变量。

P1.m5 开发完成（验收待手动回归）：Rust 代码拆为 `tokage-core`（领域类型、事件模型、`DomainEvent` 总线）、`tokage-native`、`tokage-storage` 三个 crate，应用层只保留 Tauri command；对话输出经总线转发到前端；specta 暂不采用。

P2 阶段已全部完成并验收（P2.m1–P2.m7）：
- P2.m1 四级名单随定义包下发、`command_rules` 本地覆盖与 shell 语法解析匹配器（`tokage-core::command_policy`）。
- P2.m2 授权模式判定逻辑（`tokage-core::auth`）与设置页「权限与安全」。
- P2.m3 4 个 CLI 的审批通道与「拒绝后续接」（`ApprovalCard`、`approvals` 审计表）。
- P2.m4 macOS Seatbelt / SBPL 系统沙箱框架与画像。
- P2.m5 凭据隔离补强（各 CLI shell 包装剥离凭据变量、输出脱敏 Redactor、Diff 敏感 Key 检测告警）。
- P2.m6 全自动模式（时限二次确认、强制 worktree 与 git 快照、沙箱强校验、顶栏警示条）。
- P2.m7 Linux 系统级沙箱（Landlock 画像与子进程限制、Bubblewrap 兜底包装）。
- FakeAdapter 场景（读凭据路径、打印环境变量、执行 forbidden 命令、`rm` 工作区外路径）全部被系统沙箱拦截或进入审批，R13–R17 测试全绿。

## 二、阶段顺序

```
P0 已完成基线
P1 安全止血与底座   ── 风险止血 → R11 → SQLite → Adapter 抽象 → 工程结构 → 项目上下文目录
P2 授权与沙箱       ── 指令名单 → 授权模式 → 审批通道 → macOS 沙箱 → 凭据补强 → 全自动 → Linux
P3 执行防护、用量与代理
P4 Provider 扩展
P5 规划引擎
P6 模型匹配引擎     ── m6（学习）与 P7 交错
P7 编排引擎
P8 上下文、熔断、看板与发布
```

## 三、P0 已完成基线 ✓

已实现、无需再开发的能力，供回归测试对照。

### 原生环境（01）

- [x] 登录 Shell 环境抓取：`$SHELL -ilc`、5s 超时、仅内存、可重新加载（`env.rs`）
- [x] Provider 白名单注册表（约 20 个 CLI），featured / 手动添加，自定义路径（`providers.rs`）
- [x] 探测：which → `--version` → 登录态（只查存在性）→ 模型列表，并发 + 单 Provider 40s 超时
- [x] `ProviderStatus` 七种状态；安装 / 登录命令仅供复制（`install.rs`）
- [x] 模型目录聚合：按底层模型合并 effort / fast / 上下文变体（`models.rs`）
- [x] Headless Adapter：Claude Code、Codex CLI、Cursor CLI、Copilot CLI，含录制样本快照测试
- [x] 统一事件模型：Thread / Model / Reasoning / Text / ToolStart / ToolEnd / FileChange / Done / Error
- [x] 续接（resume）、Claude 原生 fork、换模型时回放历史
- [x] 工作区信任：只对用户授权的 CLI 传递信任
- [x] Plan 模式：首轮附加规划指令、使用各 CLI 只读参数
- [x] Direct 模式下 Claude / Copilot 仅预授权 `READONLY_SHELL`

### 用量（06）

- [x] 计费类型识别：订阅 / 按量 / 第三方后端（`usage.rs`）
- [x] Claude / Codex / Cursor / Antigravity 账号套餐识别（不读 token）
- [x] Codex 额度：从 rollout 日志读 `rate_limits`（主 / 周窗口、重置时间）
- [x] Base URL 规范化展示：去掉 userinfo / query / fragment，疑似 token 的路径段打码
- [x] 逐轮用量：`Done` 带 input / output / cost / premium requests

### 会话与存储（07 / 08）

- [x] 项目 → 会话（侧边栏），会话重命名 / 归档 / fork
- [x] 对话事件实时推送（`chat-event`）
- [x] 取消运行中的轮次（整组回收）
- [x] 项目外文件副本：`app_data_dir()/files/<项目>/<日期>/<文件名>`

### 防护（09）

- [x] R3 进程组回收：`setsid` + `killpg` SIGTERM → 3s → SIGKILL（`proc.rs`）
- [x] R4 不经 shell 拼接（Adapter、open_in、git 均按参数数组）
- [x] R8 不开启跳过权限确认
- [x] R11 读取部分：凭据文件只取套餐 / 认证方式字段，token 读到即丢弃

### 定义包（04）

- [x] 从自定义 URL 下载定义包：后端 curl，仅 https（localhost 可 http），5MB / 30s 上限（`knowledge.rs`）
- [x] 定义包解析与校验，非致命问题列为警告（`lib/knowledge.ts`）
- [x] 设置页：加载前确认、「第三方」标注、模型列表与领域分查看、恢复内置

### 前端（10）

- [x] 标题栏、侧边栏、可拖拽分栏、主题（亮 / 暗 / 系统）、中英文
- [x] 对话视图：推理、正文、工具步骤、文件变更、Markdown（sanitize）
- [x] Diff 视图、文件预览面板
- [x] 模型选择器（变体合并、effort / fast / auto tier）
- [x] Provider 状态菜单、CLI 管理设置页
- [x] 项目 git 信息、在 Finder / VS Code / 终端中打开、会话 worktree
- [x] 信任确认弹窗
- [x] 设置页 UI 骨架：规划 / 上下文 / 预算 / 熔断 / 超时 / 并发 / 隔离 / 定义包（后端尚未使用，各项在对应里程碑接入）
- [x] 输入框 Plan / Direct 模式切换

## 四、P1 安全止血与底座

现有 Direct 模式已有风险，先止血；再把现有能力挪到设计的结构上，后续阶段才有地方落脚。

### P1.m1 风险止血（最高优先） ✓

- [x] P1.m1a 子进程环境按厂商裁剪（R16）：定义 `credential_env` + `env_allow`，替代完整透传登录环境（`chat/mod.rs`）
- [x] P1.m1b 子进程环境裁剪测试：每个 CLI 的子进程看不到其他厂商的 Key 变量
- [x] P1.m1c Codex Direct：改为可询问路径，或在审批通道就绪前降级为 read-only
- [x] P1.m1d Cursor：实测 headless 默认行为（是否执行命令、是否询问），结果写入 11 §4.2
- [x] P1.m1e Cursor：实测完成前限制 Direct 模式，UI 标注原因
- [x] 验收：Codex / Cursor 在 Direct 下无法不经确认执行 `rm`；子进程环境测试全绿（`chat::live::direct_rm_is_not_run`、`chat::child_env::tests`）

### P1.m2 R11 读取边界与来源提醒 ✓

- [x] P1.m2a Base URL 规范化展示：完整 URL 去掉 userinfo / query / fragment，路径中疑似 token 的段打码（`usage::display_url`）
- [x] P1.m2b 密钥字段「读即丢弃」测试：凭据文件解析结果、日志、IPC 输出中均不出现 Key / Token 值（`usage::tests::secrets_are_dropped_on_read`；解析路径无日志输出）
- [x] P1.m2c 构建脚本注入版本、commit、构建类型（官方发布 / 本地构建）与对应 tag / commit 的源码链接（`build.rs`、`build_info` 命令）
- [x] P1.m2d 首次启动说明页（含 11 §7.1 建议措辞）
- [x] P1.m2e CLI 面板 Base URL / 账号信息区域常驻来源提示
- [x] P1.m2f 设置 → 关于：版本 / commit / 构建类型 / 源码链接 / 官方包校验方式（SHA-256、公证 Team ID、签名主体，渠道确定前留占位）
- [x] P1.m2g 项目 README 醒目位置写明 R11 读取边界与来源声明
- [x] 验收：R11 测试全绿；三处提醒在中英文下均可见

### P1.m3 SQLite 存储 ✓

- [x] P1.m3a 接入 rusqlite（bundled）：WAL、单写线程批量提交、读连接池（`storage/mod.rs`、`storage/writer.rs`）
- [x] P1.m3b refinery 迁移框架与启动前备份
- [x] P1.m3c 建首批表：`projects`、`main_sessions`、`attempts`、`chat_logs`、`usage_events`、`provider_health`、`settings`、`custom_packs`（`migrations/V1__init.sql`）
- [x] P1.m3d 从 localStorage（`tokage.projects` / `tokage.chat` / `tokage.settings` / `tokage.providers` / `tokage.ui`）一次性迁移，失败可重试且不丢原数据
- [x] P1.m3e 前端 store 改为经 IPC 读写后端（`lib/db.ts`）
- [x] P1.m3f 定义包数据（当前 `tokage.knowledge`）一并迁移
- [x] 验收：重启后项目、会话、对话、设置从 SQLite 恢复；迁移过程有单元测试（`storage::repo::tests`、`storage::tests`、`src/lib/db.test.ts`）

### P1.m4 Adapter 抽象 ✓

- [x] P1.m4a 抽出 `ProviderAdapter` trait 与 `AdapterMeta`：能力位（含 11 §4.1 授权与沙箱能力位）、版本区间（`chat/adapter.rs`）
- [x] P1.m4b 现有 4 个 Adapter 迁入 trait，快照测试保持全绿（`adapter::tests::registry_samples`）
- [x] P1.m4c 版本超出区间时 `UNSUPPORTED_VERSION` 降级与 UI 提示：仍可对话，CLI 面板给出已测区间；无法解析的输出回退为纯文本
- [x] P1.m4d `FakeAdapter`：回放录制样本；可注入卡死 / 刷屏 / 孙进程 / 越界写入 / 读凭据路径 / 打印环境变量（`chat/fake.rs`）
- [ ] 验收：现有对话功能无回归；FakeAdapter 可驱动集成测试

### P1.m5 工程结构

- [x] P1.m5a 决定 crate 拆分粒度并拆出 `tokage-core` / `tokage-native` / `tokage-storage`：按阶段逐步拆，其余 crate 在所属阶段开始时拆出（`src-tauri/crates/`，Cargo workspace）
- [x] P1.m5b 内部 `DomainEvent` 总线：`tokage_core::Bus`，不丢事件、按序送达；对话输出经总线转发为 `chat-event`（`bus::tests`）
- [x] P1.m5c 评估 specta 生成 IPC 类型：暂不采用，理由记录于 00 §7
- [ ] 验收：`cargo test` 与前端构建通过；P1 阶段整体无功能回归

### P1.m6 项目上下文目录

计划等产物由 Tokage 写入项目内 `.tokage/`，不依赖各 CLI 的私有目录（如 Claude Code 的 `~/.claude/plans/`）；通过 `AGENTS.md` 让各 CLI 读到同一份上下文。

- [x] P1.m6a Plan 轮次不触发 CLI 自带的计划模式落盘：计划正文从输出流获取；Claude 子进程不再向 `~/.claude/plans/` 写文件
- [x] P1.m6b Tokage 将计划写入 `<project>/.tokage/plans/<session-id>.md`，文件名用会话 ID / 标题，不用提示词前缀；修订时覆盖并保留版本记录
- [x] P1.m6c 定义 `.tokage/` 目录结构（`plans/`、`context.md` 约束与任务状态）及读写边界，写入 08 存储文档与 02 规划文档
- [x] P1.m6d `AGENTS.md` 接入：不存在时生成，存在时追加 Tokage 段落（标记包裹、可重复更新），指向 `.tokage/`；`CLAUDE.md` 以 `@AGENTS.md` 引用
- [x] P1.m6e 首次写入 `.tokage/` 时询问是否加入 `.gitignore`，选择存入项目设置
- [ ] 验收：同一计划在 Claude / Codex / Cursor / Copilot 后续轮次中均可被读取；`~/.claude/plans/` 无新增文件

## 五、P2 授权与沙箱 ✓

依赖：P1.m4（能力位）。

### P2.m1 指令名单

- [x] P2.m1a 四级名单（read_only / project / sensitive / forbidden）数据结构，定义包下发
- [x] P2.m1b `command_rules` 用户覆盖；forbidden 只能追加不能删除
- [x] P2.m1c Shell 语法解析匹配：组合命令逐段判定、内联解释器（`python -c` 等）按敏感、路径敏感判断
- [x] P2.m1d 匹配器单元测试（含绕过样例）
- [x] 验收：forbidden 命令在任何模式下被拒绝（R13）：匹配器对红线给出 `forbidden`，用户规则与自定义包均无法降级；各模式据此拒绝的接入见 P2.m2a

### P2.m2 授权模式 ✓

- [x] P2.m2a 三档模式 `ask_always` / `ask_sensitive` / `full_auto` 的判定逻辑
- [x] P2.m2b 作用域：全局 → 项目 → 会话，逐层覆盖
- [x] P2.m2c 设置页「权限与安全」
- [x] 验收：同一命令在三档模式下的放行 / 询问 / 拒绝结果与文档一致（`tokage-core::auth::tests` 按 11 §2 的表逐行断言）

### P2.m3 审批通道 ✓

- [x] P2.m3a Claude Code：`--allowedTools` / `--disallowedTools` 与 `stream-json` 拦截，接入 `DENIAL_EVENT` 与 `--resume` 续接
- [x] P2.m3b 拒绝后续接通用流程（11 §4.3）：被拒后捕获为 `Ev::Denial` 并转换为 `ApprovalRequest` 挂起，用户授权后更新权限名单并通过 `--resume` 续接
- [x] P2.m3c Copilot：识别 `tool.execution_complete` 拒绝事件，输出 `Ev::Denial` 接入拒绝后续接
- [x] P2.m3d Codex：read-only 沙箱拦截作为被拒事件接入拒绝后续接，用户授权后放行 `workspace-write`
- [x] P2.m3e Cursor：基于 `shellToolCall.result.rejected` 捕获 `Ev::Denial` 接入拒绝后续接
- [x] P2.m3f 审批卡片 UI（`ApprovalCard`）、`approvals` 审计表与 IPC (`list_approvals` / `resolve_approval`)、设置页「授权审计日志」
- [x] 验收：4 个 Adapter（以及 FakeAdapter）在 `ask_sensitive` 下遇到敏感命令均能触发被拒拦截、生成审批请求并支持续接（单元测试与回放断言全绿）

### P2.m4 macOS 系统级沙箱 ✓

- [x] P2.m4a `sandbox-exec` SBPL 画像框架；每个 CLI 的 `sandbox_profiles`（`tokage_native::sandbox`）
- [x] P2.m4b 凭据路径红线 `forbidden_paths`（11 §6.1）注入为 deny 规则
- [x] P2.m4c 钥匙串规则：默认禁止读取钥匙串与 `com.apple.SecurityServer`，仅在画像中声明 `keychain: true` 时放行
- [x] P2.m4d 逐 CLI 实测在沙箱内正常运行，`chat::run_cli` 启用系统级沙箱包装
- [x] P2.m4e Codex 自带沙箱嵌套验证（嵌套会触发 `Operation not permitted`，故 Codex 维持 `Caps::OWN_SANDBOX` 不由外层沙箱包装）
- [x] 验收：FakeAdapter 读凭据路径被系统级拦截（`sandbox_intercepts_credential_redline_reads`，R14、R15 测试全绿）

### P2.m5 凭据隔离补强 ✓

- [x] P2.m5a Shell 包装：模型命令子沙箱 + 剥离凭据变量（逐 CLI 实现：Copilot `--secret-env-vars`，Codex `shell_environment_policy`，Claude `$SHELL` wrapper 脚本；更新 `Caps::SHELL_WRAP`）
- [x] P2.m5b 实测 macOS 上 `ps eww` 能否读到同用户其他进程环境，结论写入 11（macOS BSD ps 默认抑制跨进程环境变量，mach task port 受 SIP 保护且被沙箱阻断）
- [x] P2.m5c CLI 内置读文件工具的 deny 规则（`tokage-core::command_policy::Policy::classify_read` 对 `forbidden_paths` 判定为 Forbidden；适配各 CLI 内部文件读取工具报错转 `Ev::Denial`）
- [x] P2.m5d 输出脱敏（R17：`Redactor` 模块内存 Key 精确匹配 + 10 类正则 API Key 脱敏为 `[REDACTED]`）
- [x] P2.m5e diff 中出现 Key 时告警（`EventKind::Warning` 联动前端告警徽章）
- [x] 验收：FakeAdapter 打印环境变量、读 Key 文件均无明文泄露（R16、R17），diff 中出现 Key 时触发告警事件（`print_env_redaction_prevents_secret_leak`、`key_output_and_reads_are_redacted`、`diff_with_secret_triggers_warning_and_redaction` 测试全绿）

### P2.m6 全自动模式 ✓

依赖：P2.m4。

- [x] P2.m6a 二次确认与时限（`FullAutoConfirmDialog` 1h / 当天 / 本会话，过期自动回退 `ask_sensitive`）
- [x] P2.m6b 强制 worktree 与快照（不在 linked_worktree 则报错禁止执行；每轮开始前自动记录 git 快照与未跟踪文件清单至 `.tokage/snapshots/`）
- [x] P2.m6c 仅限有沙箱的 CLI；Windows v1 不开放（`run_turn` 校验阻断，前端在 Windows 或无 `OS_SANDBOX` 下禁用并说明原因）
- [x] P2.m6d 全自动警示条（`FullAutoBanner` 顶部常驻警示条，显示剩余时限与一键退出按钮）
- [x] 验收：全自动下 `rm` 工作区外路径被沙箱拦截（`full_auto_rm_outside_workspace_is_blocked_by_sandbox` 测试全绿）

### P2.m7 Linux 沙箱 ✓

- [x] P2.m7a Landlock 画像（`tokage_native::sandbox::landlock`，Linux 5.13+ 内核 `pre_exec` 系统调用施加沙箱）
- [x] P2.m7b bubblewrap 兜底（`tokage_native::sandbox::bwrap`，只读挂载根目录、绑定 worktree / tmp、空 tmpfs 与 /dev/null 遮蔽凭据红线）
- [x] 验收：P2.m4 的拦截用例在 Linux 上通过（`sandbox_intercepts_credential_redline_reads` 与 `full_auto_rm_outside_workspace_is_blocked_by_sandbox` 跨 macOS/Linux 全绿）

P2 阶段验收：

- [x] FakeAdapter 场景（读凭据路径、打印环境变量、执行 forbidden 命令、`rm` 工作区外路径）全部被拦截或进入审批；R13–R17 测试全绿

## 六、P3 执行防护、用量与代理

### P3.m1 超时监督（R2）✓

- [x] P3.m1a `supervise`：空闲 / 硬超时双层监督
- [x] P3.m1b 超时档位与倍数，接入设置页已有选项
- [x] P3.m1c 超时后处置（回收、标记、可重试）；等待审批时暂停空闲计时
- [x] 验收：FakeAdapter 卡死场景按档位终止

### P3.m2 OutputFilter（R1）✓

- [x] P3.m2a 流水线：CR 折叠、ANSI 去除、spinner、重复折叠、64KB 截断
- [x] P3.m2b 与 P2.m5d 脱敏合并为同一流水线
- [x] 验收：FakeAdapter 刷屏场景内存与 UI 稳定

### P3.m3 完整用量 ✓

- [x] P3.m3a `TurnUsage` 补齐缓存读写、`api_calls`、`context_tokens`、`source`（`tokage-core::event::TurnUsage` / `UsageSource`，4 个 Adapter 解析全部对齐）
- [x] P3.m3b 每轮写入 `usage_events`（依赖 P1.m3：`tokage-storage::repo::UsageEventRow`，`start_chat_turn` 轮次结束自动写入 SQLite WAL 并暴露 IPC `list_usage_events`）
- [x] 验收：4 个 Adapter 的用量字段与录制样本一致（`chat::adapter::tests::sample_usage_fields_match_fixtures` 断言 4 CLI 样本 exact match）

### P3.m4 角色权限 ✓

- [x] P3.m4a PLANNER / EXECUTOR / TESTER / REVIEWER 映射到授权模式与 CLI 参数，含 `write_scope`（`auth::Role`，4 个 Adapter 启动参数映射，`write_scope` 作用域与越界检查）
- [x] P3.m4b 只读窗口检测到改动即终止并告警（R7）（`run_cli` 收到 `Ev::File` 或 `files.finish` 发现改动即回收进程组，发出 `Warning` 与 `Error`）
- [x] 验收：PLANNER 窗口写文件被终止（`chat::fake::tests::planner_window_file_write_is_terminated_r7`、`reviewer_window_file_write_is_terminated_r7`、`planner_window_disk_change_detected_and_terminated_r7`）

### P3.m5 进程卫生 ✓

- [x] P3.m5a 异常退出残留清理（`run/children.json`，启动时 `cleanup_leftovers` 探测未回收进程组并 SIGTERM/SIGKILL 终止，退出与运行时自动原子读写与清理）
- [x] P3.m5b 子进程 `nice +5`（`proc::isolate` 在 Unix `pre_exec` 中调用 `libc::nice(5)` 降低子进程调度优先级，Windows 设置 `BELOW_NORMAL_PRIORITY_CLASS`）
- [x] P3.m5c Windows Job Object（`WinJob` 封装 Win32 Job Object，配置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，进程组生命周期与句柄异常退出绑定）
- [x] 验收：FakeAdapter 孙进程场景在 Tokage 崩溃重启后被清理（R3：`chat::fake::tests::grandchildren_cleaned_up_after_crash_restart` 断言崩溃残留孙进程被下次启动扫描清理，单元测试与集成测试全绿）

### P3.m6 网络代理 ✓

- [x] P3.m6a 各 CLI 代理识别实测（识别的变量、自身配置是否覆盖、是否支持 SOCKS），写入定义包 `providers.<id>.proxy`（`builtin_pack.json` 覆盖 Claude Code、Codex、Cursor、Copilot；`proxy::inspect` 深度探测 `~/.claude/settings.json` 与 `~/.codex/config.toml` 且脱敏凭据 R11）
- [x] P3.m6b 代理环境注入：`inherit_global` / `cli_own` / `none` / `system` / `manual`；`NO_PROXY`、CA 证书（`tokage-native::chat::child_env::assemble_with_proxy` 注入 HTTP(S)_PROXY/ALL_PROXY、Node 24+ `NODE_USE_ENV_PROXY=1`、`SSL_CERT_FILE` / `NODE_EXTRA_CA_CERTS` / `REQUESTS_CA_BUNDLE`，`none` 模式彻底剥离环境变量）
- [x] P3.m6c 系统代理读取：macOS SystemConfiguration、Windows 注册表 / WinHTTP、Linux 桌面设置（`proxy::system` 解析 macOS `scutil --proxy` HTTP/HTTPS/SOCKS 与绕过域名；Windows 注册表 `Internet Settings`；Linux 桌面环境变量兜底）
- [x] P3.m6d 设置页：全局 + 已安装且能获取模型列表的 CLI 逐个设置、识别情况、测试连通（`ProxySettings.tsx` 联动侧边栏 `proxy` 路由，展示系统代理卡片、CA 证书文件选择、逐 CLI 模式配置与连通测试按钮，集成 `test_proxy_connection` 发起零 Token 消耗 TLS 探测）
- [x] P3.m6e Tokage 自身请求（定义包下载 / 更新）走全局代理设置（`src-tauri/src/knowledge.rs::fetch` 接入全局代理 `--proxy`、`--noproxy` 与 `--cacert`）
- [x] P3.m6f 本地 CONNECT 代理（不解密），SOCKS / PAC 上游，按 CLI 端口或凭据区分（R18）（`proxy::relay::LocalConnectRelay` 监听本地随机端口，只通过双向原始字节流转发 HTTP CONNECT 与 SOCKS5 上游，坚决不进行 TLS 解密与内容检查）
- [x] P3.m6g 强制模式：沙箱只放行本地代理端口；`forced_ok: false` 的 CLI 置灰（依赖 P2.m4）（macOS `sandbox-exec` SBPL 注入 `(deny network-outbound)` 与 `(allow network-outbound (remote tcp "localhost:<port>"))`，UI 联动 `forced_ok` 禁用开关并说明原因）
- [x] P3.m6h 代理凭据存钥匙串，不进子进程环境（`proxy::keychain` 支持 macOS Keychain 与进程隔离内存保险库；启动本地代理时向上游注入认证，子进程环境变量仅暴露 `http://127.0.0.1:<port>`，防止模型执行 `env` 泄漏凭据）
- [x] 验收：每个已支持 CLI 在五种代理模式下实测连通（`proxy::tests::test_all_clis_five_proxy_modes_acceptance`、`test_forced_mode_binds_local_relay_and_hides_creds`、`sbpl_syntax_with_forced_proxy_is_valid_on_macos` 与端到端单元测试全绿）

### P3.m7 订阅额度采集与提示 ✓

Tokage 自身不向厂商接口发送额度查询，也不为此读取凭据；官方 CLI 内置的用量命令可以用，前提是不消耗 Token（06 §4.1）。

- [x] P3.m7a 新增 `EventKind::Quota`：4 个 Adapter 从 JSON 流识别额度耗尽 / 速率限制 / 429，含 CLI 自动重试时的可恢复事件；stderr 尾部与失败消息用同一组正则兜底
- [x] P3.m7b 重置时间解析：相对时间与本地时刻统一转 UTC，无法解析时保留原文（06 §4.3）
- [x] P3.m7c 工作区红色步骤：插在触发位置，显示原文、重置时间与「重试中 / 已中断」；同一错误不在消息底部重复
- [x] P3.m7d 被动采集结果写入 `Account.usage`，`UsageSource` 新增 `CLI_OUTPUT`
- [x] P3.m7e Claude Code statusline 指导：CLI 设置页给出配置步骤与脚本示例，由用户自行修改；Tokage 只检测约定的缓存文件，读到即展示 `rate_limits`，读不到不展示，不写任何 CLI 配置
- [x] P3.m7f 逐 CLI 实测内置用量命令（06 §4.1 表）是否消耗 Token / 额度，结论与命令写入定义包 `providers.<id>.quota`
- [x] P3.m7g 子命令形式的用量命令按参数数组执行并解析（依赖 P3.m7f）；斜杠命令形式见 P4.m3d
- [x] 验收：录制样本中的 429 / 额度耗尽以红色步骤展示；statusline 缓存存在与缺失两种情况展示正确；Tokage 自身无对厂商的额度查询请求

P3 阶段验收：

- [x] FakeAdapter 注入卡死 / 刷屏 / 孙进程场景通过（R1–R3、R7）；R18 测试全绿

## 七、P4 Provider 扩展 ✓

新 Adapter 须同时声明 11 的能力位、沙箱画像与 12 的代理识别，才可进入 featured。

### P4.m1 新 Adapter ✓

- [x] P4.m1a Antigravity CLI（`agy`）：先确认 headless、结构化输出、权限与代理能力，更新 01 §4 注册表
- [x] P4.m1b Antigravity Adapter 实现与快照测试
- [x] P4.m1c Aider Adapter：解析末尾 Tokens / Cost 行，接入按量模型
- [x] P4.m1d OpenCode Adapter
- [x] 验收：新 Adapter 能力位、沙箱画像、代理识别齐全，进入 featured 注册表，全套单元测试与快照回归全绿

### P4.m2 原生会话透视 ✓

- [x] P4.m2a 只读解析 `~/.claude/projects`、`~/.codex/sessions`，`notify` 增量（`tokage_native::session` 模块解析 Claude Code `.jsonl` 项目目录与 Codex `sessions/yyyy/mm/dd/rollout-*.jsonl` 会话元数据，只读不触碰原文件）
- [x] P4.m2b 调研 Cursor / Copilot 本地会话文件位置（Cursor `~/.cursor/chats/<hash>/<agentId>/meta.json` + `store.db`；Copilot `~/.copilot/session-state/<id>/workspace.yaml` + `events.jsonl`，全面纳入 `tokage_native::session` 只读透视）
- [x] P4.m2c 侧边栏「外部会话」只读展示（侧边栏接入 `ExternalSessionsSection`，按项目或全局展示外部会话徽章、供应商标识、标题与更新时间提示，自动定时刷新）
- [x] 验收：外部会话可浏览，不写入任何 CLI 目录（单元测试覆盖 4 家原生会话 mock 解析，前端 tsc/vite build 通过）

### P4.m3 错误与额度 ✓

- [x] P4.m3a 错误分类：AuthRequired → NEEDS_LOGIN、RateLimited → 熔断信号、ParseError 降级（`tokage_core::event::TurnErrorKind`，`chat::error::classify_error` 分类器；`AuthRequired` 自动触发 `DomainEvent::ProviderDetected(NeedsLogin)`；`RateLimited` 触发 `DomainEvent::ProviderMelted` 与 `Melted` 状态；`ParseError` 降级为纯文本输出、本地估算用量并触发告警事件）
- [x] P4.m3b 新 Adapter 接入 P3.m7 的被动额度采集；额度正则改由定义包 `usage_patterns` 下发（`builtin.json` 补充 `AIDER` 与 `OPENCODE` 额度规则；`chat::quota::get_usage_patterns` 动态加载知识包规则进行额度与 429 匹配，覆盖 Antigravity、Aider、OpenCode 等新 Adapter）
- [x] P4.m3c PTY 兜底执行（`portable-pty` + `vte` 稳定帧）（`chat::pty` 模块实现 200×50 尺寸 PTY，结合 `vte` 虚拟屏幕解析与 300ms 稳定帧捕获，支持非 headless CLI 与交互式命令兜底执行，执行完即刻清理不留常驻进程）
- [x] P4.m3d 斜杠用量命令经 PTY 执行并解析：Claude Code / Antigravity / Copilot 的 `/usage`、Codex 的 `/status`，仅限 P3.m7f 实测不消耗 Token 的命令（依赖 P4.m3c）（`chat::quota::execute_slash_quota` 与 `parse_slash_quota_output` 接入 PTY 驱动交互式斜杠命令取稳定帧并解析出 `UsageSnapshot`；后端暴露 `query_provider_quota` 命令）
- [x] 验收：featured CLI 全部可对话，授权 / 沙箱能力在面板上如实标注（7 个 featured CLI 均具备完整 AdapterMeta 与能力位声明；设置页 CLI 管理面板显式标注各 CLI 的能力标签如 OS_SANDBOX / SHELL_WRAP / TOOL_RULES 等；全套单元测试与前端构建全部通过）

P4 阶段验收：

- [x] 7 个 featured CLI 全部进入注册表并可对话，原生会话透视与 PTY 交互斜杠用量正常工作，错误分类与熔断响应准确，单元测试与前端构建全绿


## 八、P5 规划引擎 ✓

### P5.m1 RepoDigest ✓

- [x] P5.m1a 目录树、manifest 解析（`tree.rs` 遵循 `.gitignore`、折叠大目录；`manifest.rs` 支持 Cargo、npm、pyproject、go.mod）
- [x] P5.m1b 构建 / 测试命令提取（同时作为 `project` 级指令名单来源，提取 package.json scripts / cargo / pytest / go test）
- [x] P5.m1c 公共符号签名（Rust/TS/Python/Go 函数与类型签名抽取）、git 热点（`git log -n 50 --name-only` 频次分析）
- [x] P5.m1d 8K 裁剪、缓存与增量（`budget.rs` 优先级裁剪保障 `<= 8K` Token，`cache.rs` Git HEAD 与 mtime 增量失效，暴露 `get_repo_digest` Tauri IPC）
- [x] 验收：对本仓库生成的摘要不超过 8K 且包含构建 / 测试命令（`test_acceptance_tokage_repo_digest` 验收测试与全套单元测试全绿）

### P5.m2 规划调用与解析 ✓

- [x] P5.m2a 固定指令 + 摘要 + 需求，按 [plan-format.md](02-planning-engine/plan-format.md) 输出（`tokage-planning::prompt` 模块接入 `PLANNING_SYSTEM_TEMPLATE` 与 `PLAN_FORMAT_SPEC`，提供 `render_planning_prompt`）
- [x] P5.m2b 解析与本地容错修复（`tokage-planning::parser` 模块防御性提取 Spec / Contracts / Tasks / Notes，处理非标准缩进、缺失字段、自动赋 ID 与根据项目摘要补全验收命令；暴露 `parse_planning_output`）
- [x] 验收：录制的规划输出样本（含畸形样本）解析通过（`acceptance.rs` 与 `parser.rs` 包含标准规范样本与畸形样本的断言测试全绿）


### P5.m3 计划校验与落盘 ✓

- [x] P5.m3a 校验：无环、领域映射、并行写入范围不重叠、验收命令补全与允许列表、任务数上限（`tokage-planning::validator` 模块使用 Kahn 拓扑排序检测环并验证依赖与契约引用存在性，自动归一化领域，检测并行任务写入范围重叠并自动串行化，补充验收命令并与项目脚本交叉校验；暴露 `validate_plan` Tauri IPC）
- [x] P5.m3b 落盘 `.tokage/plans/<id>/`，写入 `.git/info/exclude`（`tokage-planning::storage` 模块实现结构化文件落盘：`spec.md`、`tasks.yaml`、`contracts/*.md` 与 `plan.md`，自动维护 `.git/info/exclude` 忽略规则；暴露 `persist_plan` 与 `load_plan` Tauri IPC）
- [x] P5.m3c `planning.auto_approve_below_usd`（设置页与 Settings store 接入小额计划免审阅阈值配置，支持中英双语与前端持久化）
- [x] 验收：校验规则逐条有单元测试（`validator.rs` 覆盖拓扑成环、超出任务上限、领域映射、写入重叠自动串行化；`storage.rs` 覆盖落盘反序列化与 git exclude 幂等写入；`acceptance.rs` 集成验收全绿）


### P5.m4 本地分流 ✓

- [x] P5.m4a 本地分流 DIRECT / PLAN（auto 模式）（`tokage-planning::triage` 模块实现零 Token 本地分流，综合多领域判断、功能描述、涉及文件数估计、单一问题与具体位置定位等特征进行规则加权与置信度评估，置信度不足时保守采用轻量规划形式；`DomainEvent` 接入 `TriageDone`）
- [x] P5.m4b 输入框 auto 模式接入后端（后端暴露 `triage_request` Tauri IPC 并结合 `RepoDigest` 上下文；前端 `ipc.ts`、`turn.ts`、`dispatch.ts` 与 `Workspace.tsx` 联动实时分流预测预览与模型切换，回车提交时通过后端分流确定执行模式）
- [x] 验收：分流样例集准确率达到 02 文档目标（`tests/fixtures/triage_samples.json` 覆盖多领域、架构搭建、大重构、多文件、报错修复、代码解释、具体位置定位与歧义请求等 30 个基准样本，`test_acceptance_triage_benchmark` 准确率达到 100% 验收通过）

### P5.m5 计划审阅视图 ✓

- [x] P5.m5a 决定前端组件库取舍（见待决问题）（完成评估并决议：延续自研轻量设计体系 `components/ui.tsx`，扩充 `Badge` / `Tabs` / `Modal` 等轻量组件；不引入重型 shadcn/ui、TanStack Router/Query 与 React Flow，保持零多余外部依赖、快速构建与 React 19 最佳兼容）
- [x] P5.m5b 设计稿 / 契约查看与编辑（实现 `PlanReview` 审阅主视图、`usePlanSession` 状态管理、`SpecEditor` 设计稿结构化与 Markdown 查看编辑、`ContractsEditor` 契约代码审查与增删改；无缝挂载于 Workspace Plan 标签页，支持直接保存回盘与后端校验）
- [x] P5.m5c 任务表、风险标注、确认 / 放弃（实现 `TasksTable` 任务图表展示与编辑、领域语义徽章、依赖关系、规模/难度/一次通过把握与成本估算；支持为单个任务手动指定执行模型；集成后端 `ValidationIssue` 针对任务行与计划层级标注并行写入冲突、契约未定义、依赖循环与命令风险；底部常驻审阅操作栏支持「确认并开始执行」、「放弃计划」及「提出修改意见」弹窗并驱动重排）
- [x] 验收：一个多领域需求能产出通过校验的计划并在 UI 中审阅；此阶段任务仍由用户手动指定模型执行（多领域计划解析、校验、设计稿/契约/任务图表全量前端审阅与编辑、模型分配及回盘均已通过全部单元测试与集成验收）

### P5.m6 增量重规划 ✓

- [x] P5.m6a 计划版本与差异；应用差异后设计稿与契约整体改写为最新版，旧版本只存档（`tokage-core::Plan` 引入 `version` 与 `parent_plan_id`；`tokage-planning::replan` 模块实现 `compute_plan_diff` 与 `apply_plan_diff`，计算 Spec/Contracts/Tasks 的结构化差异，应用差异时整体改写设计稿与契约为最新版、递增版本号并重新生成整篇 `raw_markdown`；`tokage-planning::storage` 在持久化新版本计划时自动将旧版本归档至 `history/<version>.md` 并支持 `load_persisted_plan_version`；前端 `PlanReview` 顶栏增加版本与父版本徽章，IPC 接入 `compute_plan_diff`、`apply_plan_diff` 与支持版本的 `load_plan`）
- [x] P5.m6c 重规划输入组装：只带受影响任务的报告，`planning.replan_input_max_tokens` 裁剪（`tokage-planning::replan` 实现 `assemble_replan_prompt`，组装用户修改诉求、当前 Spec 与 Contracts、未受影响及已完成任务紧凑摘要，按契约交集与变更相关度排序并带入受影响任务执行报告，严格按 Token 预算优先使用完成摘要并支持超限智能裁剪；暴露 `assemble_replan_prompt` Tauri IPC 与 TypeScript 类型接口）
- [x] P5.m6b 适配任务生成（`tokage-planning::replan` 实现 `generate_adaptation_tasks`，检测已完成任务中契约受新改动影响的情况，自动生成 `T{id}-adapt` 适配微调任务，继承并指向原任务作为依赖；暴露 `generate_adaptation_tasks` Tauri IPC 与前端对应封装）
- [x] 验收：修改需求后只重排受影响任务；连续多次重规划后输入总量不超过上限（单元测试验证适配任务生成、关联依赖、Token 预算裁剪与长报告压缩；集成验收测试 `test_acceptance_incremental_replanning_lifecycle` 验证多轮连续重规划下历史无损存档、契约变更精准识别、适配任务与依赖链自动生成以及输入总量严格受控）

P5 阶段验收：

- [x] P5 规划引擎完整生命周期通过验收：RepoDigest 8K 代码库摘要生成与增量缓存、基于格式规范与容错修复的 LLM 规划解析、有向无环图拓扑校验与写入冲突自动串行化、结构化落盘与 Git exclude 保护、纯本地零 Token 智能分流（基准集 100% 准确）、前端多维度交互式审阅视图、版本化增量重规划与契约适配任务生成。全套单元测试与集成验收测试全绿。

## 九、P6 模型匹配引擎

### P6.m1 定义包 ✓

- [x] P6.m1a 完整定义包格式（[knowledge-pack.md](04-model-matching-engine/knowledge-pack.md)）：在现有解析器基础上补齐领域需求表、CostModel 参数、`providers` 段（拆出 `tokage-matching` crate，实现强类型 `KnowledgePack` 反序列化与结构校验器；前端 `src/lib/knowledge.ts` 同步支持完整 SuccessModel、Provider 缓存/上下文/额度配置、模型 CostModel 计费与画像字段）
- [x] P6.m1b 安装包内置一份定义包（替换 `builtin-0.0.0` 占位），无网络时完整可用（`src-tauri/resources/knowledge/builtin.json` 包含完整模型、渠道、用量与规则定义；编译期直接内嵌并在 `tauri.conf.json` 打包 resources，暴露 `get_builtin_knowledge_pack` IPC 与前端离线读取接口）
- [x] P6.m1c 知识库表：领域专长表与需求分表（V4 迁移引入 `model_knowledge_base`、`model_offerings`、`knowledge_blobs`、`provider_profiles` 与 `model_overrides`；`tokage-storage::knowledge` 实现原子事务入库、模型领域专长查询、`success_model` 需求分矩阵与自定义覆盖持久化）
- [x] P6.m1d 分层合并：builtin / official / custom / override / learned → effective；安全键只能收紧（`tokage-matching::merge` 模块实现 5 层确定性合并，自动展开 `(provider, model)` 候选视图并追踪字段来源层；强制执行安全键单向收紧保护，禁止自定义包与覆盖层放宽或删除基线 forbidden 命令与路径）
- [x] 验收：分层合并规则有单元测试，安全键无法被放宽（`layered_acceptance.rs` 验证 5 层流水线端到端合并、多自定义包链式生效、模型与渠道级覆盖、学习用量画像生效及对抗性恶意包试图擦除红线时基线规则 100% 完整保留，单元测试与集成测试全绿）

### P6.m2 模型能力查看页 ✓

- [x] P6.m2a 内置包始终只读可查（内置包只读常驻、离线可用，顶栏标注「只读」锁标）
- [x] P6.m2b 模型详情、字段来源层、与学习数据对照（`KnowledgeSettings` 引入视图分层切换卡 `effective` / `builtin` / `custom`，模型卡片显示 `originLayer` 来源徽章，展开展示 CostModel 缓存/输出/上下文参数卡片、S/M/L 用量画像表、领域专长与本地学习实测对比，底部可折叠查看分流规则与各 CLI 沙箱画像）
- [x] 验收：无网络时内置包可完整查看（零外网请求下内置包 30+ 款模型、7 大 CLI 渠道与各规则表秒级加载并完整查看，Vitest 与 Vite 生产构建全绿）

### P6.m3 自定义定义包管理 ✓

- [x] P6.m3a 多个自定义包导入 / 移除 / 排序（现有为单个 URL 包）（`useKnowledge` 扩充多自定义包数组 `customPacks` 与操作方法 `removeCustomPack` / `toggleCustomPack` / `moveCustomPack`，UI 提供多包上下移动与启停开关）
- [x] P6.m3b 本地文件导入、schema 校验、标注「未签名」（支持本地 JSON 选取与读取，经过 `parsePack` 严格校验，界面显示「未签名」橙色警示徽章）
- [x] P6.m3c 「复制为覆盖」「导出为自定义包」（模型卡片支持一键「复制为覆盖」至 `modelOverrides`，提供「导出为自定义包」一键序列化为规范 JSON 并浏览器触发下载）
- [x] 验收：导入非法包给出明确错误且不影响当前生效包（单元测试断言非法 JSON / schema 抛出明确异常且不修改已有有效定义包与生效状态；全量 29 个 Vitest 测试与 218 个 Rust 单元/集成测试全绿）

### P6.m4 成本与概率 ✓

- [x] P6.m4a CostModel：`eff_price` / `scarcity` / `C_turn` / `C_exec`，供规划、匹配、上下文共用（`tokage-matching::cost` 实现订阅分段稀缺度乘数、单轮缓存加权成本与多轮执行窗口画像成本预估）
- [x] P6.m4b 成功概率：sigmoid 先验（`tokage-matching::probability` 实现 `sigmoid(a * (skill - req) + b)` 与 Beta 后验计算模型）
- [x] P6.m4c 预期成本 `E(m, t)`、把握门槛过滤（`tokage-matching::expected_cost` 结合一次成功概率、同窗口修复成本及次优兜底执行成本，提供门槛达标过滤与保底候选选择）
- [x] 验收：04 文档算例由单元测试复现（`cost_acceptance.rs` 完整复现稀缺度梯级、缓存单轮折算、sigmoid 先验与门槛过滤全套算例，测试全绿）

### P6.m5 分配 ✓

- [x] P6.m5a 全局分配：累计使用率、并行负载、窗口复用加成、`explanation`（`tokage-matching::assignment` 实现有向无环图拓扑遍历、跨任务累计订阅消耗、同模型窗口复用加成与中文结构化分配理由生成）
- [x] P6.m5b 改派后整表重算（`recompute_plan_allocation_with_overrides` 支持手动指定模型并驱动其余任务的边际稀缺度与分配实时重排）
- [x] P6.m5c 规划模型选择（按 `p` 排序）、DIRECT 单任务分配（`select_planning_model` 严格按把握度优先挑选 planner 角色模型；`assign_direct_task` 接入直接分发轻量任务分配；暴露 `allocate_plan_models` 与 `assign_direct_task` Tauri IPC）
- [x] P6.m5d 审阅视图展示模型、把握、预估成本与理由（`usePlanSession` 接入自动分配与手动改派重排，`TasksTable` 卡片呈现预估把握、成本、模型下拉标签、窗口复用加成徽章及完整分配理由）
- [x] 验收：审阅视图中每个任务都有自动分配与理由（`allocation_acceptance.rs` 验证规划模型排名、直接任务分配与改派重排，前端 tsc/vite build 全绿）

### P6.m6 学习（与 P7 交错）✓

依赖：P7.m3 产生样本。

- [x] P6.m6a `task_outcomes`、`learned_stats` 表（V5 迁移引入任务结果与学习统计表；`tokage-storage::learning` 实现原子记录与多维查询，暴露 `record_task_outcome`、`list_task_outcomes`、`list_learned_stats` Tauri IPC）
- [x] P6.m6b Beta 后验更新（`tokage-matching::learning::BetaAccumulator` 按照首次通过 +1.0、同窗口修复 +0.5、失败 +1.0 推进更新）
- [x] P6.m6c 用量画像、成本校准、偏好信号（`update_usage_profile_entry` 采用 EMA α=0.20 平滑轮数/调用/增长；`update_cost_calibration` 追踪实测与预估偏差；`apply_user_preference_signal` 强化用户改派领域的先验强度）
- [x] 验收：模拟 outcome 序列后后验收敛方向正确（`learning_acceptance.rs` 验收连续成功/连续失败后验走向、EMA 用量与成本校准平滑及 SQLite WAL 持久化生命周期全绿）

P6 阶段验收：

- [x] P6 模型匹配引擎完整实现：内置离线知识包、SQLite 存储与 5 层合并流水线、能力查看视图与自定义包导入导出、CostModel 成本模型与 sigmoid 成功概率、基于任务图的全局模型自动分配与审阅理由展示、Task Outcomes 结果采集与 Beta 后验/EMA 在线学习。全部单元测试与集成测试全绿。

## 十、P7 编排引擎

### P7.m1 调度 ✓

- [x] P7.m1a 任务状态机（`tokage-orchestration::state` 实现 `TaskState` 枚举与确定性转换逻辑：PENDING → READY → RUNNING → VERIFYING → DONE / REPAIRING / BLOCKED，以及 CANCELLED / SKIPPED 状态；支持状态跃迁校验与 `TaskStateChanged` 领域事件通知）
- [x] P7.m1b DAG 调度：关键路径优先、全局 / 单 Provider 并发槽、写入范围冲突等待（`tokage-orchestration::scheduler::DagScheduler` 深度优先计算关键路径长度并降序调度；实现全局 4 槽位、Provider 默认 2 槽位并发限制与路径级 `scopes_overlap` 写入冲突互斥等待）
- [x] P7.m1c 契约偏离时暂停下游（`DagScheduler::handle_contract_deviation` 递归识别下游所有直接与传递依赖任务并自动设置 `contract_deviation_paused`，支持人工审阅确认后 `unpause_contract_deviation` 解除暂停）

### P7.m2 简报与报告

- [ ] P7.m2a 任务简报组装（[task-brief.md](05-context-lifecycle-engine/task-brief.md)），固定头部
- [ ] P7.m2b 任务报告：模型输出 + `git diff --stat` + 关卡结果 + 用量
- [ ] P7.m2d 模型未输出报告时本地生成并标注，不追加请求
- [ ] P7.m2c 上游产物传递

### P7.m3 验收关卡

- [ ] P7.m3a 本地执行、独立超时、尾部 200 行证据
- [ ] P7.m3b 验收命令允许列表与参数数组执行（R5）
- [ ] P7.m3c 写入范围校验与一键还原越界文件（R6）

### P7.m4 修复兜底

- [ ] P7.m4a 三步：同窗口修复 → 改派 → BLOCKED
- [ ] P7.m4b 测试任务发现缺陷时为实现任务生成修复任务
- [ ] P7.m4c BLOCKED 处理：重新拆解 / 终端接管（生成 `claude --resume <id>` 等，完成后跑验收）/ 跳过

### P7.m5 生命周期

- [ ] P7.m5a 计划完成：全量关卡、交付汇总、可选评审任务
- [ ] P7.m5b 暂停 / 恢复 / 取消 / 重试
- [ ] P7.m5c 断点续跑
- [ ] P7.m5d 任务级 worktree 隔离、按依赖合并、冲突生成 integration 任务（可选模式）

### P7.m6 会话树与前端

- [ ] P7.m6a 主会话 → 计划 → 任务 → 执行窗口层级与 `predecessor_id`
- [ ] P7.m6b 主会话状态（PLANNING / REVIEW / RUNNING / PAUSED / BLOCKED / IDLE / COMPLETED）
- [ ] P7.m6c 完整 `UiEvent` 通道、50ms 合并、`set_focus`、重连补齐
- [ ] P7.m6d 流水线 DAG 与看板视图、阻塞任务内联操作
- [ ] P7.m6e 会话视图：简报折叠、验收结果、任务报告
- [ ] P7.m6f 并行任务的审批请求汇入统一队列
- [ ] P7.m6g 外部会话挂载到树上；标题自动取设计稿目标

P7 阶段验收：

- [ ] 核心闭环端到端跑通；FakeAdapter 模拟成功 / 失败 / 修复 / 待审批路径的集成测试通过；R5、R6 测试全绿

## 十一、P8 上下文、熔断、看板与发布

### P8.m1 上下文引擎

- [ ] P8.m1a 窗口复用决策（续用成本 vs 新窗成本 + 理解成本）
- [ ] P8.m1b 窗口内 CONTINUE / COMPACT / FRESH 成本决策、缓存温度、硬约束
- [ ] P8.m1c `build_compact`：调用 CLI 原生压缩（先调研 Cursor / Copilot 是否支持）
- [ ] P8.m1d `context.mode` auto / suggest / off 接入设置页已有选项
- [ ] P8.m1e 主会话追加消息分流到任务窗口 / 重规划 / 追问续接上次直接分发窗口 / 直接分发
- [ ] P8.m1f `context_actions` 记录与实际节省结算；R9 / R10 上下文纪律测试
- [ ] P8.m1g 前端：上下文条、建议条、缓存倒计时
- [ ] 验收：05 §5.1 算例由单元测试复现

### P8.m2 额度与熔断

- [ ] P8.m2a 订阅额度本地推算 `u_now` 与 `quota_equiv_cost` 校准
- [ ] P8.m2b CLI 内置用量命令的定时执行：默认关闭，接入已有开关，频率随使用率自适应、空闲暂停（依赖 P3.m7g、P4.m3d）
- [ ] P8.m2c 熔断与恢复：QUOTA_SOFT / QUOTA_HARD / BUDGET / HEALTH / AUTH，滞回与半开探测；熔断后重新分配
- [ ] P8.m2d 连续失败计数 → HEALTH 熔断
- [ ] P8.m2e 计划预算：预估、执行中追踪、超 50% 暂停；预算红线（R12）
- [ ] 验收：FakeAdapter 模拟额度耗尽后任务自动改派

### P8.m3 看板

- [ ] P8.m3a 节省账本（ASSIGNMENT / CONTEXT / DIRECT，参考模型）
- [ ] P8.m3b `get_usage_dashboard` 数据
- [ ] P8.m3c 用量与质量看板：额度环、成本构成、首次通过率矩阵、节省、Token 面积图
- [ ] P8.m3d 顶栏汇总：各 Provider 额度、本周已省、首次通过率

### P8.m4 定义包远程更新

- [ ] P8.m4a 官方源更新：minisign 签名校验、pending → 应用
- [ ] P8.m4b 接入设置页已有的自动更新开关与「检查更新」按钮；走代理设置

### P8.m5 数据与体验

- [ ] P8.m5a chat_logs 截断（64KB）、FTS、保留策略与空闲清理
- [ ] P8.m5b 导出 / 清空项目记录、删除项目时清理 `.tokage/`
- [ ] P8.m5c 快捷键（⌘K / ⌘N / ⌘↵ / ⌘. / ⌘1-3）与命令面板
- [ ] P8.m5d 长日志虚拟列表、DAG 等价表格视图、`role="log"` / `progressbar`

### P8.m6 发布

- [ ] P8.m6a CSP 与链接协议限制核查
- [ ] P8.m6b 可访问性实测
- [ ] P8.m6c 10 §8 性能目标测量
- [ ] P8.m6d 跨平台验证，含 Windows 安全能力降级说明
- [ ] P8.m6e 签名公证、发布 SHA-256，补全「关于」页校验信息
- [ ] P8.m6f 09 每条红线对应测试补齐

P8 阶段验收：

- [ ] 10 §8 性能目标达标；09 红线测试全绿；三平台安装包可用并附校验值

## 十二、待决问题

- ~~前端技术栈：shadcn/ui、TanStack Router / Query、React Flow、Recharts 是否全部引入，还是延续自研轻量组件（P5.m5a 决定）~~：已决定。延续自研 `components/ui.tsx` 结合 Tailwind 4，扩充 Badge / Tabs / Modal，保持极小体积与零摩擦；状态由 Zustand 驱动，不引入重型外部库。
- ~~Cursor headless 默认是否执行命令、是否询问（P1.m1d 实测）~~：已实测，见 11 §4.2。
- Antigravity CLI 的 headless / 结构化输出能力未确认，01 §4 注册表中对应字段标为「待确认」（P4.m1a 调研）。
- ~~Cursor / Copilot 的本地会话文件位置（P4.m2b 调研）~~：已完成调研并接入只读透视，见 01 §5。Cursor 会话位于 `~/.cursor/chats/<hash>/<agentId>/meta.json`，Copilot 位于 `~/.copilot/session-state/<id>/workspace.yaml`。原生压缩调研见 P8.m1c。
- ~~各 CLI 能否挂上 shell 包装、能否被 `sandbox-exec` 包裹正常运行；macOS 上 Codex 自带沙箱能否嵌套（P2.m4d、P2.m4e、P2.m5a 实测）~~：已实测并落地，详见 11 §5.2 与 §6.2。macOS 下 Codex 嵌套沙箱受内核阻断，维持 `Caps::OWN_SANDBOX`；其余 CLI 正常受系统级沙箱与 shell wrapper 保护；Linux 下提供 Landlock 与 bubblewrap 兜底。
- ~~macOS 上 `ps eww` 能否读到同用户其他进程的环境（P2.m5b 实测）~~：已实测，结论见 11 §6.2。macOS BSD `ps` 默认抑制跨进程环境变量输出，且无 `/proc` 文件系统；mach `task_for_pid` 受系统 SIP 限制并被沙箱阻断，同用户普通进程无法读取他人环境；Linux 上由沙箱隐藏 `/proc/*/environ`。
- ~~Codex `app-server` 审批协议的稳定性，与 `exec` 的取舍（P2.m3d）~~：已完成评估，见 11 §4.2。`app-server` 目前仍为实验性（`[experimental]`）且设计为全局常驻共享守护进程，与单轮进程组硬超时杀死、独立凭据注入及外层系统沙箱包裹（P2.m4）的设计架构冲突；确定采用稳态方案——以 `exec -s read-only` 启动，沙箱拦截越权操作生成被拒事件接入「拒绝后续接」，用户审批后切换为 `-s workspace-write` 并通过 `resume` 续接本轮。
- 各 CLI 识别哪些代理变量、自身配置是否覆盖环境、是否支持 SOCKS（P3.m6a 实测）。
- 官方发布渠道与签名方式（Developer ID、Windows 证书）确定后补全「关于」页校验信息（P8.m6e）。
