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
