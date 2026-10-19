# m2f：生产接线与持续回归

日期：2026-10-19。本节点选择继续本地回归，GitHub Actions 未启用；本机通过不称为 CI 绿灯。m2g 的完整桌面故障恢复、真实服务商异常及 m3 日常使用仍待验收。

## 上下文能力的决定

`plan_in_window_context_cmd`、`apply_context_action_cmd`、`dismiss_context_action_cmd`、`build_compact_cmd`、`record_context_action_cmd` 五个入口暂缓。它们没有生产调用方，建议估价、原生压缩执行、执行确认和持久化状态之间尚未形成经过验收的闭环，直接接按钮会产生“已应用 / 已节省”的错误承诺。

本次删除这五个 Tauri 命令及注册项、对应 TypeScript IPC 包装器和只覆盖模拟包装器的两项测试，不添加豁免。Rust 底层规划、压缩构建、存储算法及其回归保留；前端 mock 默认值测试仍明确只证明 mock。恢复开放时必须重新设计生产接线、执行确认和失败恢复，不能靠恢复注册项视为完成。

上下文栏持续显示“自动上下文建议与压缩暂未开放；请在原生 CLI 中管理上下文”，移除未使用的手动压缩回调。上下文用量 / 缓存展示继续使用已有数据；不提供建议应用、压缩或节费成功按钮。历史 context-action 的只读查询和已有 settle 接口保留，原只读查询豁免理由改为历史记录，不能声称新写入已接线。未使用的 ContextSuggestionBar 仅为保留组件，不在生产 Workspace 渲染。

## 统一本地入口

```sh
npm run verify:reliability
# 可选：指定新的证据目录；已存在目录拒绝覆盖
npm run verify:reliability -- --output /tmp/tokage-reliability-new-run
```

m2f 交付时顺序执行 10 项必要检查（m2g 后新增驱动回归与归档核验，当前为 12 项）：验收脚本自身回归、TypeScript、前端测试、Rust workspace（含 doc-tests）、生产构建、i18n、IPC 接线、CLI 固定基线及协议 fixture 指纹、m2c 交付归档复核、m2e 恢复归档复核。任一退出非零、执行文件缺失或 Rust 空测试选择使总退出码为 1；后续检查仍执行，避免首个失败掩盖其他结果。日志保留原退出码，每项运行后写入 summary.json，统计 passed / failed / skipped。

默认证据目录位于系统临时目录，脚本打印绝对路径；不自动提交日志或用户环境。源码或依赖有改动、CLI 升级前后、创建交付提交前都执行该入口。失败必须修复并重新完整执行，保留中间失败依据；本脚本提供阻断退出码，不安装绕过用户选择的 Git hook，也不能阻止人直接手工 commit。

本地模式不运行 `--ignored`，清除继承的 `TOKAGE_LIVE_SMOKE`，并明确过滤唯一可通过该变量发起真实请求的 readiness smoke；忽略的付费用例、过滤的 readiness 检查单独记录，不计作通过。Rust 的 provider / 协议替身和 Git / SQLite 用例仍执行。它是本机无模型请求回归，不承诺所有测试完全无网络（如 Cargo 依赖解析）。

## 复跑暴露的锁释放问题

统一入口第二轮实际返回失败：`pipeline_concurrent_writes_and_stale_revisions` 在同进程 Mutex 保护下仍遇到 OS `WouldBlock`。只关闭 lease 自身描述符不足以结束 flock 所有权：描述符被复制或在另一线程启动子进程时短暂继承，会把锁延长到副本关闭。Unix lease 现在在 Drop 中先显式 `LOCK_UN`，再关闭文件；不删除锁文件，不放宽 CAS，不重试覆盖其他写入者。

新增确定性回归：保留 dup 描述符时旧 owner 仍排斥竞争者；释放 owner 后新 owner 立即获得锁；随后关闭旧副本也不能释放新 owner 的锁。原跨进程竞争、SIGKILL 自动释放和 16 线程持久化用例均继续执行。Windows 使用原 LockFileEx 关闭句柄路径，本轮不扩展 Windows 验收范围。失败摘要及错误片段一并归档，不把失败轮次覆盖成成功。

## 固定基线和升级复验

`scripts/cli-baselines.json` 固定 Claude Code **2.1.296 / haiku**、OpenCode **2.0.29 / deepseek/deepseek-flash**，与生产设置页使用的 `reliabilitySupport.json` 对齐。保留协议 fixture SHA-256；更改版本、模型或 fixture 必须显式更新清单并审查证据。指纹匹配只证明版本化 fixture 未漂移，协议语义由 Rust parser / adapter 测试覆盖，不能作为新版 CLI 的实机成功证据。

Claude parser fixture 来自 2.1.288；2.1.296 的真实正常路径依据是 m2b 记录，未将旧 fixture 标成新版录制。OpenCode v2 JSONL / session export 来自真实 CLI 与本地模拟模型，真实 DeepSeek 用量证据仍单独引用 m2b。历史版本结果不覆盖当前工作树。

升级步骤：

1. 在独立路径安装候选版本，记录版本、OS、模型请求别名及实际返回模型、认证渠道；保留旧基线，不先修改支持清单。
2. 运行本地统一入口，检查参数、解析器、完成 / 错误事件、恢复、用量和导出 session 契约；更新需要变更的 fixture 时记录来源并脱敏，重算清单指纹。
3. 明确授权真实模型额度后，分别执行候选 CLI 的自然语言桌面验收；`run-live-desktop.py` 支持 `--cli-path` 与 `--version-prefix`，natural 场景实际要求**完整版本相等**。指定候选版本，不允许默默切换到已安装版本、其他模型或服务商。完成 m2g 关键异常实机复验后才扩展可靠性范围。
4. 将实测结果、失败和未测范围记录到证据文档，再同步基线、支持清单和复验记录。`haiku` 是模型请求别名；记录实际解析后的模型，不承诺服务端别名映射永久固定。

设置页对未知 / 不匹配版本显示未验证提示，其他 provider 显示本轮范围外。此处选择**提示限制**，不全局禁止用户使用已有适配器；固定基线的付费验收则拒绝版本不符。

## 付费实机入口（与本地模式分离）

```sh
# 会发送真实模型请求并消耗额度；只有用户明确授权时运行
npm run verify:reliability -- --mode live --allow-paid --provider CLAUDE_CODE
npm run verify:reliability -- --mode live --allow-paid --provider OPENCODE
```

live 模式只接受本轮两个目标，未传 provider 时顺序验证两者；没有 `--allow-paid` 在运行任何命令之前退出 2。复用 `run-m2b-desktop.sh` 的生产桌面自然语言路径，从基线清单读取完整版本；模型固定，不能静默回退。缺失 CLI / 登录拒绝 / 模型错误 / 版本不符 / 桌面或文件验收失败使目标失败；即便子脚本错误地返回 0，独立输出的 skip 行也统计为 skipped 并令总验收失败。保留每目标退出码及日志。旧 `run-live-cli-e2e.sh` 的 skip-if-absent 发现脚本不在此验收入口中，不能拿其退出 0 证明选定组合通过。

本节点未发起付费实机请求。基线使用此前记录，新增脚本的失败传播、缺失命令、skip 分类、付费许可门槛、空 Rust 选择和基线漂移均由隔离子进程 / 临时文件回归验证。最终本机运行记录见 [验证记录](reliability-evidence/m2f/validation.md)。

后续状态（2026-10-19）：[m2g 汇总](reliability-m2g.md)补充两组选定组合的完整桌面任务阶段强杀 / 冷启动阻断证据。本文上述“仍待 m2g”等表述保留节点当时的验收边界；其他阶段的分层证据不升级为完整桌面逐阶段强杀，服务商真实断网 / 额度耗尽和 m3 日常试用仍未验收。
