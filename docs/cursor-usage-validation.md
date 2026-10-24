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

- cursor-usage-002 时尚未完成完整桌面 UI 验收；后续 U5-005 已补足下述本机组合。Linux / Windows、所有套餐和认证变体仍未验证。未使用注入式桌面驱动、修改 Store / DB 或运行付费模型测试。


## 深化检查与 U5-005 修正（2026-10-25）

U5-004 实机发现：普通登录探测与额度来源核验独立运行 status；前者偶发失败、后者成功时，表头 UnknownAuth 与正文可信额度矛盾。之前 Cursor 的 Option 状态读取还把超时/网络错误压成 IdentityUnknown。

修正为：来源核验保留当前 CLI status 的登录证据，与当前绑定代次、状态版本及可执行路径一起返回并合并到原生 ProviderInfo 和前端 Store；不从用量数值、计划名称或历史快照推断登录。重启、换来源及失效时清除内存证明；明确退出登录可下调状态，单纯的存储账号 status 不能覆盖普通探测明确给出的 NeedsLogin。没有旧绑定时，正常扫描也可核验当前 status，因此重新登录后不会卡在旧的未绑定失败条目。网络/超时/无效响应保留类型并退避，不当成确认换号。过期读取不能失效更高代次的新账号。

Cursor 总查询预算从 30 秒改为 60 秒，覆盖两次各最多 12 秒的 status 与中间最多 30 秒的交互 /usage；保留取消与完成后退避。只调整本 CLI 的正常产品预算，不是测试例外。

额度解析拒绝重复面板、重复分项、加载/未知 On-Demand 状态；保留小数和官方日期原文。UI 明确标注百分比为“已用”，不把 33% 已用误读成 33% 剩余。Included、Auto、API 独立保留，Disabled 不生成假额度。

准确性边界：读取的是已安装 CLI 的原生用量面板，最多与它显示的精度一致；不能据此推算精确剩余请求、Token 或美元，也不能消除厂商延迟和四舍五入。当前实际账号 Pro+、CLI 2026.10.22-27dcf5f 属于已核对组合。官方现已区分 legacy 月度双池与新套餐周度池，不能将旧标签映射或月度假设泛化到新套餐；未知布局安全报错。资料：[官方 legacy plans](https://cursor.com/docs/account/pricing/legacy-cursor-plans)、[当前套餐说明](https://cursor.com/docs/models-and-pricing)。当前公开 slash-command 清单不列 /usage，继续以实际 CLI 补全明确识别 “Show plan and on-demand usage” 为发送 Enter 的先决条件。

离线回归：前端全量 480 passed；原生全量 393 passed / 29 ignored。覆盖新鲜认证证据与历史额度分离、登录与额度失败独立、来源路径/代次隔离、明确退出、旧状态迟到与重启证明失效、重复/加载面板拒绝、小数与零值。类型、前端构建、i18n 通过；修正提交的外部桌面及官方面板对照正在执行。


## U5-005 本机桌面复验结果

实现提交 `6587486`，独立目录 `tokage-desktop-test-3w3y_d35`。release 原生构建、本地签名和严格签名校验通过；独立克隆前端相关 15 项通过。正常 UI 只启用 Cursor，其他 CLI 在隔离设置中关闭后才开启在线额度，Demo/Debug Provider 均为 Off。

自动查询成功，源和 UI 一致：Pro+，Included 已用 33%、Auto 已用 34%、API 已用 8%、On-Demand Disabled。原始获取时间 2026-10-25 01:30:32。普通停用/启用使来源代次改变，随后自动恢复同一缓存；01:33:46 自然扫描保持 Ready、账号信息和原始额度时间。正常退出重启后，关闭的授权及历史标签成立；01:36:47 当前来源重新检测完成，再开启查询后恢复 Ready 和原 01:30:32 的可信额度。此次没有复现旧版的表头 UnknownAuth / 正文可信额度矛盾。

未向实机注入超时或退出登录。该失败分支由旧版真实失败证据及本次原生/真实 Store 回归覆盖，不称为此次人为故障注入通过。当前 status 明确认证状态的探测也返回成功，账号标识字段存在；不输出原始认证内容。

独立官方终端对照没有完成：桌面工具明确禁止访问系统 Terminal；官方网页跳转到登录页面，当前没有独立网页登录会话。没有绕过工具限制、复制凭据或登录第三方。因此结论是“已验证本机组合的官方 CLI 数据获取、解析和 UI 链路”，不是“已逐项对照官方账单证明数值绝对准确”。公开文档的新旧套餐差异继续适用。

结果与外部原始证据哈希见 [acceptance-result.json](reliability-evidence/cursor-u5-005/acceptance-result.json)、[evidence-sha256.json](reliability-evidence/cursor-u5-005/evidence-sha256.json)。原始 AX/截图不提交账号信息；保存在外部目录。结束时已关闭在线查询、正常退出隔离实例、关闭临时官方登录标签，没有模型推理或个人 CLI 配置变更。
