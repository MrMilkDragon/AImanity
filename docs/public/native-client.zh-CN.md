# 原生客户端开发

[English](native-client.en.md)

## 0.0.4 Dysprosium 开发

`apps/native` 是 Iced + wgpu 原生工作台。Rust 宿主入口启动手机、电脑和本地服务。设备画面直接上传 GPU 纹理，输入和共享对话复用既有 protobuf 服务。

当前 Mac 开发构建已接通真实双设备，切换设备时保持唯一共享会话。就地展开的 Agent 编辑器复用服务商目录和模型检测/保存流程；已保存配置包含连接应用、记忆和语音设置。

共享会话支持 Markdown、历史分页、工具回执展开、文件选择/拖入、图片预览、下载、暂停、继续和停止。待发送消息先以 protobuf 保存到本机，重试保留原请求 ID；正在编辑的新草稿与已提交消息相互独立。Markdown 和解码图片按内容缓存，闲置会话不轮询工具记录。

原生组件共用字体、圆角、40px控件高度和玻璃参数。就地窗口按实际内容尺寸平滑收放，被覆盖的设备和聊天不接收指针输入；设备与窗口过渡由窗口帧驱动。工具记录仅在当前任务运行且过程展开时轮询，结束后再读取最终回执。

顶部工具栏就地展开语言、Skill、模拟位置、工作台代理、知识库关系图及文档、三栏文件管理和设备／Agent 设置。在共享输入框的附件按钮左侧可展开「任务与目标」。设备工具栏可控制音量、旋转及截图。两台设备分别播放音频，切换前台后继续播放，直至静音。

Liberty 与 Task & Goal 读取共享助手已保存的模式状态。Copilot 采集默认麦克风，使用随包离线模型转写，再以系统声音播报。转写草稿在对话区实时显示，完成后的文本进入共享发送队列。构建原生客户端需要 CMake，以编译本机识别器。

在 macOS 或 Windows 上从 Releases 手动安装 Dysprosium。原生版使用独立的数据目录，并在客户端内接收后续原生版本。Windows 宿主启动随包的 Android 和 WHPX 桌面，通过经过认证的回环连接转发本地 protobuf 服务。

```sh
cargo build --release --manifest-path apps/native/Cargo.toml
apps/native/target/release/aimanity-native /absolute/path/native-launch.json
```

私有启动文件通过 `home`、`resource`、`request`、`source`、`android`、`images` 指定部署位置、离线组件标识和设备配置请求。凭据和设备状态保存在该部署目录。组件结构见[运行包布局](runtime-bundles.md)。

平台工厂打包 Rust 原生工作台与完整离线载荷。Windows 工厂在 Windows 11 x64 上运行。

## 已发行客户端

Rust 原生客户端负责本机配置，在本地服务就绪后打开工作台。macOS 通过 Avo 运行 Android，通过 Lima 运行 Linux。Windows 和 Linux 宿主分别使用 WHPX 和 KVM，为 QEMU 中的双设备提供硬件加速。

配置界面支持十种语言、键盘操作、重试和取消。通过 **AImanity → 设置** 返回配置。

## 构建

```sh
cargo build --release --locked --manifest-path apps/native/Cargo.toml
```

各平台工厂在原生二进制文件之外打包离线系统与服务。[macOS 工厂](engineering/macos-full-runtime.md) · [Windows 工厂](windows-client-factory.md) · [Linux 工作流](../../.github/workflows/linux-packages.yml)

[下载与宿主要求](release-status.md)

## 共享手机或电脑（Fluorine 开发版）

1. 在工作区的「配置中心 → 设备」添加「远程 Android」或「远程电脑」，命名并复制配对码。配对码五分钟有效。
2. 在提供设备的电脑上打开「配置中心 → 设备 → 共享设备」。选择「这台电脑」，或点「查找手机」，选择已经授权 USB 调试或无线 ADB 的 Android。单独打开共享窗口可运行 `aimanity-native --share-device zh-CN`。
3. 填入已建立信任的工作区 SSH 主机别名和配对码，点击「开始共享」。已有 SSH 隧道时，将工作区回环地址的 17863 端口转发到另一个本机端口（如 17869），填入该本机端口。远程电脑可共享 Windows、macOS 或 Linux X11 桌面。配对后设备管理会显示被控端的实际系统。
4. 在工作区选中设备即可查看画面，使用键盘、左右键、滚轮和拖拽。Windows 文件管理的根目录列出盘符。Agent 通过 `device_list` 读取各台已授权设备的前台应用或窗口，选中后依据实际画面操作。在被控端点击「停止共享」，或在工作区点击「断开」，都会结束授权。重连时生成新的配对码。

Android 支持点按、长按、滑动、滚动、导航键和可打印 ASCII 文本。电脑支持主显示器画面、鼠标和键盘，文字输入绑定到观察时的前台窗口。远程设备的声音和文件传输待接入。操作工作区所在电脑沿用逐任务的本机授权。

没有图形界面的 Android 接入机可以运行 `aimanity-native --share-android <SSH主机或本机转发端口> <ADB序列号>`，通过标准输入提供配对码。
