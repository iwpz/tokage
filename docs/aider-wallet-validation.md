# Aider 官方 DeepSeek 钱包（G2a）

2026-10-25。安装版0.86.2。用户要求处理已知缺口后，Aider新增明确命名的官方DeepSeek全局连接钱包，不把它解释为当前任意项目/模型的账单。其他服务商没有通用余额接口，不伪造额度。

按安装版官方实现及文档，读取普通shell环境、全局OAuth dotenv、HOME/.env、明确的全局env-file，以及全局YAML的set-env/api-key覆盖；不会读取项目/会话配置或选择默认模型。相对env-file及dotenv内新的配置重定向拒绝猜测。字段解析严格，拒绝未知插值、控制字符、可疑自定义网关与无有效Key；不执行shell读取凭据。

固定官方余额GET复用受授权、可取消、禁止重定向的HTTPS传输。APIKey仅在原生内存/服务商必要认证中使用，来源按可执行路径/HOME/当前Key绑定；Key变化使旧来源失效。CNY/USD以及total/granted/topped-up分开保留原始精度，不生成假百分比、Token或美元等效值。钱包来源不会熔断或恢复任意Aider模型任务。

同时修正Aider子进程环境遗漏已检测供应商Key的问题：仅增加AIDER_KEYS声明的模型供应商变量；其他CLI、未知密钥仍被裁剪，模型shell包装继续删除凭据变量。复用已有serde_yaml依赖版本，只增加原生crate依赖边；无需新解析器/副本凭据。

真实非推理管理诊断取得3个币种数量，没有启动Aider聊天或调用模型；证据日志tokage-g2-aider-live.log。本机Key来源有效，原生诊断不转记为桌面通过。共享额度相关62项、环境隔离10项、前端482项通过，类型检查通过。提交版外部桌面与跨卡/重启由G5继续验证。

参考：[官方dotenv](https://aider.chat/docs/config/dotenv.html)、[全局YAML](https://aider.chat/docs/config/aider_conf.html)，以及安装版main.py的load_dotenv_files、set-env、api-key顺序。未知来源明确拒绝，不扩大为所有服务商/项目配置均支持。
