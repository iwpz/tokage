# U4-002：Kiro 自动额度

2026-10-24；接入现有统一授权、身份绑定、缓存、自动刷新和失败退避。

## 行为

- 固定调用 `kiro-cli chat --agent-engine v1 --legacy-ui --model auto --agent <临时专用配置> --no-interactive /usage`。普通问题、恢复会话、工作区信任、工具自动批准均不属于此查询。
- 使用新建临时工作目录，查询配置关闭工具、钩子、MCP、Powers、资源与默认资源继承。不改用户全局设置或 HOME，不复制凭据。使用既有管理沙箱，禁止启动浏览器授权。
- 本机 2.34.0 legacy 管理面板写入 stderr。读取 stdout/stderr 各最多 64KiB，校验退出码和完整额度结构；90 秒进程组超时，上层 Kiro 请求预算为 100 秒。其他 CLI 保持 30 秒。
- 保留 Estimated、套餐、原始十进制 used/limit credits、CLI 百分比和日期。日期没有时区，不伪造精确重置时间；不换算 token、USD，不把估算计划窗口当作 account 作用域用于全局熔断。
- 当前身份适配只接受单一 CLI-owned social token 来源，且有 Google/GitHub provider、profile ARN 和 refresh grant。只读 SQLite；原始认证数据仅在原生内存，持久化及前端只有带安装盐的指纹。API Key 覆盖、多认证命名空间或未知存储结构拒绝绑定。grant 改变会保守失效缓存，查询前后再次核对来源。
- 本机 macOS 2.34.0 为目标实测环境。Builder ID/IDC、其他版本与平台未完成真实验证，不宣称覆盖。

## 诊断记录

- `--v2 --no-interactive /usage`：35 秒、55 秒均未返回；已终止查询进程组。
- `--agent-engine v2 --legacy-ui`：CLI 明确报告冲突，未调用模型。
- `--agent-engine v1 --legacy-ui`：普通本机 CLI 返回 exit 0 和真实 estimated credits 面板；加入 `--model auto` 后仍可返回。此为原生命令诊断，不是桌面验收。
- 首次生产沙箱查询 55 秒超时。CLI 日志有模型目录 API dispatch timeout；保留失败，不以普通命令成功替代沙箱结果。随后给 Kiro 单独提高有界预算并重试，结果见下方验证记录。

## 自动验证

- 原生 lib 全量：383 passed，29 ignored；包含新的日期/credits/无效输出/身份轮换/认证歧义测试。
- 前端相关：6 passed；包含 Kiro estimated/date-only/精确 credits 和历史快照展示，以及自动轮询/退避策略。
- TypeScript、i18n、test-isolation 通过。
- 本节点尚无外部克隆正常桌面 UI 验收；原生诊断不能替代真实桌面自动刷新验收。

## 官方参考

- https://kiro.dev/docs/reference/slash-commands/ ：`/usage` 显示 billing/credits，非交互运行输出完整明细。
- https://kiro.dev/docs/custom-agents/configuration-reference/ ：MCP 配置及默认资源继承范围。

## 最终原生诊断

生产调用链（正常 shell 环境、现有登录、管理沙箱、专用临时配置）重试通过：1 个 scoped window、1 个 credits quantity，查询前后身份一致，耗时 85,060ms。未输出凭据或真实额度数值到验收日志。90 秒限制用于容纳本机观测到的启动网络延迟；这不是快速查询的性能承诺，超时仍按现有失败退避展示。

日志：`/tmp/tokage-kiro-native-diagnostic-r2.log`；首次失败保留于 `/tmp/tokage-kiro-native-diagnostic.log`。普通命令与 V2 失败诊断脚本/输出在 `/tmp/tokage-kiro-usage-*`。仅声明免模型管理命令诊断通过，不将其提升为完整桌面验收。
