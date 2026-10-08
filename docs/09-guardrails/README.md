# 09 · 边缘约束与防爆红线 (Guardrails)

返回 [README](../../README.md)

本章约束在 Rust 核心层强制执行。设置只能调整参数，不能关闭红线本身。

## 1. 输出静默与过滤

### 1.1 目标

进入日志、前端、任务报告与 Token 估算的只能是有信息量的文本。进度条、加载动画、`\r` 覆盖行一律不落库。

### 1.2 从源头减少噪声

- 优先使用 CLI 的结构化输出模式，结构化事件本身不含进度条。
- headless 子进程设置 `NO_COLOR=1`、`TERM=dumb`；PTY 模式保留真实 `TERM`。
- `CI=1` 可让多数工具关闭交互式进度条，但可能改变 CLI 行为，由 Adapter 声明是否设置。

### 1.3 OutputFilter 流水线

```
bytes
 → UTF-8 解码（非法字节替换为 U+FFFD）
 → CR 折叠：按 \n 分行；行内含 \r 时只保留最后一个 \r 之后的内容
 → ANSI 剥离：vte 解析，丢弃 CSI / OSC 序列
 → 噪声规则：spinner 行、进度条模式（如 ^\s*\[?[=#>\-\s]*\]?\s*\d{1,3}%）丢弃；连续重复行折叠为「(×N)」
 → 长度限制：单条 > 64KB 时保留头 16KB + 尾 16KB
 → FilteredChunk
```

噪声规则随定义包下发，Adapter 可追加专属规则。验收命令的输出走同一条流水线。

## 2. 超时

### 2.1 双层超时

Agent 型 CLI 的一轮可能包含数十次工具调用，总时长长不代表卡死；卡死表现为长时间没有任何输出。因此同时设置空闲超时与硬超时：

| 档位 | 空闲超时 | 硬超时 | 典型用途 |
|------|---------|-------|---------|
| `QUICK` | 15 s | 30 s | 用量查询、压缩 |
| `STANDARD` | 90 s | 180 s | S / M 规模的开发与测试任务 |
| `LONG` | 180 s | 420 s | L 规模任务、全库审计 |
| `PLANNING` | 180 s | 420 s | 规划 |
| `GATE` | 120 s | 300 s | 单条验收命令 |

- 执行任务的档位由规模决定；用户可整体放大（1× / 2× / 4×）。
- 硬超时针对单轮；一个任务的多轮各自计时。

### 2.2 实现

```rust
async fn supervise(mut child: Child, group: ProcGroup, limits: Limits, mut lines: LineStream) -> RunOutcome {
    let hard = tokio::time::sleep(limits.hard);
    tokio::pin!(hard);
    loop {
        tokio::select! {
            _ = &mut hard => { group.kill().await; return RunOutcome::Timeout(TimeoutKind::Hard); }
            r = tokio::time::timeout(limits.idle, lines.next()) => match r {
                Err(_)         => { group.kill().await; return RunOutcome::Timeout(TimeoutKind::Idle); }
                Ok(Some(line)) => forward(line),
                Ok(None)       => break,
            },
        }
    }
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => RunOutcome::Exited(status),
        _              => { group.kill().await; RunOutcome::Timeout(TimeoutKind::Exit) }
    }
}
```

### 2.3 进程组回收

npm 安装的 CLI 通常是 `node` 包装进程，还会派生 shell、编译器等孙进程，必须整组回收：

- Unix：spawn 时在 `pre_exec` 中 `setsid()`；回收时 `killpg(pgid, SIGTERM)`，3 秒后仍存活则 `SIGKILL`。
- Windows：Job Object + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，回收时 `TerminateJobObject`。
- Tokage 异常退出：Unix 侧把运行中的 pgid 记录到 `run/children.json`，下次启动清理残留；Windows 依赖 Job 句柄关闭自动回收。
- **取消（用户点「停止」/ `cancel_chat_turn`）**：走协作式回收——`TurnRegistry` 通过 `watch` 通知 `run_cli`，由其 `group.kill()` + `child.kill()` 结束进程组；`cancel` 等待轮次任务结束（确认进程组退出）后才返回，超过 8 秒则 `abort` 兜底。用量监听是独立总线订阅，不占用可取消的那条任务。前端在 `cancelChatTurn` 成功返回后再标 `cancelled`，`sendQueuedNow` 也只在取消完成后再派发新消息。

### 2.4 超时后的处置

1. 回收进程组，执行窗口置 `TIMEOUT`，写入 `SYSTEM` 日志。
2. `provider_health.consecutive_fails += 1`；达到 3 次触发 `HEALTH` 熔断（[06 §6](../06-usage-melting-engine/README.md)）。
3. 任务交由编排引擎的修复兜底（[03 §8](../03-orchestration-engine/README.md)）：超时视为失败，证据为超时类型与最后输出。
4. 被终止的 CLI 可能已改动部分文件；改动清单写入失败证据，后续执行者基于现状继续。

## 3. 并发与资源

| 约束 | 默认 | 说明 |
|------|------|------|
| 全局最大并发子进程 | 4 | 不抢占本地编译资源 |
| 单 Provider 最大并发 | 定义包 `concurrency_hint`，默认 2 | 避免触发速率限制 |
| 并行任务写入范围 | 不得重叠 | 见 §4 |
| 子进程优先级 | `nice +5` | 低于用户前台任务 |
| 验收命令并发 | 1 | 构建与测试通常独占资源 |

## 4. 写入范围

- 执行窗口创建时，若 CLI 支持路径级写入限制（能力位 `WRITE_SCOPE`），按任务 `write_scope` 设置。
- 无论 CLI 是否支持，任务结束后都做范围校验：对比任务开始前后的工作区（`git status --porcelain` + 开始时的快照），越界文件视为关卡失败。
- 越界改动不自动回滚：在失败证据中列出，由修复步骤或用户处理；用户可一键还原越界文件（`git checkout -- <files>`，执行前确认）。
- 规划窗口与评审窗口为只读；若检测到任何改动，立即终止并告警。

## 5. 命令安全

- 子进程命令只由 Adapter 构造；用户输入与任务简报作为独立参数或 stdin 传入，不经 `sh -c`。
- 验收命令来自规划模型输出，执行前须满足：
  - 匹配定义包的 `gate_allowlist` 模式（如 `npm|pnpm|yarn (run )?(test|build|lint|typecheck)`、`cargo (test|build|check|clippy)`、`pytest`、`go test`、`make <target>`），或用户在审阅计划时逐条确认过。
  - 不含管道、重定向、命令替换、`;` / `&&` 拼接；按参数数组执行。
  - 工作目录限定在项目根或任务 worktree 内。
- Tokage 不替用户开启 CLI 的「跳过全部权限确认」类参数；`full_auto` 模式以「宽允许名单 + 红线拒绝 + 系统沙箱」实现（[11 §2](../11-command-authorization/README.md)）。
- 模型在 CLI 内发起的命令按 [11](../11-command-authorization/README.md) 的授权模式与指令名单处理；本节只约束 Tokage 自己执行的命令。

## 6. 预算红线

| 红线 | 默认 | 动作 |
|------|------|------|
| 计划实际成本超出预估 | 50% | 暂停队列，提示 |
| 修复成本占计划预估 | 30% | 暂停队列，提示 |
| 按量 Provider 当日花费 | 用户设置 | 熔断（`BUDGET`） |

## 7. 红线清单

| # | 红线 | 执行位置 |
|---|------|---------|
| R1 | 含 `\r` 覆盖、ANSI、spinner、进度条的原始文本不落库 | OutputFilter |
| R2 | 每个子进程都在 supervise 下运行，带空闲与硬超时 | NativeEngine |
| R3 | 超时 / 取消回收整个进程组 | NativeEngine |
| R4 | 不使用 shell 字符串拼接构造命令 | ProviderAdapter / Gate |
| R5 | 验收命令须在允许列表内或经用户确认 | OrchestrationEngine |
| R6 | 并行任务写入范围不重叠；任务结束必做范围校验 | OrchestrationEngine |
| R7 | 规划与评审窗口只读 | NativeEngine |
| R8 | 不替用户开启 CLI 的跳过权限确认参数；全自动模式也只能经由名单与沙箱实现 | ProviderAdapter |
| R9 | 同一窗口内不切换模型、不改变权限与启动参数 | ContextEngine |
| R10 | 上下文动作只在轮次边界发生；不为保温发送额外请求 | ContextEngine |
| R11 | Tokage 只读取 CLI 配置的非密钥字段（Base URL、认证方式、套餐）；Key / Token 的值读到即丢弃，不持久化、不记日志、不传前端、不外发；界面提示用户核对安装包来源（[11 §7](../11-command-authorization/README.md)） | NativeEngine / 前端 |
| R12 | 超出预算红线时暂停队列 | OrchestrationEngine |
| R13 | 模型发起的命令按授权模式处理；`forbidden` 级命令在任何模式下拒绝 | AuthEngine（[11](../11-command-authorization/README.md)） |
| R14 | 执行角色的 CLI 进程树在系统级沙箱内运行（Windows v1 例外，且不开放全自动） | NativeEngine |
| R15 | 模型不可读取凭据路径与其他 CLI 的配置目录 | 沙箱 / shell 包装 / 工具规则 |
| R16 | 子进程环境按厂商裁剪；模型执行的 shell 中不出现凭据变量 | NativeEngine |
| R17 | 日志与界面中的密钥值脱敏；Key 写入项目文件时告警 | OutputFilter |
| R18 | 本地代理只做 CONNECT 转发，不解密、不记录内容；上游代理凭据不进入子进程环境 | ProxyRelay（[12](../12-network-proxy/README.md)） |

每条红线对应单元测试或集成测试，使用 `FakeAdapter` 注入卡死、刷屏、派生孙进程、越界写入、只读窗口写文件、读取凭据路径、打印环境变量、执行 `forbidden` 命令等场景。

> 现状（2026-10-06）：全量红线 R1～R18 已全部完成实现并通过测试验证（包括 R1-R3 进程与输出生命周期、R4-R6 命令与写入范围隔离、R7 只读窗口改动阻断、R8 无自动跳过、R9-R10 上下文纪律、R11 凭据边界、R12 预算硬红线、R13 禁止命令无条件拦截、R14-R15 系统级沙箱、R16 环境变量裁剪、R17 密钥脱敏与告警、R18 代理无解密转发）。测试套件覆盖各红线单元与端到端场景。详见各模块测试与 [TODO](../TODO.md)。
