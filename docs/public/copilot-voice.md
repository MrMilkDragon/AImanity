# Copilot voice

Enable Copilot beside Liberty in the conversation composer. Both modes can run together. Voice instructions have priority.

The macOS and Windows clients bundle a multilingual Whisper model. Turn on Copilot and allow microphone access. Recognition updates the draft as you speak. A pause submits the phrase to the current conversation. The system voice reads replies, and new speech interrupts playback. The microphone opens when Copilot starts and closes when it stops.

The first activation requests the system microphone. Speech recognition and playback run locally. A spoken revision joins the active user task or the notification work already in progress, so the Agent can edit the open document and continue the same delivery. Accepted corrections and transcripts use [assistant.proto](../../packages/protocol/proto/ai_phone/v1/assistant.proto).

The Rust service also exposes compatible provider adapters defined in [voice.proto](../../packages/protocol/proto/ai_phone/v1/voice.proto), including binary WebSocket transcription and WAV synthesis. Voice service configuration lives under **Configuration → Agent → Voice**.

Guest voice messages, microphone injection and phone calls are on the roadmap.

## Copilot 语音

在输入区开启 Copilot。它可以和 Liberty 同时运行，语音指令优先。

macOS 和 Windows 随包提供多语 Whisper 模型。开启 Copilot 并允许访问麦克风后，说话时草稿会更新，停顿时发送到当前会话。系统音色播报回复，说下一句会打断播报。录音、转写和播报均在本机运行。工作进行中继续说话，改稿指令会进入当前任务，Agent 可以继续编辑同一份文件并交付。
