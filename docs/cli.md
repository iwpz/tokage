# Tokage CLI

独立无头入口，不启动桌面应用或 WebView。当前按 [开发计划](cli-plan.md) 分节点交付；未标记完成的功能不能作为真实模型验收入口。

## 构建和安装

```sh
cargo build --manifest-path cli/Cargo.toml --locked --jobs 2
cargo install --path cli --locked
```

独立 Cargo workspace 的二进制名是 `tokage`，不会覆盖桌面 workspace 的构建产物。仓库内可运行 `cli/target/debug/tokage`；安装会写用户 Cargo bin，开发验证不会自动安装。

```sh
tokage --help
tokage --json providers list
tokage --json repo inspect --repo /path/to/repo
tokage --json plan validate --file /path/to/plan.md
```

这些命令不发起模型推理；Provider 探测可能执行版本、认证状态或模型目录查询。CLI 从调用终端继承普通环境，不运行额外登录 shell、不重写环境变量，不输出账号详情或环境值。请通过正常 CLI 登录配置账号。

普通结果是 JSON；`--json` 还将错误转换为 stderr JSON。失败退出码为 1，成功为 0。未知参数、重复单值参数及缺少参数在副作用前拒绝。`--data-dir` 为后续运行命令选择独立状态空间，不修改桌面设置。

## 登记和授权

```sh
tokage --data-dir /outside/repo/runs run create --repo /path/to/repo --requirement-file /path/to/requirement.txt --provider CODEX_CLI --model gpt-6-luna
tokage --data-dir /outside/repo/runs run plan --id RUN_ID --trust-workspace
# 或由用户导入普通计划，仍需相同校验及授权
tokage --data-dir /outside/repo/runs run import-plan --id RUN_ID --file /path/to/plan.md
tokage --data-dir /outside/repo/runs run show --id RUN_ID
tokage --data-dir /outside/repo/runs run approve --id RUN_ID --plan-sha SHA_FROM_SHOW --allow-writes --trust-workspace
```

`run plan` 会调用指定模型，可能计费。`create` 仅登记，不调用模型；要求仓库根目录、干净的分支 checkout，数据目录位于仓库之外。模型 ID 必填，不自动换模型；`--provider-path PATH` 可指定正常安装的 CLI 路径。`--co-author` 在登记时选择内置 Tokage 协作者 trailer，默认关闭，不改 Git 配置。

所有模型角色使用用户指定的同一 Provider/model。首版要求适配器同时支持工作区沙箱和只读角色；不支持的平台/能力直接拒绝。CLI 为独立产品入口，没有继承桌面设置，也没有通过改数据库伪造 UI 操作。

计划导入和模型输出里的 `confirmedCommands` 不构成 CLI 授权。非允许列表命令须在 `approve` 逐项增加 `--confirm-command 'exact command'`；危险 shell 管道/重定向/拼接仍被共享校验器拒绝。运行授权摘要绑定目标分支/提交、需求、模型、路径和完整规范化计划。

状态、命令回执、事件及 transcript 统一保存到 `runtime-v1/tokage.db`，工作树及原生进程记录位于独立的 `runtime-v1.native/`。CLI 不再写 `run.json`。模型事件实时从持久 transcript 写 stderr JSONL，最终命令结果写 stdout；未知费用不写零。`run events --id RUN_ID --after 0 --limit 200` 按游标读取运行事件；`run transcript --id RUN_ID --turn TURN_ID --after 0 --limit 200` 读取原生流。查询不占用执行 owner，可在另一个终端观察活动运行。JSON 同时保留 runtime 快照及原有 id/status 等只读展示字段。

## 执行、审查和交付

```sh
tokage --data-dir /outside/repo/runs run execute --id RUN_ID
# 独立门禁和只读审查通过后停在 AWAITING_MERGE
tokage --data-dir /outside/repo/runs run show --id RUN_ID
tokage --data-dir /outside/repo/runs run merge --id RUN_ID --source ACCEPTED_SHA --target ORIGINAL_TARGET_SHA --confirm
```

首版串行使用共享 DAG 调度器。每个任务从当前集成提交建立独立 worktree，按完整 brief 和契约执行；原生结构化完成、范围检查、真实验收命令及固定任务报告均须通过。模型不得提交或改变 Git 历史。契约偏差、缺失报告、失败门禁和越界文件均阻断；不自动修复或撤销失败现场。验收命令同时受共享命令红线和门禁规则约束，显式确认也不能允许被禁止的命令。CLI 默认不读取桌面配置；普通 `run update-config --id RUN_ID --file config.json` 可显式更新可编辑运行的完整有效配置，旧授权立即失效。

任务提交由共享 `commit_verified` 服务完成，Git 配置保持不变。全部任务集成后重新跑全部去重验收命令，并用同一指定模型的只读角色审查精确候选；只接受严格 PASS 且无 findings 的 JSON，候选发生变化则失效。审查通过也不自动修改主 checkout。

合并必须确认完整源 SHA 和原目标 SHA；共享服务检查源快照、目标分支/HEAD、干净状态及已有 Git 操作。交付意图先落盘；若进程在 Git 合并后、状态落盘前中断，须执行 `run resume --id RUN_ID`，通过共享精确父提交/树校验对账，不重发模型或重复合并。不会 push。失败工作树和事件保留，首版不自动清理工作树。

## 取消和显式重试

```sh
tokage --data-dir /outside/repo/runs run cancel --id RUN_ID
tokage --data-dir /outside/repo/runs run show --id RUN_ID
tokage --data-dir /outside/repo/runs run retry --id RUN_ID --confirm
# retry 仅重新授权状态，不直接发送模型请求
tokage --data-dir /outside/repo/runs run execute --id RUN_ID
```

取消请求不代表进程已停止，须观察运行进入 CANCELLED。Ctrl-C 同样请求原生进程取消；工作树保留。活动运行持有 OS 文件锁，禁止并发执行或重试。强杀后没有锁也不等于原生子进程已退出，retry 会检查进程记录，仍活跃或损坏则拒绝，不猜测 PID 并杀进程。

重试保留失败事件、历史尝试和已验收任务，在新任务工作树重跑未完成部分。执行中断后由服务核实完整计划确认、已确认的原生清理和 Git 证据，再准备为 APPROVED；规划失败可重新 `plan`，不伪造执行授权。集成提交或内容改变时停止，要求人工对账。审查失败后重试会重新执行最终门禁和审查，不把失败改写成成功。任务提交、集成及交付回执丢失可通过 `resume` 精确对账；有未知原生进程或陈旧证据时阻断。

## 当前验证范围

命令进程测试使用 `cli/tests/fixtures/` 中的离线 Provider 协议 fixture，经公开 `--provider-path` 传入；产品没有 fixture 标识、测试模式、样本分支或模拟成功选项。测试只能证明命令和共享门禁组合的行为，不能证明真实模型能力、真实账号登录、实际费用或 OS 沙箱阻断。完整真实模型验收尚未执行。

独立状态目录和 WebView 无关，不需要辅助功能权限；它也不隔离模型 CLI 的 HOME、账号、缓存或日志。发布前仍需真实 CLI 测试和更广泛的平台验证。首版不提供并行调度、交互式逐命令审批、桌面自定义设置同步或自动修复/清理；只支持显式授权的工作树执行。

## 共享 runtime 命令与历史迁移（sh3a）

CLI 默认明确记录串行、manual 修复、关闭上下文优化、无费用上限、允许未知费用继续；没有继承 App 设置。完整有效配置可从 show 查看，更新只能经过服务状态和配置校验。用户指定模型按锁定身份传递；串行和并行均由同一个服务调度。

- `run resolve-approval --id ID --turn TURN --approval APPROVAL --digest DIGEST --decision allow-once|allow-session|deny` 提交有范围的审批；不支持的适配器或授权范围保持人工阻断。
- `run review --id ID` 显式重做最终门禁和只读审查；不允许直接把任务设为 DONE。
- `run export-patch --id ID --digest DELIVERY_DIGEST --output /outside/repo/patches --confirm` 和 `run keep-branch --id ID --digest DELIVERY_DIGEST --confirm` 使用同一接受凭据交付。
- `run submit --file request.json` 接受版本化 Request（schemaVersion、commandId、runId、expectedRevision、command），用于需要精确幂等和 revision fencing 的脚本；相同 commandId 不重复副作用。它不接受任意状态或 SQL。

旧 `runs/ID/run.json` 可用 `run legacy-show --id ID` 只读查看，不能执行、重试或合并。先独立备份，再 `run import-history --id ID --file ORIGINAL --backup BACKUP` 显式归档；相同内容幂等，冲突内容拒绝，旧完成标记不会转换为共享服务的接受凭据。新任务从 create 和普通计划导入开始，必须重新授权。取消意图仅表示请求，收到 completionConfirmed=false 后须持续观察终态；owner 不在时先 resume 对账，不凭取消文件推断进程已退出。

`run inspect-task --id ID --task T1` 在运行停止后返回真实接手工作树及源/计划摘要。人工修改后再次 inspect，再以 `run takeover --id ID --task T1 --digest SOURCE_DIGEST --plan-sha PLAN_DIGEST --report-file REPORT --confirm` 请求重新验收。报告须由用户提供完整任务报告章节；通过 gate/提交/集成后仍需独立最终审查。此路径明确记录人工接手，不生成模型成功事件。补充任务和预算调整可经 `run submit` 的 SupplementTask/AmendRun 命令执行；completed 任务与安全边界保持不可变，修改后必须重新批准。
