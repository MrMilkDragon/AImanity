# Phone control applications

The optional `phone-apps` runtime component carries AI Phone's Unicode input and notification services, no-drawer launcher and a pinned guest-ABI-specific EinkBro browser. Maintainers compile the input and launcher APKs with Android 35 tools. Initial setup installs the packaged APKs.

## Build and ownership

```sh
. ./scripts/android-env.sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo run --quiet --locked -p ai-phone-release-tools -- phone-apps build .
cargo run --locked -p ai-phone-release-tools -- client-payload \
  --with-service-runtime --with-phone-apps \
  --android-runtime /path/to/android.payload
```

The factory uses OpenSSL to create development signing keys in private PKCS#12 files under `data/android/<role>/debug.jks`. Repeated builds reuse these keys. The phone component contains the signed APKs.

The build verifies the signatures and package IDs of the launcher and input APKs. The browser is checked against the pinned archive and package ID. Cargo publishes the three APKs and their source fingerprint to `.tools/phone-apps`.

Packaging verifies the source fingerprint and file inventory before staging. `runtime/phone-apps/manifest.json` records the APKs and signer identities.

The Android runtime and phone APKs are separate requirements: Java can be omitted only when both are supplied. An emulator-only capsule without the control applications stops before device creation. Startup installs only missing packages; existing input, launcher and application data retain their current device ownership.

## Input and notification readiness

The Rust host runtime owns the serial-bound input setup lock, installation intent and permission observations. Its private length-prefixed protobuf command is `--setup-phone-input`; bundled service manifests must qualify `ai-phone-input-setup-v2` before dispatch. Source contributors build the dedicated native helper; complete clients use the bundled assistant.

Setup checks user 0, Unicode input, hardware-keyboard input, notification access and the attention accessibility service. The input APK's DUMP-protected Rust receiver reads Android's notification access and registered accessibility services through JNI. The Cargo builder emits its Android entrypoint and includes both ARM64 and x64 libraries. Setup repairs missing capabilities while retaining other input methods and accessibility services.

Existing `input-setup.pending.json` records remain readable. An installation intent is synced before the only install attempt; an unknown result is observed before recovery, and an absent package with an unresolved intent is not reinstalled. Existing APKs and application data are preserved. No model request, per-action reflection or database query participates in this bootstrap. These checks establish input/notification capabilities, not successful delivery of a real notification to an Agent or a complete host installation.

Offline setup installs the paged home screen and EinkBro browser. New phones show the browser, Settings and Files on the home screen. Apps installed later join the same screen. Input and notification services use the same serial lease. Existing apps, their data and the selected home are preserved.

EinkBro binaries, exact hashes, signer and source revision are pinned in [the browser policy](../../../config/phone-browser.json). Only the guest ABI is packaged. The new baseline uses Android’s system WebView. The previous development phone keeps its installed browser; the new baseline has not been installed there. Fresh-device setup installs a missing browser and preserves an existing one. New installers still require complete runtime and redistribution qualification.

## Availability

Current platform support, installer availability and distribution requirements are maintained in [release status](../release-status.md).

---

## 中文

手机控制组件包含 Unicode 输入/通知服务、无抽屉桌面及固定版本 EinkBro 浏览器的预构建 APK。构建阶段使用 Java 和 Android 工具，用户初始化时校验并安装随包 APK，不现场编译。独立开发签名保存在忽略的构建目录，不复用现有手机上的私有签名文件；完整生产签名与升级审查仍待完成。

Rust 管理输入初始化、安装记录和权限设置。输入 APK 接收受 DUMP 权限保护的请求，通过 Android 系统查询通知授权与已登记的无障碍服务。初始化修复缺失权限，并保留其他输入法和无障碍服务。

首装在同一串行锁内安装分页桌面和 EinkBro 浏览器。新手机在桌面直接显示浏览器、设置和文件管理，后来安装的应用自动加入桌面。已有应用、数据及桌面选择继续保留。

组件同时绑定源指纹、包名、签名摘要和文件摘要，损坏时明确停止，不回退编译。只有模拟器和两个 APK 都齐备才跳过 Java；仅有模拟器会在创建设备前停止。启动流程继续只安装缺失应用，保留已安装应用与数据。

APK 交付、设备权限激活与完整初始化是独立环节。当前平台范围、签名与分发条件统一见[发行状态](../release-status.md)。
