# CLI 查询已知缺口处理清单

2026-10-25 用户授权处理所有已知缺口。以当前源码、可访问环境和证据为准；不以清单勾选代替实现或验证。完成并验证的开发子节点自动本地提交，不自动推送。

- [x] G1 Codex：当前 CLI 自管 ChatGPT 认证的在线额度管理协议，账户/工作区绑定，免推理、取消和有界生命周期；正常刷新由 CLI 管理，不自行实施登录。
- [ ] G2 已知钱包来源：补 Aider 官方 DeepSeek 等可以明确绑定的来源；核对 OpenCode 环境/V1、Reasonix 解析优先级，拒绝对未提供余额 API 的服务商伪造通用额度。
- [ ] G3 认证恢复与类型：Kiro 正常 grant 刷新的身份语义与失败恢复；Claude 短期过期只读诊断；HTTP 超时分类及真实 Store 失败隔离。
- [ ] G4 能力/界面/文档：真实支持按认证、版本、平台记录，明确主动查询与本地历史；修正新增来源的标签与权限说明。
- [ ] G5 验证：严格离线协议/换号/401/429/取消/重启/未知结构；当前已安装版本的真实非推理诊断与正常桌面验收。
- [ ] G6 外部条件：其他 OS、套餐、账号和独立账单对照；用户已被询问是否存在独立测试环境。无法访问的实机条件明确保留，不能用 fixture 冒充通过。

首个 Codex 独立管理诊断已预登记于外部 `tokage-cli-gaps-awjulw9x`：initialize、account/read(refreshToken=false)、account/rateLimits/read、account/read；3.462秒成功，回执accountId匹配CLI当前账号，前后账号/认证文件未变化，无thread/turn/login/logout/credit-consume请求。仅当前安装0.160.0的管理诊断，不等于生产传输或桌面验收。

官方协议：[App Server](https://learn.chatgpt.com/docs/app-server)。账户读取/额度查询与模型turn分别处理；CLI-managed认证的正常refresh不等于由Tokage接管账号。进程在空临时目录运行，采用正常HOME/代理及公开的空MCP配置覆盖，防止加载项目和MCP扩展。

G1实现及原生真实查询完成：产品管理沙箱3464ms返回2窗口/1数量；401项原生、481项前端通过，类型/i18n/构建/桌面crate/隔离检查通过；外部桌面验收保留G5。详见 [Codex验证](codex-usage-validation.md) 与 [原生回执](reliability-evidence/cli-gaps/g1-codex-native.json)。
