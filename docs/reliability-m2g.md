# m2g：里程碑验收与证据归档

日期：2026-10-19。限定范围继续采用 [m2a](reliability-m2a.md)：本机 macOS arm64 / build 26A428，Claude Code 2.1.296 / haiku 官方订阅与 OpenCode 2.0.29 / deepseek/deepseek-flash 官方 API，session Full Auto、独立 worktree。历史实机通过只适用于对应源码和日期，不认证默认 CLI 的其他版本。

## 当前结论

m2g 完成，m2 按本文限定的本机分层验收与真实桌面抽样范围收口。两组选定组合均完成本轮自然语言正常路径与完整桌面 SIGKILL / 冷启动恢复；Claude 首次 OAuth revoked 负例与重新登录后的通过分别保留。未确认结果的请求保持 BLOCKED，不能重新派发模型或自动交付；这证明安全停止与接手边界，不证明被强杀的请求从未在服务商侧计费。

[逐项矩阵](reliability-evidence/m2g/manifest.json)覆盖 m2b 的 5 条、m2c 的 5 条、m2d 的 6 条和 m2e 的 4 条验收要求；每条注明通过范围、原因及固定 SHA-256 的原始证据。另按组合列出自然需求正常路径、完整桌面强杀恢复和未测的外部异常。`passed`、`failed`、`untested` 分别统计，分层通过不升级为真实服务商异常通过。

矩阵共 36 条：26 条通过、2 条历史失败已由独立新现场复验解决、8 条明确未测。未测项没有记作通过；本轮不认证真实远端断网 / 耗尽或完整桌面的每一强杀阶段。m3 日常可靠性尚未验收。

## 完整桌面强杀恢复

新增 `scripts/run-m2g-crash.py` 与显式付费入口 `scripts/run-m2g-desktop.sh`。沿用真实渲染 UI、Tauri IPC、固定模型及隔离合成仓库；此场景使用固定两任务 DAG，真实自然语言规划与多文件并行 DAG 另由 m2b 正常路径证明，不能混称同一次全链路。

1. 精确检查已有 CLI 二进制版本，在独立应用数据与 WebView 数据中打开真实桌面、审阅并启动计划。
2. 观察真实 CLI 的工具事件，确认 T1 仍 RUNNING、无完成凭据，保存流水线、一次性派发凭据指纹及 OS 进程登记。
3. 对本次启动的完整 Tokage 桌面宿主发送 SIGKILL，并验证退出码为 -9；不只强杀测试 helper 或 CLI。
4. 使用同一隔离数据目录重新启动同一桌面构建。生产启动清理回收旧进程，生产冷加载将任务变为 INTERRUPTED。
5. 从已载入的流水线执行生产 `continueTask`，同时调用两次，检查旧 turn 身份、原生进程空闲和 launch receipt。不确定结果必须 BLOCKED，下游保持 PENDING，不能交付。
6. 外部 Python 进程检查无新派发凭据、旧 PID / 进程组均消失、原始仓库 HEAD 和所有原始文件保持不变；从隔离 SQLite 导出任务、用量与未知费用归因，核对原 turn、无错误 DONE、缺失账单回执仍有 unknown-cost gap。

首次 OpenCode 强杀后进程回收和 INTERRUPTED 落盘已发生，但验收驱动又重复调用加载，返回空结果时界面已载入同一流水线，因此该尝试判失败。驱动改为优先读取界面已经恢复的流水线，避免重复冷加载；先在同一现场完成只恢复、不派发模型的探测，再从全新现场重跑完整强杀链路通过。没有删除原始失败、把空结果当通过或放宽生产恢复条件。

Claude 首次真实认证失败是独立负例：native error、无 Done / 工具调用、T1 BLOCKED、T2 PENDING，未自动换模型或认证通道。该负例的安全阻断通过，与未达到强杀触发点的失败尝试分别记录。

## 证据与脱敏

仅导出本次合成需求的白名单文件：验收进度、前后快照、独立检查、Git / gate / 数据库归因摘要、必要启动日志和源码指纹。临时根目录替换为 `<fixture>`，本机用户目录替换为 `<home>`。不导出真实认证存储、Key、完整应用数据目录、WebView 数据或用户原生会话数据库；未知费用不伪造为零。已提交的 m2b–m2f 证据通过引用和指纹保留，不复制整个临时目录。

本轮两组正常路径分别产生 4 条实际用量记录、8 次通过门禁，最终只修改 pricing.mjs、format.mjs、invoice.mjs。OpenCode 交付提交 `baac4ba9992ead4125d1b2e7bb2dfb8f5b9bee4c`，Claude 为 `0dc78cb9f9d7b26001119e9404768b094cca246c`，均保留可还原 bundle。Claude 结构化用量按原值归档；累计 / 重复事件口径未在本节点校准，不解释为实际账单或节省，继续留 m3b。

正常路径包含原始需求、真实生成计划、任务路由、交付补丁与提交、独立门禁及人工需求边界核对。真实 Git 的补丁、保留分支与恢复提交仍可分别通过 m2c / m2e 的独立脚本还原，源码自述完成不能替代这些证据。

## 复跑与阻断

本地回归不发模型请求：

```sh
npm run verify:reliability
python3 scripts/verify-m2g-evidence.py
python3 scripts/verify-m2g-evidence.py --require-complete
```

统一入口新增 m2g 驱动回归和证据完整性核验。普通证据核验只证明场景条目、归因、证据指纹与记录结论一致；`--require-complete` 额外要求两组正常路径与桌面强杀恢复，以及 m2b–m2e 必需条目通过，缺失、失败或未测返回非零。未测的外部异常和范围外组合不会因本地回归通过而消失。`replay-deliveries.py` 另在全新临时检出还原两组实际交付提交，核对 bundle / patch 一致，再运行独立原始 oracle。校验器会拒绝把下游错误执行、turn 改变、缺少原生空闲 / launch 事实包装为恢复成功；哈希本身不证明真实行为。

真实请求会消耗账户额度；只运行已获授权的目标，使用已有确切版本，不自动安装、升级或重新登录：

```sh
TOKAGE_CLAUDE_TEST_BIN=/absolute/path/to/claude-2.1.296 \
  bash scripts/run-m2g-desktop.sh --allow-paid CLAUDE_CODE
TOKAGE_OPENCODE_TEST_BIN=/absolute/path/to/opencode-2.0.29 \
  bash scripts/run-m2g-desktop.sh --allow-paid OPENCODE
```

脚本任一目标失败均返回非零，普通前端在退出时恢复。版本不符明确失败；没有 CLI / 认证不得计为 skip 后成功，没有静默模型回退。认证拒绝后需用户在本机重新登录，再显式复验。

## 未覆盖范围与接手

- 未主动制造真实远端网络中断、真实服务商余额 / 订阅耗尽或强制注销；本轮 Claude OAuth revoked 是实际发生的认证拒绝，不能外推到其他外部异常。网络 / 额度错误的生产编排与协议分层证据见 m2d。
- 完整桌面强杀只覆盖真实任务工具操作阶段。gate 前后、提交 / 合并后、交付 / 清理等阶段仍由 m2e 的原生跨进程、真实 Git / SQLite 和前端分层验收证明，不宣称每一阶段都经过付费桌面强杀。
- 恢复不确定结果时保留原工作目录、原生会话身份、任务日志与分支。用户可从任务终端接手核对结果，修改后重新验收；若要重新派发，需明确重规划，不能靠反复继续产生第二次收费请求。OpenCode 的原生会话可见性当前返回 unknown，不能据此推测旧请求未执行。
- 不能保证服务商 exactly-once 消费、绝对预算上限或中断请求的最终账单；费用校准、真实需求连续试用与人工救场成本仍留 m3。
- 其他 CLI / 模型 / 认证 / Gateway、Ask Always / Ask Sensitive、in-place、Linux / Windows / Intel Mac、多实例共享数据目录及恶意脱离进程组的后代未纳入本轮认证。

命令、原始失败与最终结果见 [检查记录](reliability-evidence/m2g/validation.md)。m3 的 20 个连续真实需求尚未开始，本轮不能解释为“已经可以放心大胆地无人看守开发”。
