# Fluorine architecture

[简体中文](architecture.zh-CN.md)

The Iced and wgpu client opens a local workspace after language selection. The workbench, storage and model worker run in one process. A workspace can start with zero devices.

| Component | Implementation |
| --- | --- |
| Shared conversation, registry and queue | `apps/native/src/workspace` stores protobuf records in an embedded redb database. |
| Model execution | `crates/model` streams Chat Completions, Responses and Anthropic Messages. The workbench sends requests to the selected model service. |
| Knowledge and files | The workspace stores linked documents and immutable chat attachments. Working files have editable copies on disk. |
| Devices | The registry gives each device its own ID. Connected devices use separate control and media transports. |
| Host desktop | A user grant belongs to a single request. Screen capture and input check that grant when used. |

Submitting a message commits the user message, assistant reply slot and job together. The worker wakes after commit. The same request ID refers to the same message after a restart. Quitting cancels active model I/O and leaves unfinished jobs on disk. Conversation views receive change notifications in process.

The development build runs shared chat, local files, knowledge search and model selection. Virtual and paired device execution, proactive modes and parallel task handoffs are being ported to this workspace.

[Workspace files and installation](distribution.en.md) · [Back up a workspace](backup-restore.en.md) · [Europium architecture](https://github.com/MrMilkDragon/AImanity/blob/v0.0.5/docs/public/architecture.en.md)
