# Shared Linux filesystem

The installer ships one architecture-matched `images.payload` archive. The Linux guest expands it once into a shared read-only system. The desktop uses a writable overlay and its own home. Terminal jobs use separate PID, mount and network namespaces with a writable `/workspace` and read-only `/skills`.

Chromium, Openbox, LibreOffice, fonts and Tesseract come from the signed Debian archive. PostgreSQL and Redis are separate native guest services. The packed Linux filesystem contains the desktop launch assets. The matching Rust desktop bridge is sealed alongside the native service builds and mounted when the desktop starts.

The dock opens Work Files in GNOME Files, Chromium, Writer, Calc, Impress and Draw. GNOME Files starts in list view. Plain text and Markdown open in GNOME Text Editor. Openbox manages windows and PCManFM draws the desktop wallpaper and icons.

The guest includes the native GTK 4 file chooser, Cantarell fonts and SVG icon support. GTK 3 applications use Yaru Blue. LibreOffice opens with compact tabs and Breeze icons in all four editors. These are system defaults, so personal view and toolbar preferences take precedence.

Build `arm64` on Linux ARM64 for macOS and `amd64` on Linux x64 for Windows. The builder uses debootstrap, the Debian Trixie keyring, GNU tar, wget and Zstandard.

```sh
cargo build --locked -p ai-phone-release-tools
sudo target/debug/release-tools factory-images rootfs arm64 build/linux-rootfs.tar.zst
```

The [macOS factory](macos-full-runtime.md) and [Windows factory](../windows-backend-runtime.md) consume this archive. The installer carries it with the Linux VM resources; the first launch operates offline.

## 中文

Linux 桌面和 Agent 工作区共用一份随安装包交付的只读文件系统。桌面单独保存可写层与个人文件。工作指令独立运行，可写 `/workspace`，只读 `/skills`。数据库、设备配置和聊天各自持久化。

底部 Dock 可打开工作文件、浏览器和四款办公应用。工作文件默认用 GNOME 文件管理器的列表视图打开，纯文本和 Markdown 交给 GNOME 文本编辑器。Openbox 管理窗口，PCManFM 绘制壁纸和桌面图标。

离线镜像内置 GTK 4 文件选择器、Cantarell 字体和 SVG 图标支持。GTK 3 应用采用蓝色 Yaru 主题，四款 LibreOffice 编辑器采用紧凑标签栏和 Breeze 图标。用户自选的视图和工具栏布局优先。
