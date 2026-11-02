# T01：会话历史特殊键修复

代码候选：`01d9325`。本项独立修复历史组件，不恢复原可靠性第十项，不增加连续试用计数。

## 问题与修正

普通对象的继承属性使 `constructor` / `toString` / `__proto__` 被误认成已有分组，模型组调用 `.push` 失败，Provider 组还可能不出现在枚举结果中。名称格式化字典存在同类读取风险。

历史分组与展开偏好改用 Map，未设置偏好默认展开；搜索临时展示结果，不覆盖用户原有折叠选择。Provider/模型复合键使用 JSON 元组编码，包含 `::` 的不同组合不会碰撞。共享名称和角色样式只查询自有属性。没有修改历史查询、IPC、轮询、模型路由或持久数据。

## 自动化证据

- 新增 6 个特殊模型/Provider 键用例，在旧实现中全部失败，原有 8 项通过；红测原始输出保留。
- 新增跨 Provider 同名模型分组、搜索/Escape/按钮清除、选择及 transcript 保持、无额外 IPC、展开偏好和复合键碰撞回归。
- 共享名称格式化增加 3 个特殊键用例。最终全量 89 文件 / 590 项通过，最多 2 worker。
- 生产构建（含 TypeScript）、i18n 1924 键和测试隔离检查通过。提交候选独立克隆的 18 项针对性测试通过。

特殊字符串仅存在于测试输入，未作为真实服务商模型名发送。组件使用测试文件内 IPC mock，这属于离线 DOM 回归，不称正常桌面证明。

## 桌面复验协议

预注册 [registration.json](reliability-evidence/t01/registration.json)，external-ui-v1。从已提交代码创建独立克隆 `/Users/iwpz/Documents/tokage-validation/tokage-desktop-test-ws9zr_mm/repo`，release 构建使用标准 Tauri overlay。预先声明复用先前 M2.e 隔离 App 的 identifier 和独立数据路径，只读取已有真实历史；不复制/写入数据库，不改 Store，不注入 JS。

npm 普通缓存和 APFS 复制的旧 Cargo target 在构建前声明，不作冷构建性能结论。WebView ephemeral，不证明前端跨重启持久性。CLI HOME 未隔离也未改动；本次允许新增模型请求数为 0。精确候选与配置摘要见 [candidate.json](reliability-evidence/t01/candidate.json)。

正常 UI 复验通过，完整步骤见 [ui-result.json](reliability-evidence/t01/ui-result.json)：当前会话 GPT-5.6-Luna 的 3 轮正文、项目级 GPT-6-Luna 的 4 轮正文均能读取；已折叠模型在搜索期间展开，清除按钮/Escape 恢复原偏好；无匹配时保留已选正文。新模型默认展开，原选择的折叠偏好未被项目级历史加载覆盖。

本次仅沿用既有允许项，无新增 CLI 授权；只读计数前后相同（attempts 5、usage 7、chat logs 24、sessions 3），没有新模型轮次。正常退出隔离 App。特殊键的桌面持久化记录不通过数据库注入构造；本项特殊键正确性的证据为组件回归，普通桌面复验仅验证既有真实历史读取/交互无回归。历史 Claude 记录不是当前安装或访问证据。

原生 release 构建成功；未改 Rust 行为，未重复运行无关 Rust 全量或付费测试。AX 观察保留在当前会话，步骤以 JSON 归档，没有独立截图文件。原始日志无损 gzip 及 [哈希清单](reliability-evidence/t01/sha256.json) 已归档。T01 按上述范围收口，不宣称特殊键真实模型支持或持久 WebView 验收。
