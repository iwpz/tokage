# Codex CLI 在线额度（G1）

2026-10-25。安装版 `codex-cli 0.160.0`。原先只有解析器而无生产传输；本节点接入固定管理协议，仍不创建会话或模型 turn。

## 身份与调用

当前 CLI 自管 ChatGPT file/default 来源使用 `auth_mode=chatgpt`、当前 account_id、配置根及 executable 绑定。身份不包含易旋转的 access/refresh token，因此普通刷新不当作换号；workspace/account_id 或配置根/路径改变则失效。显式 keyring/ephemeral、自定义 profile/provider/endpoint 不借用旧文件身份。API-key-only不视为ChatGPT订阅。

在空临时目录启动固定 `app-server -c mcp_servers={} --listen stdio://`；正常 HOME、环境和代理，公开的空MCP覆盖避免加载扩展。顺序固定 initialize → initialized → account/read(refreshToken=false) → account/rateLimits/read → account/read(refreshToken=false)。前后账号一致，额度回执accountId必须匹配当前CLI来源；再核对来源未改变。无login/logout、thread/turn、reset-credit或通知邮件方法；未知server request立即停止。

连接20秒截止、单帧64KiB、通知上限、权限撤销及整个进程组清理。撤销后不启动管理进程。RPC错误保留认证/限流/未知方法分类，不存服务商原始错误文本。账户刷新仍由官方CLI管理，Tokage不复制Token或实现OAuth客户端。

## 沙箱诊断

原严格管理沙箱在account/read返回-32603，无法完成认证读取。直接正常登录环境及普通shell包装均成功，复现定位为macOS Keychain访问：仅给固定账户管理进程正常钥匙串访问后成功，其他目录deny继续存在。实现只在完全匹配上述固定argv时启用该管理画像；Codex普通模型执行画像不变。回归同时验证前后权限差异及其他CLI凭据目录仍受限，没有取消沙箱、改系统授权或修改用户配置。

## 数据展示

多个limitId桶各自保存主/次窗口及真实reset，不将其合并为全账号限额。remaining credits保持十进制字符串；不限量是独立文本状态，不虚构0余额/百分比/分母。修正共用CliQuota忽略remaining字段的问题，并显示桶名称，避免Codex/Spark多个主窗口混淆。

## 当前证据

独立只读管理诊断3.462秒成功；回执账号与CLI当前account_id匹配、前后账号与auth.json均未变化。产品沙箱修正后真实原生查询3464ms，2窗口/1数量，无thread/turn。失败及环境比较保留在外部tokage-cli-gaps-awjulw9x，不将直接诊断追认为桌面验收。

离线覆盖同账号刷新、切账号/工作区、错配回执、401/429、未知方法、服务端刷新请求拒绝、超大帧、在途撤销并结束进程。原生全量401通过/30忽略，前端481通过；类型和i18n通过。源码提交后的正常桌面查询、重启与其他CLI并发由G5继续执行。

官方协议：[App Server](https://learn.chatgpt.com/docs/app-server)。实时查询与本地rollout历史分开，不使用当前Codex桌面应用账号额度代替CLI账号。自动刷新是官方管理功能；此观察不保证所有未来CLI版本无其他缓存写入。
