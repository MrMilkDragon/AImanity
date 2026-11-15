# Install AImanity / 安装 AImanity

[0.0.2 Beryllium](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.2) includes Android, the Linux desktop and local services.

| Platform | Requirements | Installer | Factory |
| --- | --- | --- | --- |
| macOS | Apple Silicon, macOS 15+ | [DMG](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.2/AImanity-0.0.2-macos-arm64.dmg) | [macOS](engineering/macos-full-runtime.md) |
| Windows | Windows 11 x64, hardware virtualization | [EXE](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.2/AImanity-0.0.2-windows-x64-setup.exe) | [Windows](windows-client-factory.md) |

Open the app, choose your device settings, then add a model in Configuration. Beryllium uses ad hoc signing on macOS and self-signed Authenticode on Windows.

- **macOS.** If macOS reports an unidentified developer, open **System Settings → Privacy & Security → Open Anyway** after trying to launch AImanity. [Apple first-launch help](https://support.apple.com/en-us/102445)
- **Windows.** If SmartScreen shows **Windows protected your PC**, choose **Run anyway** when offered for the EXE downloaded from the Releases link above. [Microsoft SmartScreen help](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation#certificate-options-and-their-smartscreen-implications)

安装包内置 Android、Linux 桌面和本机服务。打开客户端，配置两台设备，然后在配置中心添加模型。macOS 采用 ad hoc 签名，Windows 采用自签名 Authenticode。

- **macOS。** 如果系统提示开发者身份无法验证，尝试打开 AImanity 后，进入 **系统设置 → 隐私与安全性 → 仍要打开**。[Apple 首次打开指南](https://support.apple.com/zh-cn/102445)
- **Windows。** 如果 SmartScreen 提示 **Windows 已保护你的电脑**，从上方发行页下载 EXE 后，可在出现 **仍要运行** 时选择该项。[微软 SmartScreen 指南](https://learn.microsoft.com/zh-cn/windows/apps/package-and-deploy/smartscreen-reputation#certificate-options-and-their-smartscreen-implications)

## Uninstall

- **macOS.** Open the **AImanity** menu and choose **Uninstall AImanity…**.
- **Windows.** Go to **Settings → Apps → Installed apps → AImanity → Uninstall**, or run `unins000.exe` from the installation folder.

Uninstalling deletes both virtual devices, chats, settings, and work files. Backups saved outside the AImanity data folder stay in place.

## 卸载

- **macOS。** 打开屏幕顶部的 **AImanity** 菜单，选择 **Uninstall AImanity…**。
- **Windows。** 前往 **设置 → 应用 → 已安装的应用 → AImanity → 卸载**，也可以运行安装目录里的 `unins000.exe`。

卸载会删除两台虚拟设备、聊天、配置和工作文件。存放在 AImanity 数据目录之外的备份会保留。

[Model setup / 模型配置](model-and-channel-setup.md) · [Client development](native-client.en.md) · [客户端开发](native-client.zh-CN.md) · [Issues](https://github.com/MrMilkDragon/AImanity/issues/new/choose)
