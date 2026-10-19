# m2g-r1 验证记录

日期：2026-10-19。范围为任务会话消息、运行中流式查看、转录持久化与 attempt 终态显示；未开始 m3a。

## 本地最终结果

`npm run verify:reliability -- --output /tmp/tokage-session-stream-release-checks` 退出 0：12 项通过、0 失败、0 跳过，见 [summary.json](local/summary.json)。

- 前端：41 个文件、319 项通过，包含两个独立任务的消息隔离、迟到事件、取消、保存失败重试、慢写入顺序、会话读取竞态及 attempt 终态 / 取消竞态回归。
- Rust workspace（含 doc-tests）：592 项通过、0 失败，27 ignored 和 1 filtered 不计入通过。
- TypeScript、普通前端构建、国际化（1641 keys）、IPC 接线（134 命令、0 个未豁免缺口）、CLI 固定基线、m2c / m2e / m2g 既有证据核验全部通过。
- 独立 oracle 的 4 项 mutation 检查通过；等价 `npm run test` 不改变原始输入 / 边界断言，附加跳过参数仍被计划审阅拒绝。
- 普通桌面构建另行执行 `cargo build --manifest-path src-tauri/Cargo.toml --bin tokage --features custom-protocol --locked`；不启用 live-acceptance，不发送模型请求。

首次统一检查并非通过：FakeAdapter 的 Codex 协议回放因未消费输入而 Broken pipe，原始 Rust 失败与退出码保留在 `first-local-failed/`。修正替身读取 stdin 后，原用例通过，最终完整检查也通过；没有降低生产输入送达检查、忽略失败或将首轮计为通过。

## 实机结果分开判定

真实 CLI 固定 Claude Code 2.1.296 / haiku，官方订阅，独立临时仓库和应用数据，无 provider / model fallback。

第一次：真实计划生成了 `npm run test`，在任务启动前被审阅 allowlist 拒绝。原失败见 `first-desktop-rejected/`；仅补充这一确切等价命令后重新执行，未改写模型生成的计划。

第二次：两个并行任务处于 RUNNING 时，验收驱动在实际 session 树中分别选择它们；右侧用户提示可见，assistant DOM 的内容 / 过程数据在仍 RUNNING 时改变。两项 marker 均无转录保存错误，见 [streaming-checks.json](desktop/streaming-checks.json)。独立 SQLite 导出证实各自 USER / ASSISTANT 归属正确且均有非空内容，见 [task-transcripts.json](desktop/task-transcripts.json)。

随后 Haiku 返回真实 session limit：整个桌面验收退出 1，失败任务 BLOCKED，另一任务 CANCELLED，下游 SKIPPED，无交付。部分输出和错误 / 取消的消息状态均保留。不能将“双任务流式通过”表述成“完整交付通过”，见 [facts.json](facts.json) 和 [原始结果](desktop/result.json)。未重试已耗尽的账户，未切换模型，也未修改真实账号 / 额度配置。

此真实错误另外暴露了旧 attempt 终态遗漏：原始导出中失败任务的 attempt 仍是 RUNNING，未手工改写。随后代码补齐在 BLOCKED 前落盘 FAILED / TIMEOUT，新增回归要求实际调用 upsertAttempt，并验证保存等待期间的取消不会被错误回调覆盖。该追加修复通过最终全量本地检查，但没有在服务商额度耗尽后重复发送请求。

## 源码与边界

[desktop-build-source.json](desktop-build-source.json)保留实际桌面流式验证使用的源码指纹与验收构建 SHA-256；[environment.json](environment.json)保留包含终态追加修复的最终源码指纹。两个版本的差异如实分开，不能把后续本地回归说成同一实机二进制已测。

归档仅包含合成需求的白名单日志、状态、消息导出与必要指纹；本机用户目录和现场根目录替换为 `<home>` / `<fixture>`。不提交账号凭据、原生用户会话库、完整应用目录或 WebView 数据。文件指纹见 [sha256.json](sha256.json)。

历史未保存的任务消息不补造。正常完整交付仍保留未勾选项，待额度恢复后复验；m3a 不开始。
