# Kiro 升级环境与探测核对（CLI-MGMT-R4a）

2026-10-26 用户升级 V3 后授权检查本机与 Tokage 探测并修复问题。本次仅当前 macOS / 现有账号的非推理检查：版本/help、脱敏 whoami、显式 V2/V3 模型目录、ACP initialize、Tokage 原生 detect_one。不调用模型、不查询钱包、不登录/注销、不改变用户引擎设置或运行中的 Tokage。

## 环境与问题

PATH 指向 `~/.local/bin/kiro-cli`，链接至 `/Applications/Kiro CLI.app/Contents/MacOS/kiro-cli`；主程序和 chat 二进制均报告 2.34.0。已有 2.34.0 KAS 缓存，不凭目录名单独认定 V3 可用。

本机 whoami 正向身份有效；`chat --v2/--v3 --list-models --format json` 分别成功返回 22 项。两引擎的 `acp --agent-engine v2/v3` 均成功响应 initialize，协议版本 1；未发送 session/new 或 prompt。V2 agentInfo 报 2.34.0，V3 没有 agentInfo，不虚构独立后端版本。V3 stderr 非空（852 字节），原文未留存，不能宣称无警告；握手结果正常。

确认 Tokage 缺陷：新会话默认固定 V2，而原模型目录探测未指定引擎，可能随官方默认配置漂移。现在通过共享选择函数把目录查询显式绑定 `--v2` 或 `--v3`；默认仍 V2，`TOKAGE_KIRO_ENGINE=v3` 选 V3。非法值不查询目录、不冒充 Ready；带引擎标签的旧会话恢复优先使用其原引擎，不因新会话设置而改变。

登录仍是共享账号探测，不把 whoami 或目录成功当作 V3 模型调用成功。两引擎目录目前相同，不意味着未来目录或权限语义永远相同。

## 验证范围与停止标准

离线回归：目录 argv 与派发引擎一致、默认/显式 V2/显式 V3/非法值、旧会话恢复保持 V3、临时 agent 清理、未登录不查目录，及既有 Kiro 解析/权限/取消回归。原生全量、桌面 crate check、隔离检查通过后本地提交。

提交后从独立 checkout 运行显式 ignored 的 `providers::kiro_tests::installed_engine_detection`：使用正常登录 shell 捕获和正式 detect_one/沙箱，不调用其他 CLI，不输出身份原文。依次检查捕获设置、仅该诊断进程的 V2/V3 引擎选择；这些覆盖明确记录，不写用户配置。不为失败更换 HOME/凭据/代理或放宽沙箱；失败保留，先定位原因，不重试模型。

证据见 [环境回执](reliability-evidence/kiro-upgrade-20261026/environment-result.json)。原生诊断与桌面 UI/真实对话分别记账；不更新旧版完整角色链、V2 内容/恢复失败及 V3 高级能力验收结论。

## 最终结果（CLI-MGMT-R4b）

实现提交 `181d15d`：原生离线全量 422 通过 / 34 显式忽略；桌面 crate check、隔离与空白检查通过。新加真实探测检查默认 ignored，未混入离线通过数。

该提交的独立克隆通过 APFS 复制既有 debug 构建缓存，重新编译本地 crate；不属于冷构建或启动性能测量。正式原生探测实测 1 项通过，9.49 秒完成三个检查：正常登录 shell 捕获结果为未设置引擎（Tokage 默认 V2）；捕获设置、诊断进程显式 V2、显式 V3 均为 READY / 2.34.0 / CLI 目录 22 个模型 / 包含 Haiku 4.5。未放宽沙箱、改代理、改凭据或用户配置，模型请求为 0。独立克隆及日志路径、SHA-256 已归档至环境回执。

当前本机 V3 初始化及 Tokage 原生检测未发现阻塞。默认引擎仍 V2，升级官方 V3 不会自动改变 Tokage 新会话设置；显式 V3 已在本次诊断进程验证。未重启或替换运行中的旧 Tokage，也不把原生探测通过当成新版本桌面 UI、真实模型内容、恢复记忆或工具编辑验收。
