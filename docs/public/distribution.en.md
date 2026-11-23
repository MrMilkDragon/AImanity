# Installing Fluorine

[简体中文](distribution.zh-CN.md)

Fluorine 0.0.6 is in development. Its distribution path is a fresh installation with a new application identity, `dev.aimanity.workspace`. When it ships, install the Fluorine client for your platform, pick a language, then configure your model in the workbench. You can begin with zero devices.

## The local workspace

The Rust client opens `host/workspace.redb` in its own application data directory. Shared conversations, queued messages, model profiles, device registrations, memories, knowledge and chat attachments use embedded transactions. Editable working files live in `host/files/`.

| Desktop | Application data directory |
| --- | --- |
| macOS | `~/Library/Application Support/dev.aimanity.workspace` |
| Windows | `%APPDATA%\dev.aimanity.workspace` |
| Linux | `$XDG_DATA_HOME/dev.aimanity.workspace` or `~/.local/share/dev.aimanity.workspace` |

An existing Europium installation keeps its own profile. The Fluorine distribution uses platform installers and starts a separate workspace.
