# U4-001：用量体验与登录探测修复

日期：2026-10-24。此节点完成代码与自动回归；不代表所有 CLI 实机验收完成。

## 已实现

- 统一授权面板补齐内边距和可见开关名称。CLI 表头只保留身份、可执行名、状态；启用开关、刷新、恢复等放入正文，账号、订阅、模型与用量使用可换行响应式布局。
- 应用生命周期自动查询：授权后、可见时按缓存有效期与失败退避刷新；每五分钟重新检测状态，不自动运行消耗 token 的 Key 验证。单卡只保留一个状态与用量刷新入口。
- Copilot 使用 SDK auth.getStatus/account.getQuota 管理接口，无模型会话；保持缓存账号认证，不添加屏蔽本地登录的参数。保留服务端原始计量单位，不凭空换算金额或百分比。
- Reasonix 读取其全局配置与 .env 中明确连接的官方 DeepSeek 来源，调用固定只读余额接口；OpenCode 和 Reasonix 钱包均独立于当前任务模型，不用于通用调度熔断。
- AGY 绑定 macOS Keychain 中 gemini/antigravity 的 consumer Google 账号来源；稳定 subject 指纹不因 token 正常轮换改变。用量命令禁止启动浏览器授权。
- Claude 登录扫描只读 CLI 凭据，不再定时启动 auth status。正常 access token 过期且 refresh grant 可用不直接要求重新登录；清空或明确过期的凭据仍显示失效。稳定账号标识优先用于身份绑定。
- Kiro 识别 Logged in with Google/GitHub/Builder ID；缩短模型目录探测超时，避免已成功的 whoami 被后续探测拖成未知。
- 显示分组额度、精确余额、数据时间、缓存或历史状态，不混合模型额度或虚构重置时间。

## 核查及限制

- Copilot 正常管理接口实测认证成功且返回 quotaSnapshots；之前 no-auto-login 诊断的未认证是参数屏蔽缓存登录导致，不能当作用户退出登录。
- Claude 本机文件中的 access/refresh 均已清空；没有历史调试日志，无法确定清除来源。上游 issue 95822 报告短命令可能启动刷新却在保存轮换凭据前退出；本次移除此扫描风险，但不能断言它就是本机根因，也不能承诺延长官方有效期。用户需正常重新登录一次。
- Kiro 当前版本 2.34.0 的安全免推理用量适配仍未完成；保持 ADAPTER_PENDING。仅 whoami 成功不等于额度接入完成。曾测试绝对路径 agent 参数，被 CLI 明确拒绝，未调用模型；后续不采用该路径。
- Codex 当前主动账号绑定仍 IDENTITY_PENDING。Claude 已失效，无法完成当前账号在线额度实测。
- Reasonix/AGY 的新完整管理器链路及全部 CLI 自动查询，仍需在已提交源码的外部 checkout 通过正常桌面 UI 验收。本次浏览器 mock 只证明布局，不证明真实登录或余额。

## 自动验证

- 原生 lib：378 passed，28 ignored；初轮发现旧 Claude fixture 依赖已移除的 auth status，已改为测试内合成凭据；并发取消测试初轮超时，限制测试并发后全量通过。
- 追加 Claude 账号/token 轮换测试：credentials 4 passed。
- 前端：462 passed；初轮无关 SessionHistoryView 测试超时，maxWorkers=4 全量重跑通过，未放宽测试超时。
- TypeScript、生产构建、i18n（1816 keys）、test-isolation 检查通过。构建仍有既有 chunk/import 警告。
- 普通浏览器 UI 核查约 462px 宽预览：授权卡片内边距、标题、开关和正文操作均可见。截图 /tmp/tokage-cli-layout-final.png；只作视觉核查。

参考：
- https://github.com/anthropics/claude-code/issues/95822
- https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing
- https://github.com/esengine/DeepSeek-Reasonix/blob/studio/docs/CONFIG_PATHS.md
