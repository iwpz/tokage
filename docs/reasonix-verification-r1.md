# Reasonix R1：验证证明到期与模型范围整改

日期：2026-10-26。状态：开发子节点 CLI-MGMT-R1a 已完成代码与离线验证；新的正常桌面验证及自然跨越 24 小时观察未完成。本记录不修改 G5 历史验收结论。

## 原因和影响

`REASONIX_VERIFICATION_MAX_AGE = 24 * 60 * 60` 是 Tokage 自设的认证证据有效期，不是厂商声明的 Key 到期时间。此前布尔读取将不存在、损坏、配置变化、到期等原因统一压成 false，卡片仅显示 Auth unknown。配置和密钥指纹已绑定，但旧证明没有记录实际验证模型及 CLI 可执行文件路径；默认模型验证成功会使整个 Provider Ready。

前端 `runnable` / `selectableGroups` 决定手动选择和自动分配的可用模型；原生 `run_turn` 的 executable 检查决定任务是否启动。旧原生路径仅接受缓存的 Provider 状态，没有在派发前重新读取回执有效期。独立 DeepSeek 钱包绑定的是全局连接，其成功不能证明任意所选模型有权限。

## 当前 CLI 的免推理复核能力

本机安装版为 Reasonix 2.31.0，npm 包声明源码提交 `44e9492be4088d51c742bb7ca305719fe7f2c230`。本次只执行了本机 `--help` 和 `doctor --json`，没有执行 `run`、登录/注销、充值或模型调用。脱敏诊断显示一个已配置 DeepSeek 来源、默认 deepseek-flash 和两个模型目录项；这只证明配置存在。

核对该确切提交的官方源码：

- [CLI 命令分发](https://github.com/esengine/DeepSeek-Reasonix/blob/44e9492be4088d51c742bb7ca305719fe7f2c230/internal/frontend/cli/cli.go)：login/whoami 用于 Reasonix 自身账号；没有确认可用于所选模型权限续验的管理命令。
- [doctor 命令](https://github.com/esengine/DeepSeek-Reasonix/blob/44e9492be4088d51c742bb7ca305719fe7f2c230/internal/frontend/cli/doctor.go) 与 [providerReports](https://github.com/esengine/DeepSeek-Reasonix/blob/44e9492be4088d51c742bb7ca305719fe7f2c230/internal/frontend/doctor/report.go)：key_present 来自本地 Configured()，不是远程模型授权回执。
- [dotenv 来源](https://github.com/esengine/DeepSeek-Reasonix/blob/44e9492be4088d51c742bb7ca305719fe7f2c230/internal/contract/config/dotenv.go)：模型凭据从全局来源加载；工作区 .env 不等同于模型凭据来源。

因此本次不增加自动付费续验，也不拿模型列表或钱包响应恢复模型 Ready。结论限定当前版本已核实入口，不承诺厂商未来没有此类接口。

## 整改行为

- 回执增加模型、CLI 规范路径和类型化状态。24 小时到期保留原验证/到期时间，明确区分 Required、Valid、Expired、SourceChanged、Rejected。
- 管理卡片展示证明原因、对应模型及时间；钱包仍独立展示。用户明确选择模型后点击“验证所选模型（消耗 token）”才会发起一次模型请求，扫描/查询/计时器均不代点。
- 单份回执只授权当前明确验证的模型；验证另一模型会替换它。模型选择与下一派发检查都不能把一个模型的成功推广到全部模型。
- 到期计时器只更新前端证明状态，不联网、不刷新时间、不清除钱包。即使前端缓存未更新，原生派发前仍重新读取回执、核对来源及模型。
- 显式认证拒绝保留为来源绑定的 Rejected，后续扫描/重启不能恢复 Ready；网络或限流导致的验证失败不续期，也不抹掉仍有效的旧证明。
- 验证回复若晚于自定义 CLI 来源变化，前端不采纳。配置、凭据、endpoint 或 CLI 路径变化使证明失效；Reasonix 配置回退读取遵循同一个 REASONIX_HOME。
- 旧 v1 回执缺少模型/路径证明，显示“来源已改变或旧证明缺少模型绑定”，需要用户明确重验一次；不自动迁移为更强的认证证明。

证明仅涵盖当前扫描的全局配置来源；项目配置优先级、其他认证模式及跨平台的完整支持核对继续归属 R4，本次不新增其支持承诺。

## 验证与限制

离线回归包括：恰好 24 小时到期及前一秒、进程状态重建、未来时间/损坏/旧 schema、凭据/endpoint/默认模型/CLI 路径变化、认证拒绝跨扫描保留、网络/429 失败不续期、非推理扫描不启动 run、显式 argv 模型绑定、缓存 Ready/错误模型不启动子进程、前端到期不动钱包、手动选择/点击及来源变化后的迟到回复。

首轮前端全量发现两处旧模型契约 fixture 缺少新证据字段，已按新契约补齐；首轮原生全量发现新测试在 CLI 文件创建前签署路径，macOS 临时目录规范路径随文件创建改变，已修正测试准备顺序。没有放宽生产路径校验。

最终验证：前端全量 488 passed（75 个文件）；原生全量 415 passed / 33 ignored；类型检查、i18n（1870 键）、测试隔离检查均通过。生产前端构建及桌面 crate check 已通过；构建原有的大 chunk / 动态导入提示保留。最初类型检查因本机缺失锁文件中的 yaml 依赖失败，已用 `npm ci --ignore-scripts` 补齐，没有修改依赖清单。

尚未完成：从提交版独立构建后的正常设置 UI 验证、用户显式选择模型后的真实验证与恢复，以及自然跨越 24 小时的长期观察。离线时钟边界、组件 DOM 点击、重建 ShellEnv 均不代替上述实机证据。未发起长期自动监控，未修改个人 CLI 配置/凭据，未运行付费模型验证。
