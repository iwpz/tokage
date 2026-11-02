# M2.c：原生 compact 接线

本节点 `M2.c-wire` 完成普通主会话的 Adapter 原生压缩接线与离线协议验证。M2.c1/c4 可勾选；M2.c2 的能力门控已实现，但实际 CLI 的能力确认与 M2.c3 真实端到端记录仍待完成。本次没有调用真实模型、没有执行桌面真实验收，不把合成协议测试称为录制或 live 通过。

## 选择路径

普通预览、排队、发送前 prepare 使用同一原生解析入口。Claude 原生命令候选根据当前原生扫描的 ProviderInfo 和 Adapter 能力选择：仅 Ready 且检测版本恰为 2.1.288 时开放 COMPACT_HEADLESS 路径；其他/未知版本回退已有同窗保留指示，payload 不再残留原生命令。这里的版本是协议回归 allowlist，不是本机真实模型验收背书，扫描后换二进制仍应重新扫描。预览本身不调用模型。

Claude 的无头候选有官方 SDK 协议依据：[Commands in Agent SDK sessions / Compact history](https://code.claude.com/docs/en/agent-sdk/slash-commands#compact-history-with-compact)。文档明确说明成功 result 也可能意味着历史不足，只有 compact_boundary 才表示实际压缩。当前读取到的本机版本为 2.1.288；本次只执行 --version，未据此证明命令实测通过。

COMPACT_PTY 没有开启，不把 TUI 命令存在当作 Tokage PTY 实现。Codex/Aider 的 SYNTHESIZED_FRESH_BRIEF 继续共用 M2.b 的进展来源、新窗和费用路径；FRESH 建议优先保持新窗语义，不因 Claude 存在原生候选而改成同窗操作。共享 Runtime 和任务窗不据此开放。

## 两阶段正常发送

1. 用户显式采纳或 auto 排队，SUGGESTED 阶段仍只落库；发送时重新校验会话、模型/启动参数、角色、目录、信任、冷却和来源，绑定同一条 APPLIED 动作。
2. prepare 分开返回原生命令与原样用户消息。原生命令通过 Adapter::build_compact 生成无头 Launch：必须 resume 当前线程、不允许 fork/schema，固定 /compact 命令名单行且有大小上限。它不经过规划/计划文件/历史提示词前缀，模型、effort、原有权限和沙箱仍走正常 launch；额外使用 --tools 空列表。命令参数经 stdin，shell 不拼接执行。
3. 原生监督器先运行压缩子进程，沿用原有超时、进程组清理、信任、代理和取消机制。只有同一 session、manual trigger、正数 pre_tokens 的 compact_boundary，随后同 session 的 success result，并且进程正常退出，才放行下一阶段。自动 boundary、其他线程、空历史成功、普通文本降级、非零退出、错误、取消均不派发用户消息。每阶段使用正常的进程超时；不把两阶段说成共用一个硬超时。
4. 确认后仍检查取消，再在同一线程运行用户原消息。压缩摘要文本不混入用户回答。界面分别显示“已排队”“原生压缩已确认”“承载轮已完成”；用户轮失败不抹掉此前的原生确认，也不产生成功节省结算。原生确认失败后不会隐式追加一次 prompt 回退模型调用。

## 用量与结算

新增 contextCompact 事件携带独立原生 usage receipt 和 confirmed 标记。原生压缩费用以 CONTEXT phase、turnId:compact 幂等键保存，用户轮按原 turnId 另存；不合并两条 Done 事件提前结束界面，也不将两个用量重复记账。没有 boundary 但 CLI 给出成功 result/usage 时，保存 confirmed=false 的已报告费用，并将动作判为失败。供应商错误或取消未给出完整 usage 时，不补造费用。已完成压缩后用户轮失败/取消，已收到的压缩 receipt 仍保留。

用户轮完成后，结算传入“压缩费用＋用户首轮费用”；method=native，明细 costScope=native_compact_plus_first_user_turn。基线仍是压缩前同一 horizon=1 的原窗估价，未知任一费用不结算。该差额是反事实估算，不是实际账单节省证明。FRESH 和旧 instruction 记录的口径及重试兼容性保留。

## 验证与待验收

合成 CLI 测试通过生产 run_turn / run_cli / Claude parser 验证同线程两阶段、费用分离、缺 boundary、foreign/auto boundary、纯文本降级、用户轮失败和确认后取消。Adapter 单测验证原生命令无提示词包裹、无工具、resume/fork 及命令边界；上下文单测验证能力回退与用户消息保真；前端验证命令/消息分离和已知/未知费用合计；存储验证 native 首轮结算幂等与未知值。

尚缺在已提交独立克隆、正常 UI 中运行 Claude 真实历史→采纳/auto→manual boundary→原消息的连续记录，包含模型/CLI版本、设置、实际两笔 usage 与取消。没有该记录，M2.c2 的实机能力和 M2.c3/M2.e3 不勾选。现存旧版隔离 App 不能证明本候选代码通过。

最终离线验证：前端全量 85 文件 / 569 项；上下文 45 单元 + 2 集成；存储 46 项；核心 40 项；原生全量 437 单元 + 1 集成通过（34 项忽略，未开启付费 live），最终 native chat 回归 205 项通过 / 26 忽略。生产构建、桌面 cargo check、i18n 1907 键、IPC 接线 141 命令及隔离/差异检查通过。使用前端最多 2 worker、Rust 2 job / 2 测试线程。初版合成测试因缺临时认证 fixture 在派发前被正常 Ready 门槛拒绝，补齐测试文件中的合成认证后才运行；没有改生产权限门槛。最终界面反馈新增键曾触发 TypeScript 白名单错误，已补齐对应键类型并重新构建通过。

## 2026-10-30 实机尝试

在已提交独立克隆 fbdac3a 的正常 UI 中，Claude Code 2.1.288 / Haiku 的唯一请求返回账号 on hold。未执行 `/compact`，没有 manual boundary，停止且未重试/更换认证；c2/c3 保持未完成。Codex 合成新窗回退已得到真实首包与线程证据。完整记录见 [M2.e 验收](m2e-acceptance.md)。
