# 12 · Network Proxy

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

> Status (2026-10-05): Implemented (P3.m6). Five proxy modes (inherit global, CLI own, none, system, manual), system proxy read, local CONNECT relay (R18 zero decrypt), forced sandbox egress convergence, keychain credential isolation (P3.m6h), and non-quota connectivity probe.

## 1. Responsibilities

1. Let the user choose proxy behavior per CLI: inherit global, CLI own, none, system, manual.
2. Optionally force CLI egress only via Tokage local proxy.
3. Keep proxy credentials inside Tokage only; never expose to the model.

Non-goals: no TLS decrypt, no read or rewrite of request bodies, no transparent proxy (TUN), no per-vendor domain blocking.

## 2. Per-CLI proxy recognition

Starting points for research; verify each before development and record in pack `providers.<id>.proxy`:

| CLI | Runtime | Honors env | Own config overrides env | SOCKS |
|-----|---------|------------|--------------------------|-------|
| Claude Code | Node | `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` (official docs) | Yes, `settings.json` `env` block | TBD |
| Codex CLI | Rust | TBD | TBD | TBD |
| Cursor CLI | Node | TBD; Node fetch needs `NODE_USE_ENV_PROXY=1` (Node 24+) | TBD | TBD |
| Copilot CLI | Node | Same as above | TBD | TBD |
| OpenCode | Bun / Node | `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` | TBD | Yes |
| Aider | Python | `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` | None | `ALL_PROXY` (socks5://) |
| Antigravity CLI | Rust / Go | `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` | Yes, `settings.json` override | `ALL_PROXY` |
| Model-spawned git / npm / pip / curl | Various | Yes | git / npm have own config | Partial |

Pack field:

```json
"proxy": {
  "env": ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"],
  "extra_env": { "NODE_USE_ENV_PROXY": "1" },
  "socks": false,
  "own_config_overrides": true,
  "forced_ok": true
}
```

## 3. Proxy modes

| Mode | Behavior | Limits |
|------|----------|--------|
| `inherit_global` Inherit global | Use global settings | Per-CLI only |
| `cli_own` CLI own | Pass through proxy vars from login env (current behavior) | None |
| `none` No proxy | Strip all `*_PROXY` / `*_proxy` / `ALL_PROXY` | Proxy in CLI own config still applies; panel warns |
| `system` System proxy | Read system proxy (macOS SystemConfiguration, Windows registry / WinHTTP, Linux desktop + env), inject as `manual` | PAC cannot become env vars; needs local proxy (§4) |
| `manual` Manual | Inject `HTTP(S)_PROXY` / `ALL_PROXY` / `NO_PROXY` and `extra_env`; SOCKS upstream converted to HTTP via local proxy | Same as `none` |

- Global default `cli_own`, preserves today’s behavior.
- `NO_PROXY` default `localhost,127.0.0.1,::1`, editable.
- Optional CA file (enterprise TLS intercept): inject `NODE_EXTRA_CA_CERTS` / `SSL_CERT_FILE` / `REQUESTS_CA_BUNDLE`.
- Tokage’s own requests (pack updates) use global settings.

## 4. Local proxy and forced mode

Env injection alone is advisory: CLI may ignore or override with own config. Forced path:

```
CLI process tree ──(sandbox: only 127.0.0.1:<port>)──► Tokage local proxy (CONNECT forward only)
                                                    └─► Upstream: system / manual / direct
```

- Local proxy parses CONNECT target and forwards bytes; no decrypt, no content log; connection log may record host and byte count (disableable).
- Separate port or credentials per CLI to pick upstream per CLI.
- CLI that ignores proxy env fails to connect under forced mode rather than bypass; pack `forced_ok: false` disables forced toggle.
- `none` + forced = local proxy direct upstream, can override CLI-configured proxy.
- Depends on [11 §5](../11-command-authorization/README.md) OS sandbox; Windows v1 no forced mode.

Proxy credentials:

- Username/password in system keychain, used only by local proxy.
- Child env only sees `http://127.0.0.1:<port>`; model `env` cannot see upstream creds.
- `manual` with creds without local relay puts creds in child env—UI warns and suggests enabling local proxy.

## 5. Settings page

“Network proxy” page:

- Top: global default mode, upstream (HTTP / HTTPS / SOCKS5, host, port, auth), `NO_PROXY`, CA file, local relay toggle.
- CLI list below: only installed, probe `READY` / `DEGRADED` with non-empty model list. Each row:
  - Mode dropdown (default `inherit_global`); separate upstream when `manual`.
  - “Forced” toggle; grayed with reason when unsupported.
  - Recognition: which vars honored, whether own config sets proxy (show set/not and host only, no creds).
  - “Test connectivity”: one non-quota TLS handshake to official endpoint via this CLI’s effective config.
- Changes apply next turn, not running turns (R9).

## 6. Configuration

| Key | Default | Description |
|-----|---------|-------------|
| `proxy.mode` | `cli_own` | Global default |
| `proxy.upstream` | empty | Type, host, port; creds in keychain |
| `proxy.no_proxy` | `localhost,127.0.0.1,::1` | |
| `proxy.ca_file` | empty | |
| `proxy.local_relay` | `false` | Local relay |
| `proxy.per_cli.<id>` | `{ mode: inherit_global, forced: false }` | Per-CLI override |
