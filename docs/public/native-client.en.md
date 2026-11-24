# Control a computer in Fluorine

[简体中文](native-client.zh-CN.md) · [Set up the workspace](getting-started.en.md)

Fluorine 0.0.6 opens with zero devices. One conversation and task queue serve your authorized devices. Add virtual Android or Linux guests when you need them. The Agent can choose a connected device by its foreground app, observe its screen and act on that device. Working files and the knowledge base stay with the workspace.

## Use this computer for one task

Use this when the work is on the same Mac or Windows PC that runs your workspace.

1. Open the settings gear and enable **Allow host desktop control**.
2. In the conversation composer, select the monitor icon **Use this computer**, then **Allow for this task**. On macOS, grant AImanity Screen Recording and Accessibility access when prompted.
3. Send the instruction, for example “Type ‘Ready’ into the open test document on this computer and save it.” The grant covers the logged-in desktop across apps. The current model receives its screen observations, which are also kept in the local task record.
4. Select **End desktop access** to stop early. The grant also ends when the task finishes. A later task requires its own grant.

## Pair a remote computer

Use this when a second Windows PC, Mac or Linux X11 computer will provide its screen, keyboard, pointer and files. Open AImanity on both computers.

1. **On the workspace computer**, open **Devices → Add device → Remote computer**. Enter a name, select **Create pairing code** and copy it. This single-use code expires in five minutes.
2. **On the computer to control**, make sure SSH key login to the workspace works. Enable Remote Login on a Mac workspace or OpenSSH Server on a Windows workspace. A trusted SSH host alias or `user@host` works. From the computer to control, run `ssh my-workspace` once, verify the workspace host key and exit the session. For first-time SSH setup, follow [Apple’s Remote Login guide](https://support.apple.com/guide/mac-help/allow-a-remote-computer-to-access-your-mac-mchlp1066/mac) or Microsoft’s guides to [OpenSSH Server](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_install_firstuse) and [key-based login](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement).
3. **On the computer to control**, open **Devices → Share a device**, select **This computer** and enter the workspace SSH host and pairing code. Choose **Share screen and files**. If you are sharing a Mac, grant AImanity Screen Recording and Accessibility on that Mac when prompted. On Mac or Windows, select **Share visible notifications** if you want the Agent to receive them.
4. **On the workspace computer**, select the connected device. Try typing a short line in an open text document and saving it. In **Files**, choose the paired device to browse its files or copy a saved document into working files.
5. Select **Stop sharing** on the remote computer or **Disconnect** in the workspace to end access. Create a new code to reconnect. **Remove device** forgets the pairing in the workspace.

SSH carries the protobuf WebSocket connection to the workspace listener on `127.0.0.1:17863`. If you maintain your own tunnel, run the following on the computer to control. Leave it open and enter `17869` in **Workspace SSH host or forwarded port**.

```sh
ssh -N -L 127.0.0.1:17869:127.0.0.1:17863 my-workspace
```

## Pair an Android phone

Create a **Remote Android** code in the workspace. On the desktop computer sharing the phone, connect it using USB debugging or Android wireless debugging and approve that computer on the phone. In **Share a device → Find phones**, select the authorized phone. Enter the workspace SSH host and pairing code, then select **Share screen and files**. For Chinese text, enable the AImanity input method on the phone and focus an editable field.

## Build the workbench

The client uses Rust, Iced and wgpu. The workspace stores state in embedded redb. Copilot uses the local microphone, bundled speech recognizer and system text-to-speech. Install CMake to build the recognizer.

```sh
cargo build --release --locked --manifest-path apps/native/Cargo.toml
```

[Runtime bundle layout](runtime-bundles.md) · [Platform packages](release-status.md) · [0.0.5 native client guide](https://github.com/MrMilkDragon/AImanity/blob/v0.0.5/docs/public/native-client.en.md)
