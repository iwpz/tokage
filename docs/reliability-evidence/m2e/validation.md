# m2e 本机验证记录

日期：2026-10-19。适用范围、恢复策略和剩余限制见 [m2e 记录](../../reliability-m2e.md)。环境及最终源码指纹记录在 environment.json。以下检查不进行付费模型推理，测试数据和 Git 归档均为隔离 fixture。

- `npm run typecheck`：退出码 0，见 typecheck.log。
- `npm run test`：退出码 0，38 个文件、310 项通过，见 frontend-tests.log。
- `npm run build`：退出码 0，见 build.log；保留原有动态导入和插件耗时提示。
- `npm run check:i18n`：退出码 0，1636 keys、114 TS / 129 Rust 文件，见 i18n.log。
- `cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked`：最终单次串行 Cargo 检查，591 项通过、0 失败、27 ignored，包含 doc-tests，见 rust-tests.log。27 项 ignored 未计通过；内部子进程 helper 的实际行为由对应父测试核验，不以 helper 默认无环境变量退出作为独立故障证明。
- 上述 Rust 检查同时设置 `TOKAGE_M2E_EVIDENCE_DIR`，生成真实二进制修改的 Git bundle、补丁、父节点和树标识，见 git/。
- `python3 scripts/verify-m2e-recovery-evidence.py`：退出码 0，离开原临时目录重新导入 bundle；恢复树、补丁还原树及验收树相同，合并父节点和留存源分支匹配，二进制产物符合 fixture 预期，见 recovery-artifacts.log。
- `python3 scripts/verify-m2c-git-evidence.py`：退出码 0，原有 m2c 交付归档仍可复核，见 m2c-artifacts.log。
- `python3 scripts/check-ipc-wiring.py`：退出码 1，139 命令中仍有原有五项无生产调用，见 ipc-wiring.log。plan_in_window_context_cmd、apply_context_action_cmd、dismiss_context_action_cmd、build_compact_cmd、record_context_action_cmd 仍留 m2f，未伪记为通过。
- 源码及 Markdown 的 `git diff --check` 通过。原始日志 / Git 补丁按原字节保留，不为消除输出中的空白或 ANSI 提示改写证据。

## 验证过程中的失败

开发期间修订接口后，旧 fixture 缺少交付目标绑定、仍期望恢复时重复合并等断言出现失败，已按新的生产契约补齐 fixture 并保留严格检查。新增故障用例暴露的未知启动、缺失完成凭据、重复恢复和丢失保存确认均有回归，未通过忽略测试绕开。

完整检查还发现既有 Kiro hard 用例同时配置 1 秒空闲超时、2 秒硬超时，在负载下先触发了空闲超时。hard 子场景现在独立配置较长空闲时限，保留 2 秒硬截止并要求实际收到 heartbeat；取消 / idle / 非零退出子场景不变，未放宽生产超时逻辑。

中间阶段在源码仍变化时并行运行了多次 Cargo，曾出现旧构建的 doc-test 与新依赖元数据不一致（E0460 / E0432）。这些中间结果未记为最终通过；最终停止并行构建，在固定源码上单次完整执行 workspace 测试，包含 doc-tests。没有删除 doc-tests、降低检查范围或忽略新增失败。

完整桌面强杀恢复、服务商实际请求是否已计费以及跨环境异常矩阵不由这些分层日志证明。无法确认旧请求结果的路径按 BLOCKED 验收；未知费用保留为未知，未生成虚假账单。
