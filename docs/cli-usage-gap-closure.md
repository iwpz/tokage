# CLI 查询已知缺口处理清单

2026-10-25 用户授权处理所有已知缺口。以当前源码、可访问环境和证据为准；不以清单勾选代替实现或验证。完成并验证的开发子节点自动本地提交，不自动推送。

- [x] G1 Codex：当前 CLI 自管 ChatGPT 认证的在线额度管理协议，账户/工作区绑定，免推理、取消和有界生命周期；正常刷新由 CLI 管理，不自行实施登录。
- [x] G2 已知钱包来源：补 Aider 官方 DeepSeek 等可以明确绑定的来源；核对 OpenCode 环境/V1、Reasonix 解析优先级，拒绝对未提供余额 API 的服务商伪造通用额度。
- [x] G3 认证恢复与类型：Kiro 正常 grant 刷新的身份语义与失败恢复；Claude 短期过期只读诊断；HTTP 超时分类及真实 Store 失败隔离。
- [x] G4 能力/界面/文档：真实支持按认证、版本、平台记录，明确主动查询与本地历史；修正新增来源的标签与权限说明。
- [x] G5 验证（仅用户确认的当前Mac/现有账号）：严格离线协议/换号/401/429/取消/重启/未知结构；当前已安装版本的真实非推理诊断与正常桌面验收。
- [x] G6 范围确认：用户明确仅使用当前Mac和现有账号；其他OS/套餐/独立换号/账单对照不在本轮验收范围，未认证条件继续明确披露，不用fixture冒充实机。

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

## G5首次构建与锁屏记录（历史）

最终代码候选 `594a91a`（binary SHA-256 `710a9f3799da9afd3f874edf4b77ad6cd9085ed64360c84147565684b11a9440`）已在独立克隆正常release构建，并通过本地ad-hoc签名deep/strict校验。原生411通过/33忽略，前端482通过（73文件），类型、i18n（1853键）、隔离检查与5项隔离设施测试通过。真实固定管理诊断：Claude6635ms/2窗口（过期access由官方刷新），Kiro6157ms/1窗口1数量/身份不变；不是模型调用或桌面验收。

旧候选 `6b59605` 的正常UI证明：初次9/9 Ready、在线默认关闭，无整页“账号尚未确认”；打开权限后Codex/ Aider/ OpenCode/ Reasonix/ Cursor/ Copilot/ AGY取得真实数据。Codex分桶/remaining、三个钱包独立CLI名称正确。Claude/Kiro实际超时保留，关闭授权并正常退出；因此不将旧候选计为全部通过。旧候选缓存复用cp失败后采用完整普通构建成功，未修改源文件修编译。

修正版候选在启动时桌面工具报告Mac再次锁屏。已请求用户手动解锁；没有更改系统权限、自动解锁配置或注入UI。G5最终九卡、跨卡刷新、在途撤销、自然扫描与重启复验待解锁继续，保持未勾选。结构化回执与旧候选证据哈希见 [G5验收](reliability-evidence/cli-gaps/g5-acceptance.json)。

## G6未覆盖组合与限制

- 仅当前Mac/既有登录可用，未获得Linux/Windows、其他套餐或独立换号环境；不通过复制凭据或更改个人CLI来制造组合。
- Claude正常已过期access刷新已由当前官方流程观察通过；Kiro新“刷新后立即重试”有真实SQLite写入/慢CLI离线回归，当前有效令牌有实机GET，但没有新路径真实过期刷新证据。不人为篡改当前账号令牌来补验收。
- 真实服务商401/429与独立账单对照仍需合法测试条件；已有故障fixture只计离线验证。
- 持久WebView：锁定tauri-utils2.10.1和当前上游dev均仍以opt_vec_lit生成Option<[u8;16]>，没有已核实修复版本可直接升级；保留标准incognito隔离限制。需要独立OS用户运行普通包或先完成可审查的产品依赖修复，不为验收改依赖源码。[上游源码](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-utils/src/config.rs)。

以上未覆盖条件不在后续本机通过声明内，不将当前Mac/既有账号验收扩大为所有平台、厂商/套餐和未知认证方式认证。不自动推送或发布。

## G5解锁后限定范围验收完成

用户确认“仅使用当前Mac和现有账号”并解锁后，继续原594a91a候选，未改源码、环境、凭据或应用状态来补通过。初始9/9 Ready、Demo/Debug关闭、在线授权默认关闭；开启后九适配器均自动返回真实额度/命名钱包。Claude首轮Timeout后自动退避重试成功；OpenCode自然扫描来源读取Network后保留旧钱包并自动恢复，均保留失败，不承诺厂商网络永不失败。

- Reasonix单卡刷新只推进自身获取时间，其他七个已完成来源快照hash/时间不变；Claude同期独立自动恢复。
- 两轮自然扫描15:47:16、15:52:16，来源核验没有清空整页额度。关闭再开启后，到期缓存自动续查。
- 最终撤销同时由正常UI“Querying Kiro CLI usage”和外部只读进程观察确认HTTP在途；关闭后持有81秒，超过25秒HTTP期限，九份成功快照/hash/获取时间/尝试时间完全不变。此前一次仅处于检测阶段及两次观察器因macOS comm列截断漏检均不计通过；修正外部观察器，不改产品。
- 授权开启正常Quit/重启，持久权限仍开启；后补的57秒短时间对照证明九份快照及原获取时间不变，来源复核后显示原时间数据。授权关闭Quit/重启同样九份快照/获取时间不变，未自动发出余额请求。最后通过正常UI关闭授权、Quit，并确认确切候选进程退出、许可文件不存在、二进制hash未变。

用户继续消息有较长间隔，期间应用保持运行且自然续查；晚到读取的快照不能当作第一次重启的即时对照，保留该失败断言并重新执行短对照。长时间观察到Reasonix变为Auth unknown：只读验证回执age=87081秒，超过既有86400秒有效期；钱包仍有效，不能以独立全局钱包冒充任意当前模型权限证明，也没有自动触发收费模型验证。其余查询失败仍按真实网络类型展示与恢复。

G5按当前Mac/现有账号范围完成，不新增跨平台、真实换号或账单级精度声明；incognito不能证明所有前端设置持久性。完整机器回执、失败、重试、清理与证据hash见 [G5验收](reliability-evidence/cli-gaps/g5-acceptance.json)。最终验收包关闭授权并退出；不自动推送/发布。
