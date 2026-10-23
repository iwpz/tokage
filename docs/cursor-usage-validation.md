# Cursor 用量展示验证

节点：`cursor-usage-001`。日期：2026-10-24。

## 实现范围

- 设置 → CLI 管理 → 官方订阅 Cursor 卡片 → 刷新额度。查询沿用 `query_provider_quota` 正常 IPC、Provider Store 与原生额度缓存；API Key / Gateway 不显示订阅刷新入口。
- 原生启动不带 prompt 的 `agent`，使用 CLI 现有信任目录。等待正常输入界面，键入 `/usage`，只有补全明确显示 `Show plan and on-demand usage` 时才发送 Enter；未识别 / 登录 / 信任提示不进入聊天或自动确认。
- CLI 自己认证和请求，Tokage 不读 Cursor 密钥、不请求厂商接口。后端加载当前代理配置并调用共用代理准备 / 环境组装；失败不回退直连。当前实现不证明任意用户环境的最终公网 IP 一定相同。
- 解析 Included、Auto、API、On-Demand 四类数据。Included 是套餐总体利用率，不相加；日期只有月 / 日时保留 CLI 原文，不虚构精确 UTC reset。Disabled 是状态，不画 0% 额度。
- 新增可选快照 details，旧快照不需要该字段；现有窗口展示保留。查询时间明确标为 CLI 用量命令来源。
- 本轮不实施全局在线权限改造、账号管理或 Kiro 查询接入。原有主动轮询默认关闭；手动按钮明确执行官方 CLI 查询。

## 已验证

- 本机 `agent` / `cursor-agent` 同指向 2026.10.22-27dcf5f。此前普通终端从既有信任目录进入 UI，确认内置 `/usage` 后得到 Pro+、Resets Nov 6、Included 33%、Auto 34%、API 8%、On-Demand Disabled。未输入初始或普通聊天 prompt，未新增目录信任。
- Rust 额度 19 项、PTY 6 项回归通过，包含未识别命令永不发送 Enter、部分 / 无效数据拒绝、零百分比、分项不相加、现有信任读取及 Key / Gateway 在启动前拒绝。
- 前端用量组件与 Provider Store 7 项通过；检查分项 meter、Disabled 无假进度、CLI 来源及旧快照兼容。
- TypeScript、i18n、前端生产构建与 Rust native / Tauri `cargo check` 通过。追加共享用量 11 项、环境 / 代理 10 项、adapter 7 项回归也通过：合计 53 项相关 Rust 测试、7 项前端测试。

## 独立提交验证（cursor-usage-002）

源码提交 `93a7655`，通过普通 `git clone --shared` 建立独立 checkout，隔离静态检查通过且源码保持干净。外部临时 Rust 程序仅依赖该 checkout 的公开 `query_cursor_quota`、正常 `env::capture` 及 `prepare_turn_proxy`，未修改产品、认证、HOME 或代理环境。以默认 `CliOwn` 策略运行，不验证桌面中保存的其他代理模式。

真实 `agent` 2026.10.22-27dcf5f 查询成功，退出码 0、stderr 为空，结构化结果：`windowMinutes=0`、`usedPercent=33`，details 为 Pro+ / Resets Nov 6 / Included 33 / Auto 34 / API 8 / On-Demand Disabled；`source=CLI_OUTPUT`。调用前登记 30 秒查询截止及环境范围。已有信任目录由正常产品函数选择，没有 `--trust`、初始 prompt、`-p` 或 Tokage HTTP 请求；没有执行普通模型任务。

证据位于 `/tmp/tokage-cursor-usage-P9wbWZ/`：`request.json`、`probe/src/main.rs`、`result.json`、`stderr.log` 及 `SHA256SUMS`。临时目录可能被系统清理；本记录保留关键版本、入口、限制和结果。此检查证明新原生捕获 / 解析的真实正常路径，不等同于 Tauri IPC、桌面设置操作或账单级零消耗验收。

## 待完成的验证

- 完整桌面外部 UI 验收、Linux / Windows、所有套餐和认证变体未完成。未使用注入式桌面驱动、修改 Store / DB 或运行付费模型测试。
