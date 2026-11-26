# Gallium · 开发与验收

## 0.0.7 本轮边界

- 发行 Apple Silicon macOS。0.0.6 版仍保留在历史发行记录中，取消准备中的 fix1，0.0.6 没有可下发的应用内更新入口。
- 工作台只呈现本机和自己创建的 Android、Linux。保留虚拟设备内部控制通道，移除远程设备配对、SSH 入口以及外部机器人设置。设备通知继续驱动工作台。
- 先验收中断后的下一条消息、Android 输入组件恢复、Linux 首帧、离线语音和计划触发，再使用签名 DMG 验收首装和旧工作区重开。

## 截至本轮的实现记录

- 调度原来把远程配对作为指定设备执行条件。现以已连接的设备会话和设备状态判断。停止信号会抢占旧运行的等待，停止后唤醒队列。
- Android 输入修复先从包管理器确认实际版本。组件未安装时允许下一次显式修复使用同一离线组件重新安装。Linux 在连接后等待有效的 Protobuf 首帧，保留真实进程失败信息。设备启动阶段上画布，显示正在安装或连接。
- 位置的开关和手选坐标先保存。每个运行实例独立接收，失败或未启动出现在各自状态里。
- 计划支持自然语言整理为可确认的时间或通知触发条件与任务。已保存计划进入同一调度队列。工作台打开时使用 macOS 防空闲休眠断言，显示器仍可熄屏。
- 会话默认折叠为正在执行的步骤或结果，点开可看工具事件和回执。输入区用圆形发送键和圆形胶囊外框，悬浮展开并保留草稿。沿用本地图片上传、预览和对话附件。Copilot 更换为多语 Whisper Base Q5_1。
- 通过已登录 Safari 查看了原帖 A/B 动画。采用彩色点阵向本次转写文本末端汇聚的交互，沿用 Gallium 的白色输入框和现有色板。

## 0.0.8 可行性判断

应用目录、运行状态、窗口和文件视图可以统一。不同格式仍需要各自系统接口。DMG 是一种磁盘镜像，挂载后交给 macOS 安装并运行 Apple Silicon 兼容应用。APK 需要 Android 系统服务与图形运行环境，保留按需启动的轻量来宾。DEB 安装到共享的最小 ARM Linux 运行环境，多个 Linux 应用共用该实例。EXE 先逐应用验证 Wine 的兼容性；确需完整 Windows API 的应用走独立环境。支持 Mac 的 iPhone/iPad 应用走 Apple Silicon 上发行方开放的 Mac App Store 渠道。

以 Rust 管理应用目录、安装来源、运行状态、窗口和文件路径。用同一 redb 数据库记录应用安装、版本、权限和作用域，每个应用有独立数据目录。使用运行时提供的真实窗口或 GPU 画面。停用应用时停止进程，最后一款应用退出后可休眠其运行环境。常驻仅保留界面、任务队列、模型连接和轻量的运行监督。左侧蜂窝展示应用图标和运行点，启动后用原生缩放过渡展开。安装包拖放走同一个导入入口，各运行器处理平台签名、安装和卸载。

原生 Rust 应用可以先定义 `.aimapp` 包。包内是 manifest、资源和按平台编译的原生程序。根据包的声明授予工作区访问范围和应用能力。先做 DMG/原生应用与统一目录的原型并测首帧、内存和磁盘，再决定引入哪些兼容运行环境。

参考资料：[Apple Virtualization](https://developer.apple.com/documentation/virtualization) · [Apple Silicon 上的 iPhone/iPad 应用](https://developer.apple.com/help/app-store-connect/manage-your-apps-availability/manage-availability-of-iphone-and-ipad-apps-on-macs-with-apple-silicon/) · [AOSP 架构](https://source.android.com/docs/core/architecture) · [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
