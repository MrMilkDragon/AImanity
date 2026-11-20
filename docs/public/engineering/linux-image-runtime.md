# Shared Linux filesystem

The installer ships one architecture-matched `images.payload` archive. The Linux guest expands it once into a shared read-only system. The desktop uses a writable overlay and its own home. Terminal jobs use separate PID, mount and network namespaces with a writable `/workspace` and read-only `/skills`.

Chromium, Openbox, LibreOffice, fonts and Tesseract come from the signed Debian archive. PostgreSQL and Redis are separate native guest services. The packed Linux filesystem contains the desktop launch assets. The matching Rust desktop bridge is sealed alongside the native service builds and mounted when the desktop starts.

The floating dock opens Work Files, Chromium, Writer, Calc, Impress and Draw. Running windows and the system tray share the dock. Openbox, GTK and PCManFM load the desktop settings and wallpaper from the offline image.

Build `arm64` on Linux ARM64 for macOS and `amd64` on Linux x64 for Windows. The builder uses debootstrap, the Debian Trixie keyring, GNU tar, wget and Zstandard.

```sh
cargo build --locked -p ai-phone-release-tools
sudo target/debug/release-tools factory-images rootfs arm64 build/linux-rootfs.tar.zst
```

The [macOS factory](macos-full-runtime.md) and [Windows factory](../windows-backend-runtime.md) consume this archive. The installer carries it with the Linux VM resources; the first launch operates offline.

## 中文

Linux 桌面和 Agent 工作区共用一份随安装包交付的只读文件系统。桌面单独保存可写层与个人文件。工作指令独立运行，可写 `/workspace`，只读 `/skills`。数据库、设备配置和聊天各自持久化。

底部悬浮 Dock 集中工作文件、浏览器与四款办公应用，也可切回正在运行的窗口。窗口主题、文件管理器和壁纸随离线镜像交付。
