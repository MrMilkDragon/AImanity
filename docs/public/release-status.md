# Install AImanity / 安装 AImanity

## 0.0.6 Fluorine

[Download macOS DMG](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.6/AImanity-0.0.6-macos-arm64.dmg) · [All 0.0.6 installers](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6) · [下载及逐端安装验收](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6)

Fluorine opens an empty workspace. Add a virtual Android or Linux device when needed, pair a remote computer or phone, or grant the current task access to the host desktop. Install 0.0.6 as a fresh client. [Workspace guide](native-client.en.md)

Fluorine 首次打开为空工作区。可以按需创建虚拟设备，配对其他电脑或手机，也可单独授权当前任务使用宿主机。0.0.6 使用全新安装。[工作区指南](native-client.zh-CN.md)

| Platform | Verified on | Further hardware coverage |
| --- | --- | --- |
| Apple Silicon macOS | Signed DMG on an Apple Silicon Mac. The offline workbench opened from a fresh profile, saved English and reopened the same database in 1.402 s. Local and paired TextEdit input was checked on disk. | Pairing between two separate physical computers |
| Windows 11 x64 target | Notepad and zero-device reopen on a hosted Windows Server desktop | A physical Windows 11 machine and its audio devices |
| Linux x86_64 (DEB and RPM) | Embedded workspace, device lifecycle and offline tools on a hosted Linux runner | Physical desktop audio and GPU configurations |
| Android phones and tablets | One candidate APK on two Android 15 emulators, landscape input, draft on resume and cold reopen | ARM64 AVF guest creation, simultaneous recordings and playback on a physical phone or tablet |

**Workspace features under development.** The Skills window can enable or disable bundled skills. Importing a custom SKILL.md from a file or URL still lacks a native workspace handler. The external messaging channel list, binding, pairing and removal also await their native handlers. Computer and Android device pairing use a separate, working connection.

**工作区功能进度。** 内置 Skill 的启停和重开保存已经验证。自定义 SKILL.md 的文件和网址导入，以及外部聊天渠道的列表、绑定、配对和解绑，仍需接入原生工作区。电脑和 Android 设备的配对使用独立通道，已有实际连接回执。

iPhone and iPad are scheduled for 0.0.7. Installer acceptance uses the signed artifacts listed on the release page as they are published.

## Installation / 安装

- **macOS 15+ on Apple Silicon.** Open the DMG, drag AImanity to Applications and open it. This release uses an ad hoc signature. Apple describes the first-launch control in Privacy & Security. [Apple guide](https://support.apple.com/en-us/102445)
- **Windows 11 x64.** Run the signed setup EXE. Enable hardware virtualization to create guests. Windows packages use a self-signed Authenticode certificate. [Windows client guide](windows-client-factory.md)
- **Android 15+.** Use the same APK for phones and tablets. The workbench runs in landscape. Local guest creation needs AVF custom VM access. [Device guide](native-client.en.md)

打开客户端后选择语言，在配置中心添加模型。桌面端和安卓端都可以从空工作区开始。[设备接入、主机授权与远程配对教程](native-client.zh-CN.md)

## Linux

In your download directory run the command for your distribution. The installers include Android, Linux, QEMU and ADB. Creating local virtual devices requires read/write access to `/dev/kvm`.

在下载目录运行对应的安装命令。安装包自带 Android、Linux 镜像、QEMU 和 ADB。创建本地虚拟设备需要读写 `/dev/kvm`。

**Debian 12+ / Ubuntu 24.04+**

```sh
sudo apt install ./AImanity-0.0.6-linux-amd64.deb
```

**Fedora / Enterprise Linux 10**

```sh
sudo dnf install ./AImanity-0.0.6-linux-x86_64.rpm
```

Launch **AImanity** from the applications menu. Optional control of the host desktop requires an X11 session.

从应用菜单打开 **AImanity**。需要授权 Agent 操作宿主机桌面时，请使用 X11 会话。

## Earlier versions / 历史版本

[0.0.5 Europium downloads](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.5) · [0.0.4 → 0.0.5 update guide](https://github.com/MrMilkDragon/AImanity/blob/v0.0.5/docs/public/release-status.md)

## Uninstall

- **macOS.** Open the **AImanity** menu and choose **Uninstall AImanity…**.
- **Windows.** Go to **Settings → Apps → Installed apps → AImanity → Uninstall**, or run `unins000.exe` from the installation folder.
- **Android.** Uninstall AImanity through Android’s application settings.
- **Linux.** Select **Uninstall AImanity** in the workbench settings to remove the package and your local data. Removing only the system package with apt or dnf retains your user profile.

The app’s uninstall action deletes its managed devices, chats, settings and work files. Backups saved outside the AImanity data folder stay in place.

## 卸载

- **macOS。** 打开屏幕顶部的 **AImanity** 菜单，选择 **Uninstall AImanity…**。
- **Windows。** 前往 **设置 → 应用 → 已安装的应用 → AImanity → 卸载**，也可以运行安装目录里的 `unins000.exe`。
- **Android。** 打开系统的应用信息，选择卸载 AImanity。
- **Linux。** 在工作台设置中选择 **卸载 AImanity**，移除软件包和当前用户的数据。使用 apt 或 dnf 单独移除软件包会保留个人数据。

通过应用卸载会删除受其管理的设备、聊天、配置和工作文件。存放在 AImanity 数据目录之外的备份会保留。

[Model setup / 模型配置](model-and-channel-setup.md) · [Client development](native-client.en.md) · [客户端开发](native-client.zh-CN.md) · [Issues](https://github.com/MrMilkDragon/AImanity/issues/new/choose)
