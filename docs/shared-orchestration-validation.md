# sh4 验证记录

本记录独立于可靠性 20 项样本；10/20 不因本迁移测试自动恢复。离线 Provider fixture、Rust 服务测试和前端 transport 单测均不计真实模型 / 正常 UI 验收。

## sh4a：共享契约和确定性（2026-10-22）

`tests/fixtures/shared-runtime-v1.json` 是独立预期：12 种 App 命令及对应 Rust / CLI wire 格式、完整有效配置、普通计划和四步状态 / 事件预期。前端测试调用真实 runtimeSubmit codec，Rust 测试解析同一 wire，公开 CLI `run submit` 在独立数据目录执行同一输入。三个入口不各写一套决策来互相证明正确。

重复提交返回相同回执且不产生额外事件；不同 commandId 的陈旧版本拒绝且不改状态；模型自带分配被冻结的用户锁替代；取消撤销批准；只读分页和完整重放一致，无 turn / Git 副作用。语义比较不包含随机 ID、时间戳、绝对临时目录或事件批次。

另两次真实原生 fixture 执行各自产生单个执行 turn、独立最终审查、通过且清理的 gate、任务提交及集成证据；任务 DONE、候选 AwaitingMerge、目标文件不变。实际集成 Git blob 为期望内容，模型调用各两次，未知费用保留 null。断言具体预期，不用两个结果相等代替正确性。

验证：runtime contract_parity 3 项、公开 CLI shared_contract 1 项、App runtimeContract 12 项、typecheck、测试隔离和差异空白检查通过。首次 CLI 测试错误地把 serde 省略的空 turns 字段当必有对象而失败；修正测试以接受契约的省略空集合表示，未改产品或业务断言。未调用真实模型。
