# Native client development

[简体中文](native-client.zh-CN.md)

## 0.0.4 Dysprosium development

`apps/native` contains the Iced + wgpu workbench. Its Rust host integration starts the phone, desktop and local services. Device frames upload directly to GPU textures; input and the shared conversation use the existing protobuf services.

The macOS development build connects real devices and preserves one conversation across device switches. Its anchored Agent editor uses the shared provider catalogue and the existing model verification/save flow. Saved profiles include connection, memory and voice settings.

The conversation supports Markdown, older-message pagination, expandable tool receipts, file selection/drop, image previews, downloads, pause, resume and stop. Pending messages are stored locally as protobuf before submission and retain their request IDs when retried. New drafts remain independent of in-flight messages. Rendering caches parsed Markdown and decoded images; idle conversations do not poll tool history.

The native component system shares typography, radii, 40px control heights and glass parameters. Anchored windows measure their content and animate their size; covered device and chat controls receive no pointer input. Device and anchored-window transitions use window frame events. Tool-history polling runs only for the current running task with its process expanded, with one final fetch when it ends.

The global toolbar opens the language selector, Skill manager, simulated location, workbench proxy, knowledge graph and document editor, three-pane file manager, and device/Agent settings as anchored panels. Open Task & Goal beside the attachment button in the shared composer. The device toolbars control native playback volume, rotation and screenshots. Each device has an independent audio stream that continues across focus changes until muted.

Liberty and Task & Goal use the shared assistant's persisted mode state. Copilot captures the default microphone, uses the bundled offline recognizer, and reads replies with the system voice. Partial recognition appears in the composer; final text enters the shared message outbox. Building the native client requires CMake for the bundled recognizer.

Install Dysprosium from Releases on macOS or Windows. It opens its own data directory and receives later native releases through its updater. The Windows host starts the bundled Android and WHPX desktop and relays local protobuf services over authenticated loopback.

```sh
cargo build --release --manifest-path apps/native/Cargo.toml
apps/native/target/release/aimanity-native /absolute/path/native-launch.json
```

The private launch file selects an installation with `home`, `resource`, `request`, `source`, `android` and `images`. It reuses the host runtime's offline component identifiers and device setup request. Credentials and device state stay in that installation. See the [runtime bundle layout](runtime-bundles.md).

The platform factory packages the Rust workbench with the complete offline payload. The Windows factory runs on Windows 11 x64.

## Released client

The Rust client runs setup and opens the workbench when the local services are ready. macOS uses Avo for Android and Lima for Linux. Windows and Linux run the bundled Android and Linux guests under QEMU, with WHPX on Windows and KVM on Linux.

The setup screen supports ten languages, keyboard-accessible selectors, retry and cancellation. Return to setup through **AImanity → Setup**.

## Build

```sh
cargo build --release --locked --manifest-path apps/native/Cargo.toml
```

The macOS, Windows and Linux factories add the offline systems and services to the native binary. [macOS factory](engineering/macos-full-runtime.md) · [Windows factory](windows-client-factory.md) · [Linux workflow](../../.github/workflows/linux-packages.yml)

[Downloads and host requirements](release-status.md)

## Share a phone or computer (Fluorine development)

1. In **Settings → Devices**, add **Remote Android** or **Remote computer**, name it and copy its pairing code. The code lasts five minutes.
2. On the computer providing the device, open **Settings → Devices → Share a device**. Select **This computer**, or use **Find phones** to select a phone already authorized for USB debugging or wireless ADB. The desktop app can open this window independently with `aimanity-native --share-device en`.
3. Enter the workspace computer’s trusted SSH alias and the pairing code. With an existing SSH tunnel, forward the workspace’s loopback port 17863 to a different local port, such as 17869, and enter that local port. Choose **Start sharing**. Desktop sharing supports Windows, macOS and Linux X11.
4. Select a device to see and control its screen. The shared Agent uses `device_list` to see the foreground app or window on each authorized device, then observes the chosen screen before acting. **Stop sharing** at the source or **Disconnect** in the workspace ends access. Reconnecting requires a new code.

Android supports taps, long presses, swipes, scrolling, navigation keys and printable ASCII input through ADB. A computer shares its primary display, pointer and keyboard. Desktop text entry uses the foreground window observed before the action. Use File Manager to transfer files between working files and paired devices. Guests with an audio channel expose separate microphone and speaker streams. The local workspace desktop keeps its per-task consent flow.

An Android gateway without a graphical session can run `aimanity-native --share-android <ssh-host-or-local-port> <adb-serial>`. Supply the pairing code on standard input.
