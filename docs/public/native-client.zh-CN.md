# 在 Fluorine 中操作电脑

[English](native-client.en.md) · [工作区初次配置](getting-started.zh-CN.md)

0.0.6 支持零设备启动，所有已授权设备共用一个对话和待办队列。需要时可创建 Android 或 Linux 虚拟设备。Agent 可以根据已连接设备的前台应用选设备，查看画面后执行操作。工作文件和知识库属于整个工作区。

## 在本机执行一次任务

适用于工作文件或应用就在运行 AImanity 工作区的这台 Mac 或 Windows 电脑上。

1. 打开右上角设置齿轮，开启「允许本机协作」。
2. 在对话输入区点击显示器图标「本机协作」，选择「允许本次任务」。macOS 提示时，在系统设置中授予 AImanity 屏幕录制和辅助功能权限。
3. 发送指令，例如「在这台电脑已打开的测试文稿中输入『已核对』并保存」。本次授权涵盖已登录桌面的多个应用。当前模型会收到观察到的画面，画面也会保存在本地任务记录中。
4. 可随时点击「结束本机协作」，任务结束时授权也随之结束。下次操作本机需重新授权。

## 配对远程电脑

适用于另一台 Windows、macOS 或 Linux X11 电脑。两台电脑都打开 AImanity。以下将发出任务的一端称为工作区电脑。

1. **在工作区电脑上**，打开「设备 → 添加设备 → 远程电脑」，填写名称，点击「生成配对码」。配对码一次有效，五分钟内使用。
2. **在被控电脑上**，确认能通过 SSH 密钥登录工作区。工作区是 Mac 时开启「远程登录」，是 Windows 时启用 OpenSSH Server。可以填写已信任的 SSH 别名或 `用户名@主机`。首次连接可在被控电脑运行 `ssh my-workspace`，核对工作区主机指纹，登录成功后退出。首次设置可参考 [Mac 远程登录](https://support.apple.com/guide/mac-help/allow-a-remote-computer-to-access-your-mac-mchlp1066/mac)、[Windows OpenSSH 服务](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_install_firstuse)及[密钥登录](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement)。
3. **在被控电脑上**，打开「设备 → 共享设备」，选择「这台电脑」，填写工作区 SSH 主机和配对码，点击「共享屏幕与文件」。被控电脑是 Mac 时，在这台 Mac 上按提示授予 AImanity 屏幕录制和辅助功能权限。若希望 Agent 收到该 Mac 或 Windows 电脑上的可见通知，可勾选「共享可见通知」。
4. **回到工作区电脑**，点选已连接设备。可以先让 Agent 在打开的文稿中输入一行字并保存，再到「文件管理」选中该设备，把文稿复制到工作文件。
5. 在被控端点击「停止共享」，或在工作区点击「断开」即可结束访问。重连时生成新码。「移除设备」会删除工作区内的配对记录。

SSH 将 protobuf WebSocket 连接送到工作区的 `127.0.0.1:17863`。如果已有自己的隧道，可以在被控电脑执行以下命令并保持连接，然后在「工作区 SSH 主机或本地转发端口」中填 `17869`。

```sh
ssh -N -L 127.0.0.1:17869:127.0.0.1:17863 my-workspace
```

## 配对 Android 真机

在工作区生成「远程 Android」配对码。在提供手机的电脑上通过 USB 调试或无线调试连接手机，并在手机上授权这台电脑。打开「共享设备 → 查找手机」，选中设备，填入工作区 SSH 主机和配对码，点击「共享屏幕与文件」。输入中文时，请在手机上启用 AImanity 输入法，并先点入可编辑的输入框。

## 构建工作台

客户端采用 Rust、Iced 和 wgpu，工作区使用内嵌 redb。Copilot 使用本机麦克风、离线语音识别器和系统语音播报。编译语音识别器需要 CMake。

```sh
cargo build --release --locked --manifest-path apps/native/Cargo.toml
```

[运行包结构](runtime-bundles.md) · [平台安装包](release-status.md) · [0.0.5 原生客户端指南](https://github.com/MrMilkDragon/AImanity/blob/v0.0.5/docs/public/native-client.zh-CN.md)
