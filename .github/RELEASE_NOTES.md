## Tokage v{{VERSION}}

### 下载与安装 / Downloads

| 平台 (Platform) | 架构 (Arch) | 安装包文件名 (Installer) |
| :--- | :--- | :--- |
| **macOS** | Apple Silicon (`ARM64`) | `Tokage_{{VERSION}}_aarch64.dmg` |
| **Windows** | 64-bit (`x86_64`) | `Tokage_{{VERSION}}_x64-setup.exe` / `Tokage_{{VERSION}}_x64_en-US.msi` |

---

### macOS 首次打开说明（Gatekeeper 提示处理）

将 `Tokage.app` 拖入「应用程序（`/Applications`）」后，若首次启动提示 **「无法打开“Tokage”，因为Apple无法检查其是否包含恶意软件」** 或 **「“Tokage”已损坏，无法打开」**（因当前预览版未附带 Apple 在线公证票据），请在终端（Terminal）执行以下命令清除下载隔离属性后再打开：

```bash
xattr -cr /Applications/Tokage.app
```

> 也可以前往 **系统设置 → 隐私与安全性**，在页面底部点击 **「仍要打开」**。

---

### macOS First-Launch Notice (Gatekeeper)

After dragging `Tokage.app` to `/Applications`, if macOS Gatekeeper blocks the first launch because the preview build is not notarized with Apple, run the following command in Terminal before opening the app:

```bash
xattr -cr /Applications/Tokage.app
```

> Alternatively, open **System Settings → Privacy & Security** and click **Open Anyway**.
