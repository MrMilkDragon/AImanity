# Phone control applications

The `phone-apps` runtime component carries AImanity's Unicode input and notification services, the home screen and a pinned EinkBro browser for the guest architecture. The offline installer includes all three APKs.

## Build and ownership

```sh
export ANDROID_HOME=/path/to/android-sdk
rustup target add aarch64-linux-android x86_64-linux-android
cargo run --quiet --locked -p ai-phone-release-tools -- phone-apps build .
```

Cargo builds the Rust libraries for ARM64 and x64, generates the Android entrypoints and signs the launcher and input APKs. Android 35 build tools compile and link the resources. OpenSSL creates the development signing keys under `data/android/<role>/debug.jks`, which repeated builds reuse.

The build stages the three APKs in `.tools/phone-apps`. The bundled `runtime/phone-apps/manifest.json` records the packages and signer identities.

## Input and notification readiness

The Rust host runtime coordinates setup for each device serial. It enables Unicode input, hardware-keyboard input, notification access and the attention accessibility service. The input APK reports Android's notification and accessibility permissions through a DUMP-protected Rust receiver.

New phones show EinkBro, Settings and Files on the home screen. Apps installed later join the same paged screen. The browser uses Android's system WebView. [EinkBro build and signer pins](../../../config/phone-browser.json)

## Availability

Current platform support, installer availability and distribution requirements are maintained in [release status](../release-status.md).

---

## 中文

离线安装包包含 Unicode 输入与通知服务、分页桌面和对应设备架构的 EinkBro APK。Cargo 构建 ARM64 与 x64 的 Rust 库、生成 Android 入口并完成 APK 签名，Android 35 构建工具负责编译和链接资源。

宿主按设备序列号启用输入法、键盘、通知和无障碍服务。新手机桌面显示浏览器、设置和文件，后来安装的应用自动加入分页桌面。[发行状态](../release-status.md)
