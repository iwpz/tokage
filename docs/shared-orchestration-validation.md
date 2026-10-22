# sh4 验证记录

本记录独立于可靠性 20 项样本；10/20 不因本迁移测试自动恢复。离线 Provider fixture、Rust 服务测试和前端 transport 单测均不计真实模型 / 正常 UI 验收。

## sh4a：共享契约和确定性（2026-10-22）

`tests/fixtures/shared-runtime-v1.json` 是独立预期：12 种 App 命令及对应 Rust / CLI wire 格式、完整有效配置、普通计划和四步状态 / 事件预期。前端测试调用真实 runtimeSubmit codec，Rust 测试解析同一 wire，公开 CLI `run submit` 在独立数据目录执行同一输入。三个入口不各写一套决策来互相证明正确。

重复提交返回相同回执且不产生额外事件；不同 commandId 的陈旧版本拒绝且不改状态；模型自带分配被冻结的用户锁替代；取消撤销批准；只读分页和完整重放一致，无 turn / Git 副作用。语义比较不包含随机 ID、时间戳、绝对临时目录或事件批次。

另两次真实原生 fixture 执行各自产生单个执行 turn、独立最终审查、通过且清理的 gate、任务提交及集成证据；任务 DONE、候选 AwaitingMerge、目标文件不变。实际集成 Git blob 为期望内容，模型调用各两次，未知费用保留 null。断言具体预期，不用两个结果相等代替正确性。

验证：runtime contract_parity 3 项、公开 CLI shared_contract 1 项、App runtimeContract 12 项、typecheck、测试隔离和差异空白检查通过。首次 CLI 测试错误地把 serde 省略的空 turns 字段当必有对象而失败；修正测试以接受契约的省略空集合表示，未改产品或业务断言。未调用真实模型。

## sh4b：真实进程离线故障矩阵（2026-10-22）

新增公开 CLI 场景使用 tests/ 中的 vendor JSONL fixture，经正式 Provider 路径、真实进程、共享 runtime、SQLite、gate 和 Git 执行，不添加产品 test mode：

- owner 强杀：只杀测试自己的 CLI owner，原生子进程继续等待。resume 在活跃进程下拒绝、retry 拒绝；释放 fixture 后只对账为 Failed / cleanupConfirmed，丢失结构化完成不被接受，调用次数仍为 1，目标不变。
- gate 中断：观察真实 gate children.json 登记后请求正常 cancel，最终进程清理确认；无 PASS、任务接受、提交或集成，目标不变。
- 审批 / 取消竞争：真实 Provider 发出不可静态解析命令的拒绝事件，两个 CLI 进程用同一 expectedRevision 竞争 Deny / CancelRun。恰一个接受事件，无续发 turn；重复同 commandId 保持幂等，新 commandId 的陈旧审批拒绝。

已有真实原生 / SQLite 故障测试继续覆盖：重复命令和事件分页（contract_parity、repository、shared_contract），连续流取消（workflow），DB / transcript 写失败（repository、turn_owner），任务提交及集成回执丢失（acceptance），三种交付及交付回执丢失（delivery、公开 CLI workflow）。Git 对账核实实际提交 / 父提交 / 树及调用次数，缺证阻断；不是仅验证返回枚举。

开发失败记录：owner 恢复测试最初把“已确认清理但仍 Blocked”的 CLI 非零误作未恢复，改为独立检查真实 Failed / cleanupConfirmed，保留 Blocked 断言。审批 fixture 最初发出静态可解析命令，被 Full Auto 正常策略允许；改用确实无法静态解析的 vendor 拒绝事件，不修改产品权限策略。测试也修正了等待审批及通用 submit 的退出码假设：已持久化的业务等待 / Deny 回执允许成功返回，不能用退出码代替状态；同 ID 的获胜 Deny 必须允许幂等重放，新 ID 的陈旧审批仍须拒绝。所有失败均为离线尝试，未调用真实模型、未放宽期限或业务断言。

sh4b 最终验证：runtime 全量 49 项通过，CLI 全量 20 项（commands 2 + shared_contract 1 + workflow 17）通过；前端全量 57 文件 / 394 项、typecheck、i18n、测试隔离和差异检查通过。测试集合串行执行，每集最多两个线程 / worker。完整首次失败、针对性重跑和最终日志归档于外部证据目录 `offline/`，附 results.json 和日志 SHA-256；不是付费或正常 UI 证据。
