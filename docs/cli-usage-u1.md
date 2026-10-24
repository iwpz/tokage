# U1：原生在线用量权限与旧查询路径退役

日期：2026-10-24。范围依据：[U0](cli-usage-u0.md)。本节点实现原生权限与撤销边界；不代表 U2 数据 / 身份契约、U3 各 CLI 新适配或 U6 真实桌面验收完成。

## 用户行为

设置 → CLI 管理顶部增加「允许在线查询用量」，默认关闭，附查询范围与边界说明。预算页旧「主动查询额度」开关移除。关闭时各 CLI 的手动额度按钮禁用；原生同样拒绝直接 IPC，不能通过前端绕过。

当前只开放既有 CLI 手动命令，不启动自动查询；界面明确说明自动调度将在共享服务接入后提供。U4 仍需完善「更新本地数据」、卡片状态与统一展示。正常模型请求、普通认证和模型目录获取不受该额度权限开关直接禁止，正常任务产生的本地用量继续保留。

## 权威存储与迁移

`tokage_native::online_usage::OnlineUsagePermission` 持有原生权威状态，启动 setup 时建立，早于前端加载及任何额度请求。记录为标准 app_config_dir 下独立的 `online-usage-consent.json`，不放入 SQLite 的 settings，也不进入前端 Settings Store。数据库备份采用 VACUUM INTO，通用 settings 导入、项目导出和数据库快照恢复不会复制 / 覆盖本文件。

缺失、损坏、格式不兼容或不可读均关闭；旧 `activeQuotaQuery=true` 不授予权限。前端迁移对旧、当前和未来设置版本都移除 activeQuotaQuery / allowOnlineUsageQuery，避免通用导入创建权限镜像。旧数据库中可能保留惰性的历史设置行，没有生产读取方，不把它映射到新授权。

显式开启先以临时文件写入并同步，再原子替换记录，持久化成功才允许查询。正常重启重读独立记录。关闭先在内存撤销并通知在途请求，再删除授权文件；文件不存在即默认关闭。若磁盘拒绝删除，返回“本进程已关闭、撤销未持久化”的错误，不能声称下次重启也已关闭；设置页显示错误，重试执行撤销。开启保存失败也保持内存关闭。该记录不是跨机器可同步授权；本节点不实现配置目录整体克隆或 OS 备份恢复的授权迁移。

权限读取 / 更新是正常产品 IPC，不接收普通设置布尔值作为查询授权。前端 onlineUsage Store 仅镜像原生状态，读取失败关闭，原生 generation 事件覆盖旧读响应，权限不参与 db.capture。

## 查询及撤销

`query_provider_quota` 在环境捕获、providers_or_scan 和代理准备之前取得原生 permit。所有生产专用 CLI 查询统一经 `quota::query_authorized`；原始 Cursor / Antigravity 实现及 slash 输出执行器改为模块私有，无生产调用方的通用任意 subcommand 执行器删除。测试文件及 cfg(test) 的现有 slash 包装不进入产品构建。

每次权限更新递增 generation，使旧 permit 永久失效；关闭后重新开启不能复用旧请求。watch 通知取消正在等待的查询 future，发布快照 / Reasonix 余额时，在与撤销共用的锁下再次校验权限及 generation。关闭前已完成的历史快照保留，关闭后不接受旧授权结果。

PTY 子进程加入 Drop 守卫：在取消、提前错误和正常结束时回收并等待子进程；Unix 仅在确认进程自身为组长时清理该组，避免杀到宿主进程组。Antigravity JSON 查询复用原沙箱 / 凭据环境，独立进程组及 Group 清理管理，取消不只遗弃 wait future。未新增 HTTP 查询、TLS 策略或凭据读取。

Reasonix 余额缓存写入从裸 PTY 路径移至 permit 保护的发布步骤。只有余额而没有 quota window 时可成功缓存余额，不伪造窗口；前端统一余额结果及错误展示仍属 U2/U3。

前端手动刷新先读取原生状态，在权限不可用 / 关闭 / 更新中不发 quota IPC；返回后核对权限及 generation。关闭操作在等待原生响应期间立即停止前端发布。新失败不再静默返回旧缓存冒充本次成功，已有本地快照不清空。

## 扫描与旧轮询清理

- Antigravity 普通 probe 不再执行 `/usage`；只保留模型目录与原有 Key 验证判断。Google 订阅无独立认证证据时标为 UNKNOWN_AUTH，保留账号类别与明确核验指引；离线模型目录不能让它 READY。后续 U3 决定独立认证 / 查询结果映射，不伪造通过。
- Reasonix detect_one 不再附加 `/status` 钱包查询，doctor 与既有显式 Key 验证边界保留。
- detect_all / detect_each / detect_one 的移除同时覆盖桌面启动 / 全量与单卡重扫、空缓存惰性扫描、API Key 验证前后扫描、runtime 启动 turn、正式 CLI providers list。当前 runtime / CLI 不加载桌面授权，不自动查询专门额度。
- 旧 scheduleQuotaPolling、活动监听与 activeQuotaQuery 订阅全部删除，禁止旧授权抢跑或继续触发软熔断。唯一旧调用方退役后，一并删除 tripProviderMelt 前端封装及 trip_provider_melt_cmd IPC；原生 health_sub 与存储熔断服务保留。U2/U4 接入适用身份 / 有效快照后再提供自动查询与额度消费。

## 验证及限制

机器可读结果、失败记录及核心源码指纹见 [U1 验证摘要](reliability-evidence/cli-usage/u1.json)。

离线测试覆盖：默认关闭、显式保存及重新打开、损坏 / 旧格式记录、持久化失败、关闭再开启后的旧 permit、在途取消、实际 PTY 子进程终止、扫描不执行 usage、导入不继承、原生读取失败、保存失败、等待关闭期间阻止发布、旧读取不能覆盖新撤销事件、关闭时无 quota IPC 及迟到回复丢弃。

结果：前端全量 66 文件 / 449 项通过；后续相关 3 文件 / 22 项再次通过。原生全量 337 通过 / 28 ignored；真实付费或本机账号用例未执行。`cargo check` 桌面 tokage 和独立 cli/Cargo.toml 均通过。tsc + Vite 生产构建、i18n 1775 keys、测试隔离、IPC 接线（135 commands / 0 无生产调用）和差异检查通过。Vite 保留现有大 chunk / 混合静态动态导入提示，不为该节点调整打包。

过程中保留以下失败：新前端事件测试最初依赖被自动清理的 mock.calls 而失败，改为在测试内保留实际订阅回调后通过；新 Rust 取消测试首次因临时 argv 生命周期编译失败，固定局部绑定后通过；首次把独立 CLI 当作 src-tauri workspace package 检查失败，改用 cli/Cargo.toml 后通过；旧轮询退役后 IPC 接线检查检出孤立的 trip_provider_melt_cmd，删除无调用的 IPC 后通过。没有删断言、排除失败测试或修改生产超时来通过。

没有运行真实 CLI 额度请求、模型、账号切换、真实桌面外部 UI 或跨平台验收。原生重新打开权限文件的测试不是完整桌面重启实测。设置页面的真实开启 / 关闭 / 重启及 8 CLI 全链路继续留给 U6；自动调度、去重 / 退避、身份与账号切换代际、统一快照、启用状态竞态和新 HTTP 来源留给 U2–U4。可靠性第十项保持暂停。
