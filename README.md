# Tokage (托卡奇)

多联邦 Agent 本地指挥部与上下文生命周期网关。

Tokage 是一个面向独立开发者的跨平台桌面网关（Tauri 2 + Rust + React 19）。它调度本机已安装的官方原版 AI CLI（Claude Code、Codex CLI、Cursor CLI、Antigravity CLI、OpenCode 等），把一个需求变成一条由多个模型协作完成的流水线。

> 文档版本：v1.1（2026-10-04）。项目已进入开发阶段：CLI 探测、多 CLI 对话、Diff 与会话 worktree 已可用；规划 / 匹配 / 编排闭环尚未实现。各模块进度与后续阶段见 [docs/TODO.md](docs/TODO.md)。

> [!IMPORTANT]
> **数据与凭据读取边界（R11）**
>
> Tokage 会在本机读取 CLI 配置中的以下非密钥字段，仅用于界面展示与计费类型识别：
>
> | 读取 | 用途 | 处理 |
> |------|------|------|
> | Base URL | CLI 面板展示 | 去掉 userinfo、query、fragment；路径中疑似 token 的段打码 |
> | 认证方式、套餐、账号类型 | 计费类型识别 | 只取声明字段 |
> | Key 变量名、凭据文件是否存在 | 登录态判断 | 不取值 |
>
> Tokage **不读取** API Key / Token 的值（解析时读到即丢弃），**不保存、不写日志、不传给界面、不上传**。除你主动配置的模型定义包下载外，Tokage 不发起任何外网请求（官方定义包更新通道暂未启用，见 [docs/04 §10](docs/04-model-matching-engine/README.md) / F8）。实现见 [src-tauri/crates/tokage-native/src/usage.rs](src-tauri/crates/tokage-native/src/usage.rs)，设计见 [11 §7](docs/11-command-authorization/README.md)。
>
> **以上承诺只对官方发布或你自行从 [github.com/iwpz/tokage](https://github.com/iwpz/tokage) 编译的版本成立。请勿使用来源不明的安装包。** 被篡改的副本可以删掉这段说明，应用无法自证未被修改；请以官方签名与发布校验值核对来源。

## 核心竞争力：一开始就选对模型

Tokage 不靠「先用便宜的，失败再升级」来省钱，因为便宜模型做砸本身就是浪费。它在动手之前就完成拆解与分发：

```
用户需求
   │
   ▼
① 规划：强力模型（如 Opus / Fable）只读项目摘要，输出精简的设计稿
   │     ─ 接口契约（API、数据结构、组件 Props）
   │     ─ 任务图：N 个子任务、依赖关系、领域、验收标准
   ▼
② 匹配：为每个子任务挑选「一次做对概率达标、预期总成本最低」的模型
   │     前端任务 → 擅长 UI 的模型；后端任务 → 擅长服务端设计的模型
   ▼
③ 编排：按依赖排队执行，互不依赖的任务并行
   │     A 完成 → 产出报告 → 作为 B 的输入；所有人都受同一份契约约束
   ▼
④ 验证：本地执行构建 / 类型检查 / 测试（零 Token）
   │     重复、机械的测试工作交给廉价模型
   ▼
交付：变更汇总 + 每个模型做了什么、花了多少
```

强力模型只做它最值钱的部分（思考与约束），输出被刻意压缩到几千 Token；大量的编码与测试 Token 由性价比更高、且足以胜任的模型承担。

## Token 节省的杠杆

| # | 杠杆 | 模块 |
|---|------|------|
| 1 | 规划与执行分离：贵模型只产出精简设计稿，不写实现代码 | [02](docs/02-planning-engine/README.md) |
| 2 | 一次做对的匹配：按领域专长与成功概率分发，避免失败返工 | [04](docs/04-model-matching-engine/README.md) |
| 3 | 契约约束：并行任务按同一契约开发，减少集成返工 | [02](docs/02-planning-engine/README.md) / [03](docs/03-orchestration-engine/README.md) |
| 4 | 任务级窗口：每个子任务只带精简任务简报，不背负整段对话历史 | [05](docs/05-context-lifecycle-engine/README.md) |
| 5 | 缓存感知的上下文规划：按各家缓存特性决定续用、压缩或新窗口 | [05](docs/05-context-lifecycle-engine/README.md) |
| 6 | 本地零 Token 机制：项目摘要、分流判断、验收执行、用量采集都在本地完成 | [01](docs/01-native-environment-engine/README.md) / [03](docs/03-orchestration-engine/README.md) |
| 7 | 持续学习：记录每个模型在各领域的首次通过率，下一次分发更准 | [04](docs/04-model-matching-engine/README.md) |

## 核心原则

- 原版优先：只调用官方 CLI 的原生命令，不向用户 Shell 注入 alias，不修改 rc 文件和 CLI 配置。
- 结构化优先：优先使用 CLI 的 headless 结构化输出，PTY 屏幕抓取仅作兜底。
- 本地优先：数据存于本机 SQLite；官方定义包更新通道暂未启用，除你主动配置的自定义定义包下载外不产生外网请求。
- 默认受限：模型发起的命令按授权模式放行或询问，执行窗口在系统级沙箱内运行，读不到凭据。
- 认准来源：以上承诺只对官方发布或自行从 [github.com/iwpz/tokage](https://github.com/iwpz/tokage) 编译的版本成立。
- 可解释、可干预：规划、分发、上下文动作都附带预估成本与理由；执行前可审阅、修改计划。

## 设计文档目录

| # | 模块 | 职责摘要 |
|---|------|----------|
| 00 | [总体架构](docs/00-architecture-overview/README.md) | 技术栈、进程模型、核心闭环、crate 划分、IPC 契约 |
| 01 | [原生环境与联邦引擎](docs/01-native-environment-engine/README.md) | CLI 探测、Provider Adapter、headless / PTY 执行、角色权限 |
| 02 | [规划引擎](docs/02-planning-engine/README.md) | 需求分流、项目摘要、强力模型规划、设计稿与契约、任务图 |
| 03 | [编排引擎](docs/03-orchestration-engine/README.md) | 任务队列与 DAG 调度、产物传递、验收关卡、修复兜底 |
| 04 | [模型匹配引擎](docs/04-model-matching-engine/README.md) | 模型知识库、领域专长、成功概率、预期成本、全局分配 |
| 05 | [上下文生命周期引擎](docs/05-context-lifecycle-engine/README.md) | 任务简报、窗口复用、缓存感知的继续 / 压缩 / 新窗决策 |
| 06 | [精准用量与熔断引擎](docs/06-usage-melting-engine/README.md) | 订阅额度、按量记账、预算、节省账本、熔断与恢复 |
| 07 | [聚合会话树引擎](docs/07-session-aggregation-engine/README.md) | 主会话 → 计划 → 任务 → 执行窗口的树、事件推送、人工介入 |
| 08 | [本地数据仓](docs/08-storage/README.md) | SQLite 表结构、索引、迁移、保留策略 |
| 09 | [边缘约束与防爆红线](docs/09-guardrails/README.md) | 输出过滤、超时、进程组回收、写入范围、命令安全 |
| 10 | [前端 Mission Control](docs/10-frontend/README.md) | 计划审阅、任务流水线看板、会话视图、用量与节省 |
| 11 | [指令授权与凭据隔离](docs/11-command-authorization/README.md) | 授权模式、指令名单、系统级沙箱、凭据隔离、来源提醒 |
| 12 | [网络代理](docs/12-network-proxy/README.md) | 按 CLI 的代理模式、本地 CONNECT 代理、强制出网 |

每个模块目录中的 `README.md` 是该子系统的设计主文档；更细的格式定义、模板与资源放在同目录的其他文件中。

建议阅读顺序：00 → 02 → 04 → 03 → 05 → 01 → 06，其余按需查阅。

开发进度与阶段规划：[docs/TODO.md](docs/TODO.md)。专项修复清单：[docs/fix-todo.md](docs/fix-todo.md)。

## 开发与测试

本地（需联网安装依赖；任何 `npm ci` / `cargo fetch` 先征得使用者同意）：

```bash
npm ci
npm run typecheck
npm test

cd src-tauri && cargo test --workspace --locked
```

依赖真实本机 CLI 的用例标了 `#[ignore]`（`chat::live::*`、`providers::live::*`），默认不跑；手动验收时用：

```bash
cd src-tauri && cargo test --lib chat::live -- --ignored --nocapture
```

CI：推送到 `master` / `main` 或开 PR 时，[`.github/workflows/ci.yml`](.github/workflows/ci.yml) 会跑前端 typecheck + vitest、Rust 工作区（macOS / Ubuntu，根 crate `tokage` 仅 macOS），以及 IPC 接线检查。

IPC 接线检查（口径见 [docs/fix-todo.md](docs/fix-todo.md) 附录 F）：列出生产 TS 中无调用方的 `#[tauri::command]`；未写入豁免表的命令会使退出码为 1。

```bash
python3 -I scripts/check-ipc-wiring.py
# 豁免表：scripts/ipc-wiring-allowlist.txt（每行：命令名  理由）
```

## 术语

| 术语 | 含义 |
|------|------|
| Provider | 一个本地官方 CLI 及其背后的厂商账号（如 Claude Code 订阅） |
| 主会话 (Main Session) | 用户在侧边栏看到的一条需求线 |
| 计划 (Plan) | 规划模型产出的设计稿 + 契约 + 任务图，可有多个版本 |
| 契约 (Contract) | 设计稿中约束各任务的接口定义：API、数据结构、组件 Props、目录约定 |
| 任务 (Task) | 计划中的一个子任务，有领域、依赖、写入范围与验收标准 |
| 执行窗口 (Attempt) | 某个任务在某个 CLI 上的一次实际执行，对应一个 CLI 会话 |
| 任务简报 (Task Brief) | 执行窗口的首条输入：任务卡 + 相关契约 + 上游报告 |
| 任务报告 (Task Report) | 任务完成时输出的精简结果，传给下游任务 |
| 验收关卡 (Gate) | 任务完成后由 Tokage 在本地执行的构建 / 类型检查 / 测试命令 |
| 首次通过率 | 任务在第一个执行窗口内就通过验收关卡的比例，衡量分发准确度 |
| 等效成本 | 统一折算的成本：按量制为真实美元；订阅制为等效 API 价格 × 额度稀缺系数 |
| 熔断 (Melt) | Provider 用量或健康度越过红线后被分发暂时摘除 |
