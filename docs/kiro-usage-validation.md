# Kiro 自动额度深化验证（U5-006）

**当前结论：U5-006实现及U5-007本机Kiro/Reasonix桌面生命周期验收完成。** 原锁屏尝试及原生诊断为历史步骤，最终正常UI结果见文末与 acceptance-result.json；其他认证/套餐/平台保持未测。

日期：2026-10-25。平台：本机 macOS arm64。CLI：2.34.0；认证：Google social；套餐：KIRO PRO+。

## 根因与实现

原 legacy `/usage` 需要先启动聊天引擎并加载模型目录。当前网络下前 91.13 秒无输出，91.46 秒出现完整额度面板，93.44 秒退出，超过原 90 秒限制。不是登录未知，也不是收到额度后等待退出。V3 ACP initialize 成功，但安装版没有声明 `_kiro/account/getUsage`，未调用未声明 RPC。

安装版官方 `@kiro/agent` SDK 的 `GetUsageLimitsCommand` 使用固定 `GET /Get-Usage-Limits?origin=KIRO_CLI`，管理服务为 `https://management.<region>.kiro.dev`，profileArn 为 query、当前社交登录 access token 为 bearer。本次以该只读协议替代每次完整聊天启动；不是调用模型或会话接口。该接口来自已安装官方 SDK，并非承诺稳定的公开第三方 API；升级后未知结构安全失败。

复用现有受授权、可取消的 HTTPS transport；只支持当前 CLI 自有单一 Google/GitHub auth namespace，以及严格验证的商业 us-east-1/eu-central-1 profile ARN。URL/region 不是调用方自由输入；凭据与 ARN 仅通过 curl stdin，禁止重定向、curlrc、任意响应持久化。保留正常代理和 CA。Kiro 15 秒连接/25 秒请求；DeepSeek 原有期限不变。

若服务商拒绝 bearer，由原固定 `/usage` 管理命令让 CLI 自行处理认证刷新；不实现 OAuth 客户端、不写个人认证配置。该命令上限 120 秒，总预算 150 秒；网络/429/无效结构不启动慢回退。失败的管理启动/登录面板仍保留已知 Authentication 类型。授权撤销仍取消整个调用，前后来源核验仍拒绝 grant/account 改变。实际过期刷新成功分支尚无本轮实机证据，不宣称已验证。

`currentUsageWithPrecision` / `usageLimitWithPrecision` 优先于整数；credits 不换算美元、tokens 或请求数。套餐、试用、奖励、超额分别展示，保留 Estimated 和服务商实际 reset timestamp；不相加，不作为 account-wide 熔断证据。零额度不除零，超额数量不被进度条截断值替代。重复/未知维度拒绝猜测；不保存 userInfo/email/ARN/token。旧 legacy parser 同时拒绝重复面板。

## 验证记录

外部诊断目录：`tokage-kiro-account-diagnostic-js9tdtm8`（系统临时目录）。诊断已预登记，只用于只读管理查询，不作为桌面验收。

- ACP initialize：9.996 秒；没有账户额度扩展，不创建会话。
- 首次直接 API：连接期限 5 秒超时，保留失败。
- 按 SDK 15 秒连接期限：6.327 秒返回 HTTP 200；第二次 6.271 秒。认证来源不变，无模型请求。
- 原生产品新路径：首次6186ms；b5d60c5 提交后相同原生源码复测6081ms，1 个 scoped window / 1 个 credits quantity，前后身份相同；非 UI 诊断通过。
- API 返回套餐 Pro+；已用 1980.4 / 2000.0 credits；reset epoch 1793491200。仅是此次服务商估计值，随实际用量变化。
- 前端 480 项通过；最终原生全量 397 passed / 29 ignored，相关集 57 passed / 1 ignored。覆盖精度/零/超额/多池、未知维度、固定 endpoint/ARN stdin、无需启动 CLI 的正常查询、429、管理启动失败保留认证错误、撤销授权。
- TypeScript、i18n、前端生产构建、隔离静态检查通过。原生桌面 cargo check 通过；最终版外部桌面验收在下方补充。

独立桌面登记：[request.json](reliability-evidence/kiro-u5-006/request.json)。提交版独立构建、正常 UI 自动查询/取消/跨卡刷新/两轮扫描/重启尚待完成；不得将上述原生诊断转记为桌面通过。


## U5-007 独立候选与首次桌面尝试（历史，解锁后已完成）

实现提交 b5d60c5，外部目录 tokage-desktop-test-vy_clmeh；普通 release app 构建、预登记 ad-hoc 签名和严格验证通过，源码克隆不变。候选哈希与配置来源见 [candidate.json](reliability-evidence/kiro-u5-006/candidate.json)，原生诊断汇总见 [native-result.json](reliability-evidence/kiro-u5-006/native-result.json)。

首次桌面 getApp 明确返回 Mac locked / manual unlock required；未绕过锁屏或改系统权限，已请用户手动解锁。此尝试只计为准备完成，UI 尚未执行，U5-007/U5-004 保持未完成。


## U5-007 解锁后的正常桌面验收

用户手动解锁后继续同一已签名候选；没有修改源码或环境。初始9个CLI Ready，在线权限默认关闭，没有全体“账号未确认”；Demo/Debug Provider 均 Off。通过普通设置 UI 只保留 Kiro 与 Reasonix 启用后授权。

Kiro 自动取得 HTTP 回执：12:40:37，Pro+，1980.4 / 2000.0 credits，Estimated，99.02%已用（界面约1%剩余），真实 reset 2026-11-01 00:00 UTC；表头 Ready、Google account 与当前查询一致。Reasonix 钱包自动成功且提示只引用自身配置。手动刷新 Reasonix 后，仅其获取时间推进到12:41:08，Kiro保持原时间与数值。

Kiro卡片明确显示 Querying 时关闭授权；51.107秒后（超过该直接HTTP路径25秒期限）未发布或写入迟到成功，旧快照保持历史。重新开启后恢复可信缓存，未把重新授权时间写为新获取时间。此次取消不是在检测或排队阶段推断。

12:43:58、12:49:03两轮五分钟自然扫描，两个Provider的generation、成功获取时间和额度保持；没有重扫整页清空或重复查询。授权开启时正常Quit/重开，启用范围仍为两家，权限开启，当前来源核验后可信额度仍为原获取时间。关闭授权后再次正常Quit/重开，权限关闭、原生同意文件不存在，旧快照明确为历史；超过旧TTL也未产生新成功。只声明这些具体原生设置与快照的重启行为，不泛化为所有WebView前端设置持久性。

结束通过正常UI关闭授权并Quit，独立进程已退出；只读观察确认成功快照仍只有Kiro/Reasonix；外部克隆源码干净。没有模型请求、账号切换、个人凭据配置修改、页面注入或Store/DB写入。真实expired bearer刷新、其他认证/套餐/平台仍不宣称通过。

结果：[acceptance-result.json](reliability-evidence/kiro-u5-006/acceptance-result.json)。37份外部证据的哈希：[evidence-sha256.json](reliability-evidence/kiro-u5-006/evidence-sha256.json)。原始AX/截图仅在外部目录，不提交账号内容；首次锁屏失败记录保留。U5-007及U5-004的本机限定验收完成，其他发布门槛仍单列。
