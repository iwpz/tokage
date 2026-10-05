# 10 · 前端 Mission Control

返回 [README](../../README.md)

## 1. 技术选型

| 项 | 选型 |
|----|------|
| 框架 | React 19 + TypeScript（strict） |
| 构建 | Vite |
| 样式 | Tailwind CSS 4，默认暗色，支持亮色 |
| 组件 | 自研轻量体系（`components/ui.tsx`，Tailwind 4 驱动，P5.m5a 决议不引入外部重量级组件库） |
| 路由 | Zustand 驱动的桌面窗口状态（不引入 TanStack Router） |
| 服务端状态 | 手写 Tauri IPC（`src/lib/ipc.ts`）+ `DomainEvent` 推送（不引入 TanStack Query） |
| 客户端状态 | Zustand |
| 任务表与 DAG | 高密度多栏任务表与轻量 SVG 状态流（暂不引入 React Flow） |
| Markdown | `react-markdown` + `rehype-sanitize` |
| IPC 类型 | 手写于 `src/lib/ipc.ts`，与 Rust 定义逐一对应（不采用 `specta` 的理由见 [00 §7](../00-architecture-overview/README.md)） |

> 现状（2026-10-05）：P5.m5a 确定延续自研轻量设计体系（`components/ui.tsx`，含 Button, Switch, Select, NumberInput, Card, Badge, Tabs, Modal），避免 React 19 兼容性摩擦与不必要的运行时体积；视图与数据流由 Zustand 和 IPC 紧凑驱动，构建速度极快且零多余外部依赖。

## 2. 布局

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ 顶栏: [Claude ● 42%] [Codex ● 18%] [Cursor ●] [Aider ● $0.31]   本周已省 $12.40  首次通过 91% │
├──────────────┬────────────────────────────────────────────────┬───────────────┤
│ 侧边栏        │ 主区                                            │ 右栏（可收起）   │
│              │ ┌ 用户资料功能 · 运行中 · 预估 $0.92 / 实际 $0.61 ┐ │               │
│ 项目 A        │ │ [计划] [流水线] [会话]                          │ │ 选中任务详情     │
│  ├ 用户资料功能 │ │                                                │ │  T2 frontend-ui │
│  └ 修复登录    │ │   T1 backend-api ✓ ─┐                          │ │  Cursor Comp.   │
│ 项目 B        │ │   Codex  $0.18      ├─► T3 integration ● ─► T4 │ │  把握 0.88       │
│              │ │   T2 frontend-ui ✓ ─┘   Codex(续用)     testing│ │  预估 $0.21      │
│ 外部会话       │ │   Cursor $0.20                         ○ DS   │ │  实际 $0.20      │
│  (CLI 原生)   │ │                                                │ │  验收 ✓ typecheck│
│              │ └────────────────────────────────────────────────┘ │  [报告] [日志]    │
│ + 添加项目     │ 输入框  [自动 ▾ 规划/直接]  [目标: 主会话 ▾]   发送      │               │
└──────────────┴────────────────────────────────────────────────┴───────────────┘
```

## 3. 视图

### 3.1 计划审阅（`REVIEW` 状态）

- 左侧：设计稿与契约（Markdown，可编辑）。
- 右侧：任务表，每行显示领域、依赖、写入范围、验收命令、分配模型、一次通过把握、预估成本；模型列为下拉，可改派，改派后整表重算。
- 底部汇总：计划预估总成本、对各订阅额度的预计占用、相对参考模型的预计节省。
- 风险标注：无模型达到把握门槛的任务、需要用户确认的验收命令、写入范围冲突。
- 操作：确认执行 / 提出修改意见（增量重规划）/ 放弃。

### 3.2 流水线

- 任务 DAG：节点颜色表示状态，节点上显示模型与成本；续用窗口用连线标注；关键路径高亮。
- 可切换为看板（按状态分列：等待 / 就绪 / 运行 / 验收 / 完成 / 阻塞）。
- 阻塞任务内联操作：重新拆解 / 终端接管 / 跳过。
- 顶部：暂停 / 恢复队列，实际与预估成本进度条。

### 3.3 会话

- 选中任务或执行窗口时显示日志流：简报（可折叠）、助手输出、工具调用（折叠）、验收结果、任务报告。
- 上下文条：分段显示基线 / 历史 / 有效上限，缓存剩余时间倒计时。
- 上下文建议条：动作、预估节省、原因；自动模式下显示「将在发送时执行 · 撤销」。

### 3.4 用量与质量看板

- Provider 卡片：额度环（标注快照 / 推算）、重置倒计时、缓存命中率、熔断原因。
- 成本构成：规划 / 执行 / 修复占比。
- 首次通过率矩阵：模型 × 领域，单元格显示通过率与样本数。
- 节省总览：按 ASSIGNMENT / CONTEXT / DIRECT 分类，标注「估算」，可展开查看参数。
- Token 堆叠面积图：未缓存输入 / 缓存读 / 缓存写 / 输出。

### 3.5 设置

Provider 探测结果、规划模式与规划模型、把握门槛、上下文模式、预算红线、超时倍数、工作区隔离模式、参考模型、定义包版本与更新、模型覆盖。新增：

- 权限与安全：授权模式（每次询问 / 敏感询问 / 全自动，全自动需二次确认并显示时限）与按项目 / 当前会话的覆盖、命令与文件编辑试判（级别、原因、三档模式下的结果）、指令名单查看与本地规则、各 CLI 的沙箱与凭据隔离能力（完整 / 部分 / 不支持）。见 [11](../11-command-authorization/README.md)。
- 网络代理：全局默认 + 已安装且能获取模型列表的 CLI 逐个设置。见 [12 §5](../12-network-proxy/README.md)。
- 模型能力：内置 / 在线 / 自定义定义包的分层查看与字段来源。见 [04 knowledge-pack §7](../04-model-matching-engine/knowledge-pack.md)。
- 关于：版本、commit、构建类型、源码链接与安装包校验方式（[11 §7.1](../11-command-authorization/README.md)）。

### 3.6 审批队列

- 待审批命令以非模态面板出现，并在顶栏显示计数；每项显示任务、CLI / 模型、命令、风险级别与原因。
- 操作：允许一次 / 本会话允许 / 写入项目名单 / 拒绝 / 拒绝并说明；键盘可完成。
- 全自动模式下顶栏常驻警示条。

## 4. 状态与数据流

```
Tauri Channel ──► eventRouter（单例）
                    ├─ log_append              → streamStore（按 attemptId 缓冲，rAF 合帧）
                    ├─ task / attempt_status   → setQueryData 精确更新 DAG 节点
                    ├─ plan / assignment       → invalidateQueries(['plan', id])
                    ├─ context / suggestion    → contextStore
                    ├─ gate / cost             → setQueryData
                    └─ notice                  → toast 队列
```

- 首屏经 Query 拉取，之后由事件增量更新，不轮询。
- 流式文本只追加、每帧最多刷新一次；完成后用落库内容替换缓冲。
- WebView 重载后按各窗口最后的 `logId` 补齐。
- 缓存倒计时由前端根据 `cacheWarmUntil` 本地计算。

## 5. 快捷键

`⌘K` 命令面板，`⌘N` 新需求，`⌘↵` 确认计划，`⌘.` 暂停队列，`⌘1/2/3` 切换计划 / 流水线 / 会话视图。

## 6. 可访问性

- 全部交互可键盘完成；DAG 视图提供等价的表格视图，供键盘与读屏用户使用。
- 状态不只靠颜色：节点与状态灯同时带图标与文字。
- 日志流使用 `role="log"` + `aria-live="polite"`，只播报任务完成、阻塞、关卡失败与上下文建议，不逐字播报流式文本。
- 进度类组件使用 `role="progressbar"` 并提供 `aria-valuetext`（如「计划成本 $0.61，预估 $0.92」）。
- 对比度目标 WCAG 2.2 AA；尊重 `prefers-reduced-motion`。
- 正式发布前需用 VoiceOver / NVDA 实测并经人工评审。

## 7. 安全

- Markdown 渲染经 `rehype-sanitize`，禁止原始 HTML。
- 链接通过 Tauri opener 在系统浏览器打开，拒绝非 http(s) 协议。
- CSP：`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost`。
- 前端没有直接执行命令或访问文件系统的能力。
- CLI 面板展示 Base URL 时附来源提醒与源码链接（[11 §7.1](../11-command-authorization/README.md)）。

## 8. 性能目标

| 指标 | 目标 |
|------|------|
| 冷启动到可交互 | < 1.5 s |
| 20 个任务的 DAG 实时更新 | 60 fps |
| 单窗口 10 万行日志滚动 | 60 fps |
| 4 个执行窗口同时流式输出 | 主线程单帧 < 8 ms |
| Rust 进程空闲常驻内存 | < 50 MB |
| 整体（含 WebView）空闲常驻内存 | < 150 MB |
