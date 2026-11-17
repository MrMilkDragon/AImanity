# Copilot voice

Enable Copilot beside Liberty in the conversation composer. Both modes can run together. Voice instructions have priority.

The macOS and Windows clients bundle a multilingual Whisper model. Turn on Copilot and allow microphone access. Recognition updates the draft as you speak. A pause submits the phrase to the current conversation. The system voice reads replies, and new speech interrupts playback. The microphone opens when Copilot starts and closes when it stops.

The Agent receives the final text and executes through its normal tools. Accepted corrections and transcripts use [assistant.proto](../../packages/protocol/proto/ai_phone/v1/assistant.proto).

The Rust service also exposes compatible provider adapters defined in [voice.proto](../../packages/protocol/proto/ai_phone/v1/voice.proto), including binary WebSocket transcription and WAV synthesis. Voice service configuration lives under **Configuration → Agent → Voice**.

Guest voice messages, microphone injection and phone calls are on the roadmap.

## Copilot 语音

在输入区开启 Copilot。它可以和 Liberty 同时运行，语音指令优先。

macOS 和 Windows 随包提供多语 Whisper 模型。开启 Copilot 并允许访问麦克风后，说话时草稿会更新，停顿时发送到当前会话。系统音色播报回复，说下一句会打断播报。Agent 收到最终文本后通过原有工具执行。
