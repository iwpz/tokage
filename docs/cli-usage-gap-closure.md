# CLI 查询已知缺口处理清单

2026-10-25 用户授权处理所有已知缺口。以当前源码、可访问环境和证据为准；不以清单勾选代替实现或验证。完成并验证的开发子节点自动本地提交，不自动推送。

- [x] G1 Codex：当前 CLI 自管 ChatGPT 认证的在线额度管理协议，账户/工作区绑定，免推理、取消和有界生命周期；正常刷新由 CLI 管理，不自行实施登录。
- [x] G2 已知钱包来源：补 Aider 官方 DeepSeek 等可以明确绑定的来源；核对 OpenCode 环境/V1、Reasonix 解析优先级，拒绝对未提供余额 API 的服务商伪造通用额度。
- [x] G3 认证恢复与类型：Kiro 正常 grant 刷新的身份语义与失败恢复；Claude 短期过期只读诊断；HTTP 超时分类及真实 Store 失败隔离。
- [ ] G4 能力/界面/文档：真实支持按认证、版本、平台记录，明确主动查询与本地历史；修正新增来源的标签与权限说明。
- [ ] G5 验证：严格离线协议/换号/401/429/取消/重启/未知结构；当前已安装版本的真实非推理诊断与正常桌面验收。
- [ ] G6 外部条件：其他 OS、套餐、账号和独立账单对照；用户已被询问是否存在独立测试环境。无法访问的实机条件明确保留，不能用 fixture 冒充通过。

首个 Codex 独立管理诊断已预登记于外部 `tokage-cli-gaps-awjulw9x`：initialize、account/read(refreshToken=false)、account/rateLimits/read、account/read；3.462秒成功，回执accountId匹配CLI当前账号，前后账号/认证文件未变化，无thread/turn/login/logout/credit-consume请求。仅当前安装0.160.0的管理诊断，不等于生产传输或桌面验收。

官方协议：[App Server](https://learn.chatgpt.com/docs/app-server)。账户读取/额度查询与模型turn分别处理；CLI-managed认证的正常refresh不等于由Tokage接管账号。进程在空临时目录运行，采用正常HOME/代理及公开的空MCP配置覆盖，防止加载项目和MCP扩展。

G1实现及原生真实查询完成：产品管理沙箱3464ms返回2窗口/1数量；401项原生、481项前端通过，类型/i18n/构建/桌面crate/隔离检查通过；外部桌面验收保留G5。详见 [Codex验证](codex-usage-validation.md) 与 [原生回执](reliability-evidence/cli-gaps/g1-codex-native.json)。

G2a Aider官方DeepSeek全局钱包已实现并通过原生真实只读查询；共享额度62项/环境10项/前端482项通过，未知配置与网关拒绝猜测。G2仍需完成OpenCode V1/环境来源核对；不将整个G2提前勾完。详见 [Aider验证](aider-wallet-validation.md)。

G2b OpenCode版本分流及环境来源完成：10项协议/配置测试、10项环境测试通过，当前V2真实钱包查询584ms；V1保留实机条件，见 [兼容验证](opencode-wallet-compatibility.md)。Reasonix来源优先级继续核对。

G3a完成：固定HTTP传输将curl退出28归类Timeout、63及超限/无效响应归类InvalidData；保持取消与401/429/重定向语义，固定目标/凭据隔离集成测试3项通过。

G2核对：Reasonix钱包始终明确命名为自己的全局官方DeepSeek连接，仅读其config.toml声明的唯一provider和同目录.env，不推断当前项目/模型的账单；重复变量、未知/网关配置拒绝。无法从当前公开源码证明其他Reasonix配置层优先级，因此不扩张为通用任务账单解析。

G3b完成：Kiro账号键改用经固定官方目标校验的provider/profile/路径，grant旋转不误报换号；缺access时允许固定官方管理命令自行处理刷新，并受同一撤销权限控制。Claude/Codex/Kiro/AGY本地来源单独记录安装盐保护的凭据版本，仅同身份新凭据解除既有认证失败；429/网络冷却不清除，快照时间不改写。无源码可见凭据的其他认证方式不猜测credential revision。

只读Claude当前元数据：access已过期，refresh存在且声明到期时间仍未来；普通access过期不要求重新登录。Tokage不能延长官方令牌时限，也不能证明过去失效的历史原因。继续禁止短生命周期claude auth status探测。真实Store写失败已有离线回归保证旧快照保留与失败类型，不用修改正在运行的数据库模拟成功。

G3b回归：原生全量409通过/32忽略；最后追加缺access/撤销测试后额度68通过/4忽略，桌面crate check通过。曾因旧fixture使用非法profile而失败，已改为真实格式的合成ARN；不降低生产校验。真实过期刷新与独立换号保留G6。

G3c额外刷新契约测试通过：本地access缺失且有refresh时，固定官方管理argv（以/usage结束）的合成CLI返回估算额度；证明实际进入管理fallback，不以“缺失可执行文件仍报Auth”的同形断言代替。仅cfg(test)追加，不改6b59605产品行为；G5候选仍为6b59605。

G4a能力标记同步：Codex/Aider/OpenCode/Reasonix/Kiro补齐USAGE_QUERY，限定来源仍由独立QueryCapability和当前认证控制；跨两个注册表一致性回归及适配器8项通过。首轮6b59605桌面已取得Codex/Aider等真实结果，Claude/Kiro超时保留，后续修复重新构建，不提前标记G5通过。

G3d修复Kiro过期刷新慢路径：CLI固定管理命令运行时只读观察同provider/profile的access变化；变化后终止legacy进程组，使用CLI刚保存的凭据重试固定GET一次。不同profile立即拒绝；无变化则保留原有管理结果/超时；撤销仍统一取消。合成真实SQLite写入+慢面板回归证明不到5秒获得HTTP结果、不等待10秒面板，Kiro相关7通过/1忽略。6b59605首轮实际超时不追认为通过，最终候选必须重验。

G3e修复Claude PTY准备时序：禁止启动后仅按3秒计时盲发命令；等待官方提示框与快捷键栏，键入/usage后必须识别其“Show session cost, plan usage, and activity stats”内置补全才回车。未知补全/回显不提交，不回答信任/登录提示。查询预算45秒、含来源复核的总预算75秒。当前2.1.288同参数真实诊断6635ms返回2窗口；前后只读元数据确认已过期access被官方正常刷新为有效，无auth status或登录命令。旧15秒及60秒盲输入失败、只键入不提交的诊断均保留，不冒充成功。

额度解析及准备/补全拒绝回归15通过/1忽略，隔离检查通过。诊断仅cfg(test)保存最后稳定帧到外部目录，生产路径无诊断变量或日志。官方令牌时限仍不可由Tokage延长，历史异常注销原因不能从本次刷新推断。
