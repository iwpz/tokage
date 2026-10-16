# m1g：里程碑验收与文档同步

日期：2026-10-16。验收代码基线：`8131c8f`（m1a–m1f），另移除已被严格快照替代的旧 `git_staged_diff_names` IPC、前端包装与调试白名单项。没有增加豁免，也没有改变测试忽略配置。本记录与[可靠性优化计划](可靠性优化计划.md)共同限定 m1 的完成结论。

## 结论

m1 的代码与本机自动化验收通过：关键保存或安全检查失败会停止受影响推进；执行资料按会话/计划/任务隔离；普通 Git 故障不会生成伪冲突；自动还原在证据不足或检测到后续修改时拒绝覆盖；正常交付合并要求绑定实际内容的通过证据。

这不是“已经可以放心大胆执行任意需求”的结论。真实需求、CLI/模型/授权组合、桌面人工操作、跨进程竞争和完整崩溃恢复仍须 m2；日常依赖程度仍须 m3 连续样本。现有安全协议对未知状态保守阻塞，而不是保证任意故障均可自动恢复。快照无法观测 ignored、仓库外及网络/数据库副作用，也无法冻结外部程序的写入。

## 可重复回归与修复前后依据

以下测试均在本轮全量命令中重新运行。前端测试直接调用生产 store、任务执行、审批、验收及恢复函数，IPC 用替身注入失败；Rust 测试使用生产存储/Git 函数和真实本地临时数据库、文件系统及子进程。它们不等同于运行桌面 WebView 或付费模型。

- **m1a**：原并发保存实验 160 次中 137 次错误，保存失败只记控制台。现在 `src/stores/execution.persistence.test.ts`、`src/lib/windowClose.test.ts`、`src/lib/taskRunner.test.ts` 验证串行、失败暂停、关闭等待、派发/审批/Git 清理屏障；`tokage-planning/src/storage.rs` 的 `pipeline_concurrent_writes_and_stale_revisions`、`pipeline_process_exit_during_save_recovers_complete_snapshot` 验证 160 次并发成功、拒绝旧版本和保存中终止子进程后恢复完整快照。详见 [m1a](reliability-m1a.md)。
- **m1b**：原按 T1 查询混入另一会话报告。现在 `src/lib/artifactIdentity.test.ts` 和 taskRunner 的 m1b 用例验证生产查询参数、重规划继承与拒绝错误身份；`tokage-storage/src/orchestration.rs` 的四个 `m1b_*` 用例验证跨项目/会话/版本、并发重开、错误 attempt 和 V11 迁移。未知历史保留未关联，不猜测或删除。详见 [m1b](reliability-m1b.md)。
- **m1c**：原范围读取异常可能继续，事件漏报和 gate 写入可能逃过检查。现在 `src/lib/scopeGuard.test.ts` 通过任务完成/接管等生产入口断言不提交、不 DONE、不派发下游；`tokage-native/src/git/scope.rs` 验证真实磁盘/index/删除/链接、失效证据、过滤器及提交 tree。其他任务 scope 不再用于排除无法归属的改动。详见 [m1c](reliability-m1c.md)。
- **m1d**：原不存在的分支得到空文件 Conflict，触发无效修复。现在 `src/lib/gitDelivery.test.ts` 的 18 项验证保存阶段冷加载、Git 故障和重试不重新调用模型/gate；`tokage-orchestration/src/lifecycle.rs` 的五个 `m1d_*` 和 native scope 中三个 `m1d_*` 验证锁/hook/源身份、真实冲突、私有基线、丢响应后的提交/合并对账。详见 [m1d](reliability-m1d.md)。
- **m1e**：原按 index 还原会覆盖执行前未暂存内容。现在 `src/lib/taskRollback.test.ts` 和 scopeGuard 启动入口验证备份失败、确认/取消、轮次、归属及部分失败不报成功；`tokage-native/src/git/rollback.rs` 九项测试验证原有暂存/未暂存/未跟踪/二进制、新文件、权限、后续人工修改、恢复副本与真实本地替身子进程封口。详见 [m1e](reliability-m1e.md)。
- **m1f**：原空命令、缺失全量结果可默认为通过，普通确认可能合并失败内容。现在 `src/lib/acceptance.test.ts` 16 项通过生产验收/交付入口验证全部结果、取消、跳过、内容失效和重新验收；`tokage-native/src/git.rs` 的 `m1f_delivery_requires_exact_accepted_source_content_and_commit` 验证真实合并 SHA，planning 持久化和 orchestration 摘要验证结果往返与缺失证据不通过。详见 [m1f](reliability-m1f.md)。

修复前数值和行为来自计划基线及各子节点已保存的实施记录；本轮没有切回旧提交重跑，也不把历史实验解释为实际故障概率。当前回归包含可暴露旧行为的断言，失败注入后的“拒绝推进/保留内容”是通过条件；不是仅验证函数能返回。所有源路径均相对仓库根；Rust crate 路径前缀为 `src-tauri/crates/`。

## 运行命令与持久证据

在仓库根运行，无需真实 CLI 凭据：

```sh
npm test
cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked
npm run build
npm run check:i18n
python3 scripts/check-ipc-wiring.py
git diff --check
```

- 前端：35 文件、251 项通过，退出 0。
- Rust：555 通过、26 个原有 ignored、0 失败，退出 0。ignored 包含真实模型/本机配置用例和子进程辅助入口，均不算实机通过；本节点没有新增 ignored/skip。
- 构建（含 TypeScript）：通过，退出 0；已有 chunk 体积、混合静态/动态导入及构建耗时提示未消除。
- 国际化：1615 个键、111 TS / 126 Rust 文件通过，退出 0。
- IPC：移除旧入口后为 135 个命令、5 个无生产调用，退出 1：`plan_in_window_context_cmd`、`apply_context_action_cmd`、`dismiss_context_action_cmd`、`build_compact_cmd`、`record_context_action_cmd`。与计划基线的五个缺口一致，留给 m2f，不能称整个仓库检查全绿。
- `git diff --check`：通过。文档相对链接与本节点引用的迁移/测试源路径另行核对。

本轮精简终端证据保存在 [m1g 验收输出](reliability-evidence/m1g-checks.txt)，含环境、测试汇总、忽略原因及 IPC 缺口；各子节点记录保留此前的结果。临时完整日志只用于排错，不是唯一长期证据。GitHub Actions 仍未启用，本结论不引用 CI 绿灯。统一仓库检查入口和 IPC 缺口收口属于 m2f。

## 状态、恢复与接手约定

1. **保存失败**：停止派发，保留最新内存状态和错误；点击重试保存后再决定继续，不删状态文件绕过。窗口关闭失败应保持窗口。原子替换后的同步失败不能解释为“完全没写入”。
2. **范围或身份失败**：查看失败日志中的实际 cwd、HEAD、路径与报告身份。缺失旧基线不得事后补拍；不把 worktree 失败改成 inplace 成功，也不按 T1 搜另一会话报告补齐。
3. **Git 失败**：保留任务与集成分支、checkout 和 `gitDeliveries`。继续/重试先对账已保存意图；锁、hook、源分支变化或未知 merge 状态交给用户检查，不能强制清理唯一产物。任务合并已保存后才清理；最终用户合并结果已保存后才清理集成 checkout。
4. **回滚**：确认具体路径及 turnId；自动还原只使用原有内容备份与已封口结束证据。后续人工修改、缺失证据或部分失败时保留现场；错误中的 `recovery-*` 目录包含覆盖前副本供人工核对，不自动覆盖重试。已提交/合并结果不走本轮还原入口。
5. **验收与交付**：DONE 只说明执行结束，未配置验收不是通过；用户命令批准不是验收失败的合并授权。内容/任务/命令变化后重新验收。取消/失败/旧证据不能经普通确认合并；导出或保留分支也不改变验收结论。
6. **重启**：DONE 保留，在途任务转 INTERRUPTED，不自动派发；交付验收中途结果重置后重新执行。执行状态文件与 Git/SQLite/CLI 不是统一事务，跨进程所有权、结果丢响应的完整幂等及外部写入竞争留给 m2e，不承诺所有副作用恰好发生一次。

中英文 [编排](03-orchestration-engine/README.zh-CN.md)、[存储](08-storage/README.zh-CN.md)、[防护](09-guardrails/README.zh-CN.md)、[授权](11-command-authorization/README.zh-CN.md)及存储 schema 说明已同步，替换旧范围排除、checkout/clean 还原、空验收通过与宽泛续跑描述。硬件断电、其他平台、桌面按钮操作和付费 CLI 需求均未在本节点实测。
