# Distribution requirements / 发行标准

Fluorine 0.0.6 targets four hosts and five offline installers. Apple Silicon macOS uses a DMG, Windows 11 x64 an EXE, and Linux x86_64 a DEB or RPM. Android phones and tablets share one signed landscape APK. Start the workspace with zero devices, then create Android or Linux guests from the bundled templates. Install 0.0.6 as a new client.

Keep user state separate from application versions. Build installers from clean images and source. Include third-party notices, source provenance and applicable corresponding source. Review each Android SDK component under its own terms. [Android SDK terms](https://developer.android.com/studio/terms)

GitHub accepts release assets under 2 GiB. Users download a complete installer. [GitHub release limits](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)

## 中文

Fluorine 0.0.6 覆盖四类宿主、五个离线安装包。Apple Silicon macOS 提供 DMG，Windows 11 x64 提供 EXE，Linux x86_64 提供 DEB 和 RPM，Android 手机和平板共用一个签名横屏 APK。全新安装后可直接进入空工作区，按需使用内置模板创建 Android 或 Linux 设备。

用户数据与程序版本分开保存。使用干净镜像和源码构建，附上第三方声明、来源和适用的对应源码。GitHub 单附件小于 2 GiB，用户下载一个完整安装包。
