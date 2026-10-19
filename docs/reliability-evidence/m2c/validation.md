# m2c 最终验证

日期：2026-10-19。本记录对应 m2c 最终代码；真实桌面归档的执行时间和源码边界见实施文档。

- `npm test -- --reporter=dot`：38 个文件，277 项通过，退出码 0。
- `cargo test --workspace --locked`（src-tauri）：568 项通过，26 项忽略，0 失败，退出码 0；含 7 项真实 Git m2c 场景。忽略项不计为通过。
- `npm run build`：退出码 0；保留构建器的已有动态导入及性能提示。
- `npm run typecheck`：最终重规划保护调整后重新检查，退出码 0。
- `npm run check:i18n`：1629 keys，退出码 0。
- `python3 scripts/verify-m2c-git-evidence.py`：退出码 0；补丁精确树还原、保留分支和最终合入父提交均通过。
- 源码 `git diff --check`：通过。原始日志及 delivery.patch 保留真实空白和 CRLF，整包检查会提示这些证据文件的空白；不为消除提示改写证据字节。
- `python3 scripts/check-ipc-wiring.py`：退出码 1，136 命令中 5 项缺少生产调用，完整列表见 ipc-wiring.log；这是已登记的 m2f 待办，未记为通过。

真实桌面正常路径：OpenCode 复验通过；不将前端模拟或本地 Git 故障用例解释为真实 CLI 的断网、强杀、额度或恢复认证。m2d/m2e 未执行、未勾选。
