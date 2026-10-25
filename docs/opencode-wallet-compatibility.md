# OpenCode 钱包兼容（G2b）

2026-10-25。默认依旧关闭在线查询，钱包与当前项目/模型账单独立。

先执行公开`--version`，只接受已识别的1.x/2.x；未知版本停止。V1绝不接收V2的api/provider.get/auth export命令，避免旧版将未知输入解释为模型prompt。V2保留显式内置DeepSeek integration与唯一活动凭据；仅账号列表确实为空时，才允许官方环境Key来源，不覆盖存储账号，不借用多活动账号。

V1只读官方全局auth.json与全局JSON/JSONC；支持XDG、公开自定义配置路径/目录、inline config和OPENCODE_AUTH_CONTENT。来源顺序为环境Key→存储API Key→显式全局options.apiKey；配置顺序为全局→自定义文件→自定义目录→inline。不读取任意项目或默认模型，不执行插件、MCP或hooks。JSONC只允许标准注释/尾逗号，拒绝松散属性名且限制嵌套深度；明确插值仅读取已可访问的声明环境变量，不执行文件/命令插值。

自定义网关/头、未知SDK/npm或options、远程组织认证、插件、企业managed配置/MDM保留为无法安全归属，拒绝猜测或绕过。此限制不是通用账号管理功能。钱包按实际Key/来源/路径绑定；Key改变不复用旧身份。调用相同固定DeepSeek余额API，原始币种和数量保真。

OpenCode总管理预算120秒覆盖前后各阶段版本/提供商/凭据检查与20秒HTTP请求，正常成功并不等待该上限。自身DeepSeek环境Key只向其CLI和固定官方余额目标提供；其他未知密钥仍裁剪。钱包提示改为“该CLI所配置的连接”，不错误声称环境来源必定保存在某个文件。

当前安装2.1.4真实诊断：首次来源读取返回Network，保留失败；相同环境/代码路径重试成功，查询584ms、3个币种数量，前后来源相同，没有模型调用。临时增加的测试诊断日志已移除。不能将一次恢复声称为上游永无瞬时故障。现有共享服务保留历史并退避重试。

V1公开契约与配置/优先级、深度边界、V1只收到version、V2环境fallback/存储优先/多活动拒绝/未来版本停止等10项通过；环境隔离10项通过；共享额度66项通过（追加配置层测试前），前端钱包5项、i18n、桌面crate检查通过。V1没有当前已认证实机环境，不将fixture当成实机；G5继续验证提交版桌面。新JSONC依赖固定兼容0.26.x/edition2021，不升级Tauri或改CLI配置。

参考：[官方提供商与认证位置](https://opencode.ai/docs/providers/)、[配置优先级](https://opencode.ai/docs/config/)、[V1.19认证实现](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/opencode/src/auth/index.ts)、[V1.19提供商优先级](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/opencode/src/provider/provider.ts)。
