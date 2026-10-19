# m2d 本机检查记录

日期：2026-10-19。环境与最终源码 SHA-256 见 [environment.json](environment.json)，验收范围与限制见 [m2d 记录](../../reliability-m2d.md)。以下结果来自最终源码，未执行真实模型推理。

- `npm run test`：退出码 0，38 个文件、293 项通过，见 frontend-tests.log。
- `cargo test --workspace --locked`（在 src-tauri 执行）：退出码 0，580 项通过、0 失败、27 ignored，见 rust-tests.log。26 项为原有 ignored；新增的 1 项为内部强杀宿主 helper，已由通过的父测试作为子进程实际调用并检查，不作为独立通过项计数。
- `npm run typecheck`：退出码 0，见 typecheck.log。
- `npm run build`：退出码 0，见 build.log。构建器仍输出已有动态导入与插件性能提示，未用修改警告阈值掩盖。
- `npm run check:i18n`：退出码 0，1633 keys，114 TS / 127 Rust 文件，见 i18n.log。
- `python3 scripts/verify-m2c-git-evidence.py`：退出码 0，既有补丁、保留分支及合入归档仍能独立核验，见 m2c-artifacts.log。
- `python3 scripts/check-ipc-wiring.py`：退出码 1；136 个命令中仍有既有 5 项无生产调用，见 ipc-wiring.log。缺口保留给 m2f，未记为通过：plan_in_window_context_cmd、apply_context_action_cmd、dismiss_context_action_cmd、build_compact_cmd、record_context_action_cmd。
- 源码及 Markdown 的 `git diff --check` 通过。原始运行日志按原样保留，日志中的空白行 / ANSI 控制符不作为代码格式修改。

## 中间失败及修复

中间版本的一次全量 Rust 检查出现 `stderr_quota_fallback_triggers_quota_event` 的事件列表为空，摘录保留在 intermediate-quota-failure.txt。单独复跑曾通过，但没有据此删掉失败记录或忽略测试。排查补出了可确定复现的相同监督缺口：CLI 提前关闭 stdin 时，大输入触发 BrokenPipe，过早返回会丢失已输出的认证 / 额度协议。现在继续收集协议与 stderr，同时禁止输入写入失败的成功帧放行；新增 `m2d_closed_stdin_preserves_quota_error_instead_of_broken_pipe` 固定此路径，最终全量 580 项通过。该回归说明对应缺口已修复，不将一次全量通过解释为长期稳定性概率。

其余开发中类型和断言失败均已修正后再执行最终检查，未跳过失败用例。完整桌面强杀、服务商真实网络 / 余额变化和跨进程业务恢复不在这些日志的证明范围内。
