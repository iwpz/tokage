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

状态保存到 `runs/RUN_ID/run.json`，原生事件保存到 `events.jsonl`，实际用量保留原生 Done 事件；未知费用不写零。模型事件实时写 stderr JSONL，最终命令结果写 stdout。`run events --id RUN_ID` 读取已记录事件。

## 执行、审查和交付

```sh
tokage --data-dir /outside/repo/runs run execute --id RUN_ID
# 独立门禁和只读审查通过后停在 AWAITING_MERGE
tokage --data-dir /outside/repo/runs run show --id RUN_ID
tokage --data-dir /outside/repo/runs run merge --id RUN_ID --source ACCEPTED_SHA --target ORIGINAL_TARGET_SHA --confirm
```

首版串行使用共享 DAG 调度器。每个任务从当前集成提交建立独立 worktree，按完整 brief 和契约执行；原生结构化完成、范围检查、真实验收命令及固定任务报告均须通过。模型不得提交或改变 Git 历史。契约偏差、缺失报告、失败门禁和越界文件均阻断；不自动修复或撤销失败现场。验收命令同时受共享命令红线和门禁规则约束，显式确认也不能允许被禁止的命令。CLI 暂不读取桌面自定义包/规则；使用同一内置包。

任务提交由共享 `commit_verified` 服务完成，Git 配置保持不变。全部任务集成后重新跑全部去重验收命令，并用同一指定模型的只读角色审查精确候选；只接受严格 PASS 且无 findings 的 JSON，候选发生变化则失效。审查通过也不自动修改主 checkout。

合并必须确认完整源 SHA 和原目标 SHA；共享服务检查源快照、目标分支/HEAD、干净状态及已有 Git 操作。交付意图先落盘；若进程在 Git 合并后、状态落盘前中断，可重复相同 merge 命令，通过共享精确父提交/树校验对账。不会 push。失败工作树和事件保留，首版不自动清理工作树。
