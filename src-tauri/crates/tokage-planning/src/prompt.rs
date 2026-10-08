//! Prompt template rendering for planning requests (docs/02 §5, docs/02 plan-format.md).

/// Default planning output format specification markdown injected into the prompt.
pub const PLAN_FORMAT_SPEC: &str = r#"```markdown
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
type User = { id: string; email: string };
```

## C2 目录约定
- 说明或约定路径

# Tasks
```yaml
# 支持三级粒度拆分：P0/P1/P2 (里程碑) -> P0.M1/P0.M2 (模块) -> P0.M1a/P0.M1b (原子执行任务)
- id: P0.M1a
  domain: backend-api
  title: 用户鉴权与Token发放
  goal: 实现用户登录验证并颁发 JWT Token
  depends_on: []
  contracts: [C1]
  write_scope: ["src/auth/**"]
  deliverables: ["AuthService 与 Token 发放逻辑"]
  acceptance:
    commands: ["cargo test -p tokage-auth"]
    criteria: ["单测全部通过且无编译告警"]
  size: M
  difficulty: 2
- id: P0.M1b
  domain: frontend-ui
  title: 登录表单与状态同步
  goal: 对接用户 API 实现前端登录交互
  depends_on: [P0.M1a]
  contracts: [C1]
  write_scope: ["src/components/auth/**"]
  deliverables: ["LoginForm 组件"]
  acceptance:
    commands: ["npm test"]
    criteria: ["表单验证与接口调用成功"]
  size: S
  difficulty: 1
```

# Notes
给执行者的补充约束，可为空。
```"#;

/// Default system instructions template for planning invocation (docs/02 plan-format.md §2).
pub const PLANNING_SYSTEM_TEMPLATE: &str = r#"你是技术规划者。你的输出将被拆分给多个不同的 AI 编码代理并行执行，它们彼此看不到对方的上下文，只能看到你写的契约和各自的任务。

要求：
1. 只写约束，不写实现代码。契约使用签名、类型、表结构、状态码表达。
2. 任务采用层级拆分（P0/P1/P2 里程碑 -> P0.M1 模块特性 -> P0.M1a/P0.M1b 原子可执行任务），可并行的任务写入范围不得重叠。
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
{{user_request}}"#;

/// Standard domains if knowledge pack does not override them.
pub const DEFAULT_DOMAINS: &[&str] = &[
    "backend-api",
    "frontend-ui",
    "database",
    "devops",
    "testing",
    "security",
    "docs",
    "general",
];

/// Parameters required to construct a full planning prompt.
#[derive(Clone, Debug)]
pub struct PlanningPromptArgs<'a> {
    pub user_request: &'a str,
    pub repo_digest: &'a str,
    pub domains: Option<&'a [String]>,
    pub output_max_tokens: Option<usize>,
    pub custom_format_spec: Option<&'a str>,
}

/// Renders the complete planning prompt according to docs/02 plan-format.md.
pub fn render_planning_prompt(args: PlanningPromptArgs<'_>) -> String {
    let output_max_tokens = args.output_max_tokens.unwrap_or(6000);
    let format_spec = args.custom_format_spec.unwrap_or(PLAN_FORMAT_SPEC);

    let domains_str = if let Some(custom) = args.domains {
        custom.join(", ")
    } else {
        DEFAULT_DOMAINS.join(", ")
    };

    PLANNING_SYSTEM_TEMPLATE
        .replace("{{output_max_tokens}}", &output_max_tokens.to_string())
        .replace("{{domains}}", &domains_str)
        .replace("{{format_spec}}", format_spec)
        .replace("{{repo_digest}}", args.repo_digest.trim())
        .replace("{{user_request}}", args.user_request.trim())
}

/// Format-repair prompt based on [`PLAN_FORMAT_SPEC`] (F16.b — single source of truth).
pub fn render_plan_repair_prompt(raw_text: &str) -> String {
    format!(
        "请将以下技术规划内容严格转换为符合 Tokage 规约的 Markdown 格式。\n\
         必须包含以下四个一级段落，顺序固定，且任务图必须使用 yaml 代码块：\n\n\
         {PLAN_FORMAT_SPEC}\n\n\
         规则：\n\
         1. 严禁使用 Markdown 表格描述任务，必须使用 yaml 代码块。\n\
         2. 保留原计划中的所有任务拆分、依赖、目标和文件路径。\n\n\
         原始内容如下：\n\n{}",
        raw_text.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_planning_prompt() {
        let prompt = render_planning_prompt(PlanningPromptArgs {
            user_request: "实现用户注册功能",
            repo_digest: "## 目录结构\n- src/main.rs",
            domains: None,
            output_max_tokens: Some(4000),
            custom_format_spec: None,
        });

        assert!(prompt.contains("你是技术规划者"));
        assert!(prompt.contains("4000 Token"));
        assert!(prompt.contains("可用领域：backend-api, frontend-ui"));
        assert!(prompt.contains("# 项目摘要\n## 目录结构\n- src/main.rs"));
        assert!(prompt.contains("# 需求\n实现用户注册功能"));
        assert!(prompt.contains("# Spec"));
        assert!(prompt.contains("# Tasks"));
    }
}
