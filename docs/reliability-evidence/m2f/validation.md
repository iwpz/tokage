# m2f 本机验证记录

日期：2026-10-19。完整范围、暂缓理由、版本升级步骤及付费入口见 [m2f 记录](../../reliability-m2f.md)。本节点没有真实模型请求或新增付费实机通过结论。

最终统一运行：`npm run verify:reliability -- --output /tmp/tokage-m2f-pass-20261019`。原始输出见各项 `.log`，退出码与分类见 summary.json，最终源码指纹与工具版本见 environment.json。

- 验收脚本回归：统一轮次 7 项通过；补充失败套件计数回归后，8 项定向复验通过（harness-supplement.log）。主动注入退出 7 / 9、缺失命令 127、退出 0 但 skip、空 Rust 测试选择、基线 / fixture 漂移，确认阻断有效；日志中的这些 FAILED / SKIPPED 是通过的负向断言，不是本轮真实目标失败。还覆盖缺少付费 opt-in 时未执行任何命令、继承付费 smoke 标志被移除、失败后后续检查仍运行、已有日志目录拒绝覆盖。
- TypeScript 检查、生产构建通过；构建保留已有动态导入及插件耗时提示。
- 前端：39 个文件、309 项通过。移除两项暂缓 IPC mock 测试，新增高上下文用量仍显示暂缓边界、没有操作按钮的界面回归；不靠减少生产 Rust 覆盖获得通过。
- Rust workspace：591 passed、0 failed、27 ignored、1 filtered，含 doc-tests。过滤项仅为会读取真实主机并可通过环境变量发起请求的 `chat::live::smoke_detect_skip_if_absent`。27 项 ignored 未执行，不计通过；本地模式没有 `--ignored`。
- i18n：1637 keys、114 TS / 129 Rust 文件；通过。
- IPC：134 命令、0 个未豁免的无生产调用缺口；原有两个只读查询豁免保留，没有为五个暂缓入口新增豁免。
- 固定 CLI 基线与 fixture 指纹检查通过；m2c 交付和 m2e 恢复 Git 归档复核均通过。
- `bash -n scripts/run-m2b-desktop.sh` 与源码 / Markdown `git diff --check` 通过。

统一入口首轮通过；第二轮实际捕获并阻断了文件锁延迟释放问题（9 passed / 1 failed / 0 skipped），原退出码 101、失败摘要和错误片段见 intermediate-failed/ 与 intermediate-lock-failure.txt。随后修复 Unix lease 的显式解锁并新增确定性的 dup 描述符所有权回归，在固定的生产源码上完整复跑。验收脚本随后补充 FAILED 套件计数解析及定向复验：旧失败摘要的测试小计只汇总了 ok 行，但 Rust 退出码 101 与该检查 failed 分类始终正确；现在失败测试数也保留，即使输出存在 failed 而进程误返回 0 仍拒绝通过。每项必要检查均退出 0，最终检查分类为 10 passed、0 failed、0 skipped；Rust ignored / filtered 单独统计，不能解释为付费测试已通过。

继续本地回归，没有启用 GitHub Actions；真实自然语言桌面正常路径脚本仅在显式 live + allow-paid 模式执行，未纳入本次模型额度消费。m2g 关键异常实机证据及 m3 日常使用仍未验收。
