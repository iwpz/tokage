# 任务简报与任务报告模板

返回 [05 · 上下文生命周期引擎](README.md)

## 1. 任务简报

固定执行指令逐字不变，放在最前；任务相关内容在后。

```markdown
# Tokage Task Brief v1

## 执行规则
你是多代理流水线中的一名执行者。其他代理在并行完成其他任务，你看不到它们的上下文。
1. 严格遵守「契约」中的接口、类型与约定；需要偏离时，不要自行修改契约，在报告中说明。
2. 只修改「写入范围」内的文件。
3. 完成后运行「验收命令」自查。
4. 最后输出「任务报告」，格式见末尾，不超过 300 Token。

## 任务
- 编号：{{task.id}}  领域：{{task.domain}}
- 标题：{{task.title}}
- 目标：{{task.goal}}
- 交付物：{{task.deliverables}}
- 写入范围：{{task.write_scope}}
- 验收命令：{{task.acceptance.commands}}
- 验收标准：{{task.acceptance.criteria}}

## 契约
{{#each contracts}}
- {{id}} {{title}}：{{path}}
{{#if inline}}
{{content}}
{{/if}}
{{/each}}

## 上游结果
{{#each upstream}}
### {{task_id}} {{title}}
{{report}}
改动文件：{{changed_files}}
{{/each}}

## 失败证据
{{failure_evidence}}   <!-- 仅修复改派时出现 -->

## 任务报告格式
### 完成内容
### 偏离契约之处（无则写「无」）
### 给下游的提示
### 验收标准自查
```

## 2. 任务报告

执行模型输出四段内容后，Tokage 追加本地抽取的部分，最终存入 `task_reports`：

```markdown
## T1 用户 API · 完成
模型：{{model}}  执行窗口：{{attempt_id}}  首次通过：是

### 完成内容
…（执行模型输出）

### 偏离契约之处
无

### 给下游的提示
…

### 验收标准自查
- C1 中的状态码全部覆盖：是

### 改动（本地）
src/server/users/routes.rs   +120 −0
src/server/users/repo.rs     +64  −0

### 验收关卡（本地）
cargo test -p server users   通过（12 passed）
```

传给下游任务时只保留「完成内容」「偏离契约之处」「给下游的提示」与改动文件列表。

## 3. 进展报告（FRESH 动作）

窗口内规划选择 `FRESH` 时，先在旧窗口中请求一次进展报告，格式与任务报告相同，另加「下一步」一段，作为新窗口简报中「上游结果」位置的内容。旧窗口缓存热时，这一轮成本很低；冷时改为本地抽取（改动文件 + 最近错误 + 用户消息摘录）。
