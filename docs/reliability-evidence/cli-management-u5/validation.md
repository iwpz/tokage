# CLI 管理重构验证记录

日期：2026-10-24–25；协议：external-ui-v1；范围以 request.json 为准。

## 当前代码验证

- 前端全量：73 files / 477 passed；包含真实 Store 订阅的 10 项生命周期回归。
- 原生全量：390 passed / 29 ignored / 0 failed；跳过项主要需要真实账号或模型，不计为通过。
- 原生 usage_query：51 passed / 1 ignored / 0 failed。
- 类型检查、前端生产构建、i18n（1834 keys）和测试隔离静态检查通过。
- 原生桌面 crate 编译检查通过；这些自动检查不替代真实桌面验收。

## 已保留的桌面尝试

所有目录均在 macOS 临时目录，使用提交版独立克隆、唯一应用标识、独立原生数据目录及标准 Tauri incognito WebView；没有页面注入或 Store/DB 改写。CLI 使用原本 HOME/网络，未复制凭据或触发模型推理。依赖安装和 APFS 构建缓存方案已在 request.json 预先登记。

- e09318f / tokage-desktop-test-ja1fdpgd：复用依赖缓存的首轮构建缺失 SQLite 生成文件；清理外部缓存后构建成功，未启动旧候选。
- 6f2626a / tokage-desktop-test-v_5k4zr4：正常 UI 自动取得 OpenCode/Reasonix 钱包、Copilot 额度；单卡刷新保留其他卡片和 Kiro 在途查询。Kiro 超时揭示退避起点错误，候选失败；通过正常 UI 关闭授权。
- 1b95961 / tokage-desktop-test-5yonkyvd：仅构建，未执行桌面验收。
- 9a4bfd3 / tokage-desktop-test-6k8t241o：Kiro 本地 Google 登录正确显示，超时后保留完整冷却间隔；OpenCode 来源读取失败被错误归为身份未知，候选失败。
- d8a73af / tokage-desktop-test-ejcwt581：自动查询及单卡刷新可见；首轮定时扫描保留 OpenCode/Reasonix/Copilot/AGY 数据。Cursor 重新核验后缓存仍携旧绑定版本，前端拒收恢复结果，候选失败。保留 08/09 的 AX 记录；没有把该次运行写为全通过。

## 当前修正及剩余门槛

U5-003d 将结果封套的当前绑定及状态版本与额度原始查询时间分开。回归确认同账号重新核验可恢复原缓存、额度获取时间/TTL 不变、旧状态迟到不会覆盖恢复结果、真正切换账号不复用旧快照。状态版本随原生条目持久化，旧格式读取有默认值。

最终修正版尚未完成正常桌面 UI 的两轮定时扫描、撤销授权与重启复验。续作时桌面操作工具不可调用，System Events 只读检查返回 UI elements enabled = false；未改变系统权限、注入界面或改用伪造 IPC。U5-004 保持未完成。

临时 WebView 无法证明跨重启的前端设置持久性。原生缓存重启行为虽有离线回归，真实 UI 重启仍待验证。Kiro 额度端点超时、Codex 额度适配边界以及各厂商可能拒绝请求的限制均不转写为“所有 CLI 完美支持”。
