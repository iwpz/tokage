# 12 · 网络代理 (Network Proxy)

返回 [README](../../README.md)

> 现状（2026-10-04）：未实现。子进程环境为登录 Shell 环境原样透传（`src-tauri/src/chat/mod.rs`），`.zshrc` 中 export 的代理变量会传给 CLI，macOS 系统代理设置不会。实施计划见 [TODO](../TODO.md) P3.m6。

## 1. 职责

1. 让用户为每个 CLI 选择代理方式：跟随全局、跟随 CLI 自身、不使用、系统代理、指定代理。
2. 可选地强制 CLI 只能经由 Tokage 本地代理出网。
3. 代理凭据只留在 Tokage 内部，不暴露给模型。

非目标：不解密 TLS、不读取或改写请求内容、不做透明代理（TUN）、不按厂商拦截目标域名。

## 2. 各 CLI 的代理识别

以下为调研起点，开发前逐个实测并写入定义包 `providers.<id>.proxy`：

| CLI | 运行时 | 认环境变量 | 自身配置覆盖环境 | SOCKS |
|-----|--------|-----------|-----------------|-------|
| Claude Code | Node | `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY`（官方文档） | 会，`settings.json` 的 `env` 块 | 待验证 |
| Codex CLI | Rust | 待验证 | 待验证 | 待验证 |
| Cursor CLI | Node | 待验证；Node 内置 fetch 需 `NODE_USE_ENV_PROXY=1`（Node 24+） | 待验证 | 待验证 |
| Copilot CLI | Node | 同上 | 待验证 | 待验证 |
| OpenCode | Bun | 待验证 | 待验证 | 待验证 |
| Aider | Python | 认 | 无 | 需额外依赖 |
| Antigravity CLI | 待调研 | 待调研 | 待调研 | 待调研 |
| 模型派生的 git / npm / pip / curl | 各异 | 认 | git / npm 有自身配置 | 部分 |

定义包字段：

```json
"proxy": {
  "env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"],
  "extra_env": { "NODE_USE_ENV_PROXY": "1" },
  "socks": false,
  "own_config_overrides": true,
  "forced_ok": true
}
```

## 3. 代理模式

| 模式 | 做法 | 限制 |
|------|------|------|
| `inherit_global` 跟随全局 | 使用全局设置 | 仅用于单个 CLI |
| `cli_own` 跟随 CLI 自身 | 登录环境中的代理变量原样透传（即现状） | 无 |
| `none` 不使用 | 剥离所有 `*_PROXY` / `*_proxy` / `ALL_PROXY` | CLI 自身配置中的代理仍会生效，面板提示 |
| `system` 系统代理 | 读取系统代理（macOS SystemConfiguration、Windows 注册表 / WinHTTP、Linux 桌面设置与环境），按 `manual` 注入 | PAC 无法转为环境变量，需本地代理（§4）执行 |
| `manual` 指定代理 | 注入 `HTTP(S)_PROXY` / `ALL_PROXY` / `NO_PROXY` 及 `extra_env`；SOCKS 上游由本地代理转为 HTTP | 同 `none` |

- 全局默认 `cli_own`，保持现有行为。
- `NO_PROXY` 默认包含 `localhost,127.0.0.1,::1`，可编辑。
- 可选 CA 证书文件（企业解密代理）：注入 `NODE_EXTRA_CA_CERTS` / `SSL_CERT_FILE` / `REQUESTS_CA_BUNDLE`。
- Tokage 自身的网络请求（定义包更新）使用全局设置。

## 4. 本地代理与强制模式

只注入环境变量是建议而非强制：CLI 可能不识别，或被自身配置覆盖。强制方式：

```
CLI 进程树 ──(沙箱：只允许连 127.0.0.1:<port>)──► Tokage 本地代理（仅 CONNECT 转发）
                                                    └─► 上游：系统代理 / 指定代理 / 直连
```

- 本地代理只解析 CONNECT 目标并转发字节流，不解密、不记录内容；连接日志只记目标 host 与字节数（可关闭）。
- 每个 CLI 一个独立端口或独立凭据，便于按 CLI 选择上游。
- 不识别代理环境变量的 CLI 在强制模式下会连接失败而非绕过；定义包中 `forced_ok: false` 的 CLI 不开放强制开关。
- `none` + 强制 = 本地代理直连上游，可压过 CLI 自身配置的代理。
- 依赖 [11 §5](../11-command-authorization/README.md) 的系统级沙箱；Windows v1 不提供强制模式。

代理凭据：

- 用户名密码存入系统钥匙串，只由本地代理使用。
- 子进程环境中只出现 `http://127.0.0.1:<port>`，模型执行 `env` 看不到上游代理凭据。
- 未开启本地代理的 `manual` 模式若带凭据，凭据会进入子进程环境，界面提示并建议开启本地代理。

## 5. 设置页

「网络代理」页：

- 顶部：全局默认模式、上游配置（类型 HTTP / HTTPS / SOCKS5、地址、端口、认证）、`NO_PROXY`、CA 证书、本地代理开关。
- 下方 CLI 列表：只列出已安装、探测状态可用且能获取模型列表的 CLI（`READY` / `DEGRADED` 且模型列表非空）。每行：
  - 模式下拉（默认 `inherit_global`）；选 `manual` 时可填单独的上游。
  - 「强制」开关，不支持时置灰并说明原因。
  - 识别情况：认哪些变量、自身配置是否设置了代理（只显示是否设置与 host，不显示凭据）。
  - 「测试连通」：经本 CLI 的生效配置对其官方端点发起一次不消耗额度的 TLS 握手。
- 修改在下一轮生效，不影响运行中的轮次（R9）。

## 6. 配置项

| 键 | 默认 | 说明 |
|----|------|------|
| `proxy.mode` | `cli_own` | 全局默认 |
| `proxy.upstream` | 空 | 类型、地址、端口；凭据在钥匙串 |
| `proxy.no_proxy` | `localhost,127.0.0.1,::1` | |
| `proxy.ca_file` | 空 | |
| `proxy.local_relay` | `false` | 本地代理 |
| `proxy.per_cli.<id>` | `{ mode: inherit_global, forced: false }` | 单 CLI 覆盖 |
