# 计划格式与规划指令模板

返回 [02 · 规划引擎](README.md)

## 1. 规划输出格式

规划模型的输出必须是一个 Markdown 文档，包含以下四个一级段落，顺序固定。任务图使用 YAML 代码块，便于本地解析。

````markdown
# Spec
## 目标
一到三句话。

## 范围
- 包含：…
- 不包含：…

## 关键决策
- 决策：理由（每条一行）

# Contracts
## C1 用户 API
```ts
// 只写签名与类型，不写实现
type User = { id: string; email: string; createdAt: string };
// GET /api/users/:id -> 200 User | 404
// POST /api/users  body: { email: string } -> 201 User | 409
```

## C2 目录约定
- 前端 API 客户端放在 web/src/api/，每个资源一个文件

# Tasks
```yaml
- id: T1
  domain: backend-api
  title: 用户 API
  goal: 按 C1 实现用户查询与创建接口
  depends_on: []
  contracts: [C1]
  write_scope: ["src/server/users/**"]
  deliverables: ["GET/POST 路由", "数据访问层"]
  acceptance:
    commands: ["cargo test -p server users"]
    criteria: ["C1 中的状态码全部覆盖"]
  size: M
  difficulty: 2

- id: T2
  domain: frontend-ui
  title: 用户资料页
  goal: 按 C1 的 User 类型实现资料页与创建表单
  depends_on: []
  contracts: [C1, C2]
  write_scope: ["web/src/pages/users/**"]
  deliverables: ["资料页", "创建表单（含校验与错误态）"]
  acceptance:
    commands: ["pnpm -C web typecheck"]
    criteria: ["表单可键盘操作，错误信息关联到输入框"]
  size: M
  difficulty: 2

- id: T3
  domain: testing
  title: 用户模块测试
  goal: 为 C1 的每个状态码补齐接口测试，为表单补齐组件测试
  depends_on: [T1, T2]
  contracts: [C1]
  write_scope: ["tests/users/**", "web/src/pages/users/**/*.test.tsx"]
  acceptance:
    commands: ["cargo test -p server", "pnpm -C web test"]
  size: S
  difficulty: 1
```

# Notes
给执行者的补充约束，可为空。
````

字段说明：

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | 是 | 计划内唯一，`T` + 序号 |
| `domain` | 是 | 取自定义包的领域表（[04 knowledge-pack](../04-model-matching-engine/knowledge-pack.md)） |
| `depends_on` | 是 | 上游任务 id；下游任务会收到上游的任务报告 |
| `contracts` | 否 | 引用的契约编号 |
| `write_scope` | 是 | 允许写入的路径 glob；只读任务为空数组 |
| `acceptance.commands` | 否 | 由 Tokage 在本地执行的验收命令 |
| `acceptance.criteria` | 否 | 由执行模型自查、写入任务报告的标准 |
| `size` | 是 | `S` / `M` / `L`，用于成本估算 |
| `difficulty` | 是 | 1–3，用于成功概率估算 |

## 2. 规划指令模板

固定部分在前（各次规划完全相同，可被缓存），可变部分在后。

```markdown
你是技术规划者。你的输出将被拆分给多个不同的 AI 编码代理并行执行，
它们彼此看不到对方的上下文，只能看到你写的契约和各自的任务。

要求：
1. 只写约束，不写实现代码。契约使用签名、类型、表结构、状态码表达。
2. 任务按领域拆分，可并行的任务写入范围不得重叠。
3. 每个任务必须能在不询问其他任务的情况下独立完成；需要的信息写进契约。
4. 每个任务给出可在本地执行的验收命令，命令从「项目摘要」中的已有脚本里选取。
5. 重复性、机械性的测试工作单独成任务，领域标为 testing。
6. 输出总长度不超过 {{output_max_tokens}} Token，严格使用给定格式。
7. 优先依据项目摘要；只有在摘要不足以确定接口时才读取文件。

可用领域：{{domains}}

输出格式：
{{format_spec}}

---
# 项目摘要
{{repo_digest}}

---
# 需求
{{user_request}}
```

增量重规划时，在「需求」之前追加「当前计划」「已完成任务报告」「变更说明」三段，并把要求 6 改为「只输出差异：新增、修改、取消的任务与契约」。
