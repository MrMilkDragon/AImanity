# Install AImanity / 安装 AImanity

## 0.0.6 Fluorine

Fluorine opens an empty workspace. Add a virtual Android or Linux device when needed, pair a remote computer or phone, or grant the current task access to the host desktop. Install 0.0.6 as a fresh client. [Workspace guide](native-client.en.md)

Fluorine 首次打开为空工作区。可以按需创建虚拟设备，配对其他电脑或手机，也可单独授权当前任务使用宿主机。0.0.6 使用全新安装。[工作区指南](native-client.zh-CN.md)

| Platform | Verified before installer acceptance | Physical hardware coverage still open |
| --- | --- | --- |
| Apple Silicon macOS | TextEdit on this Mac, local and paired actions, Chinese input saved to disk | Pairing between two separate physical computers |
| Windows 11 x64 target | Notepad and zero-device reopen on a hosted Windows Server desktop | A physical Windows 11 machine and its audio devices |
| Linux x86_64 (DEB and RPM) | Embedded workspace, device lifecycle and offline tools on a hosted Linux runner | Physical desktop audio and GPU configurations |
| Android phones and tablets | One candidate APK on two Android 15 emulators, landscape input, draft on resume and cold reopen | ARM64 AVF guest creation, simultaneous recordings and playback on a physical phone or tablet |

iPhone and iPad are scheduled for 0.0.7. Installer acceptance uses the signed artifacts listed on the release page as they are published.

## 0.0.5 Europium downloads

Each installer below includes Android, the Linux desktop and the local services.

| Platform | Requirements | Installer | Factory |
| --- | --- | --- | --- |
| macOS | Apple Silicon, macOS 15+ | [0.0.5 Europium · DMG](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) | [macOS](engineering/macos-full-runtime.md) |
| Windows | Windows 11 x64, hardware virtualization | [0.0.5 Europium · EXE](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) | [Windows](windows-client-factory.md) |
| Debian / Ubuntu | x86_64, glibc 2.36+, KVM | [0.0.5 Europium · DEB](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) | [Linux workflow](../../.github/workflows/linux-packages.yml) |
| Fedora / Enterprise Linux 10 | x86_64, glibc 2.36+, KVM | [0.0.5 Europium · RPM](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm) | [Linux workflow](../../.github/workflows/linux-packages.yml) |

Open the app, choose your device settings, then add a model in Configuration. The macOS app uses an ad hoc signature. The Windows installer uses self-signed Authenticode.

- **macOS.** If macOS reports an unidentified developer, open **System Settings → Privacy & Security → Open Anyway** after trying to launch AImanity. [Apple first-launch help](https://support.apple.com/en-us/102445)
- **Windows.** If SmartScreen shows **Windows protected your PC**, choose **Run anyway** when offered for the EXE downloaded from the Releases link above. [Microsoft SmartScreen help](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation#certificate-options-and-their-smartscreen-implications)

安装包内置 Android、Linux 桌面和本机服务。打开客户端，配置两台设备，然后在配置中心添加模型。macOS 采用 ad hoc 签名，Windows 采用自签名 Authenticode。

- **macOS。** 如果系统提示开发者身份无法验证，尝试打开 AImanity 后，进入 **系统设置 → 隐私与安全性 → 仍要打开**。[Apple 首次打开指南](https://support.apple.com/zh-cn/102445)
- **Windows。** 如果 SmartScreen 提示 **Windows 已保护你的电脑**，从上方发行页下载 EXE 后，可在出现 **仍要运行** 时选择该项。[微软 SmartScreen 指南](https://learn.microsoft.com/zh-cn/windows/apps/package-and-deploy/smartscreen-reputation#certificate-options-and-their-smartscreen-implications)

## Linux

Use a desktop session with read/write access to `/dev/kvm`. In the download directory, run the command for your distribution. The package manager installs any missing system libraries. The Android and Linux images, QEMU and ADB come with AImanity.

请在能读写 `/dev/kvm` 的桌面会话中使用。在下载目录运行对应的命令，包管理器会补齐所需系统库。Android 与 Linux 镜像、QEMU 和 ADB 均随包提供。

**Debian 12+ / Ubuntu 24.04+**

```sh
sudo apt install ./AImanity-0.0.5-linux-amd64.deb
```

**Fedora / Enterprise Linux 10**

```sh
sudo dnf install ./AImanity-0.0.5-linux-x86_64.rpm
```

Launch **AImanity** from the applications menu. To update, quit AImanity and install the newer DEB or RPM with the same command and the new filename. Devices, chats and model settings stay in your user profile. Optional control of the host desktop requires an X11 session.

从应用菜单打开 **AImanity**。更新时退出应用，使用相同命令安装同格式的新版软件包，将文件名换为新版文件名。设备、聊天和模型配置保存在个人目录中。需要授权 Agent 操作宿主机桌面时，请使用 X11 会话。

## Updating macOS and Windows from 0.0.4 / macOS 和 Windows 从 0.0.4 更新

Open the update icon at the top right of the 0.0.4 workbench and confirm the update. AImanity downloads the installer, restarts and reopens with your existing devices, conversations and model settings.

在 0.0.4 工作台右上角打开更新图标，选择「确认更新」。AImanity 会下载安装包并重启，原有设备、聊天和模型配置会保留。

## Moving from Calcium

Back up any devices, configuration and conversations you want to keep, uninstall 0.0.3, then install the version listed for your platform above.

从 Calcium 迁移时，请先备份要保留的设备、配置和聊天，再卸载 0.0.3，通过上表安装对应平台的版本。

## Uninstall

- **macOS.** Open the **AImanity** menu and choose **Uninstall AImanity…**.
- **Windows.** Go to **Settings → Apps → Installed apps → AImanity → Uninstall**, or run `unins000.exe` from the installation folder.
- **Linux.** Select **Uninstall AImanity** in the workbench settings to remove the package and your local data. Removing only the system package with apt or dnf retains your user profile.

The app’s uninstall action deletes its managed devices, chats, settings and work files. Backups saved outside the AImanity data folder stay in place.

## 卸载

- **macOS。** 打开屏幕顶部的 **AImanity** 菜单，选择 **Uninstall AImanity…**。
- **Windows。** 前往 **设置 → 应用 → 已安装的应用 → AImanity → 卸载**，也可以运行安装目录里的 `unins000.exe`。
- **Linux。** 在工作台设置中选择 **卸载 AImanity**，移除软件包和当前用户的数据。使用 apt 或 dnf 单独移除软件包会保留个人数据。

通过应用卸载会删除受其管理的设备、聊天、配置和工作文件。存放在 AImanity 数据目录之外的备份会保留。

[Model setup / 模型配置](model-and-channel-setup.md) · [Client development](native-client.en.md) · [客户端开发](native-client.zh-CN.md) · [Issues](https://github.com/MrMilkDragon/AImanity/issues/new/choose)
