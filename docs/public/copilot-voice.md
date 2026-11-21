# Copilot voice

Enable Copilot beside Liberty in the conversation composer. Both modes can run together. Voice instructions have priority. The Copilot label turns blue, violet and rose when listening is enabled. As you speak, particles rise from the bottom of the input field and spread outward with each syllable. Nearby particles follow the pointer. While the Agent works, a low band remains visible. The particles settle when the input and Agent are quiet.

The macOS and Windows clients bundle a multilingual Whisper model. Turn on Copilot and allow microphone access. Recognition updates the draft as you speak. A pause submits the phrase to the current conversation. The system voice reads replies, and new speech interrupts playback. The microphone opens when Copilot starts and closes when it stops.

The first activation requests the system microphone. Speech recognition and playback run locally. A spoken revision joins the active user task or the notification work already in progress, so the Agent can edit the open document and continue the same delivery. Accepted corrections and transcripts use [assistant.proto](../../packages/protocol/proto/ai_phone/v1/assistant.proto).

Copilot starts reading at a natural break in a streaming reply. It keeps decimal values and version numbers together, reads link labels, and can speak a completed short reply in one utterance.

The Rust service also exposes compatible provider adapters defined in [voice.proto](../../packages/protocol/proto/ai_phone/v1/voice.proto), including binary WebSocket transcription and WAV synthesis. Voice service configuration lives under **Configuration → Agent → Voice**.

Guest voice messages, microphone injection and phone calls are on the roadmap.

## Copilot 语音

在输入区开启 Copilot。它可以和 Liberty 同时运行，语音指令优先。开启后 Copilot 字样呈蓝、紫、玫红三色。输入框底边的彩色方粒随重音升起、向两侧散开，鼠标靠近时会带起附近几粒。Agent 工作时保留一道低矮的粒子带。语音和任务都静下来后，粒子收回。

macOS 和 Windows 随包提供多语 Whisper 模型。开启 Copilot 并允许访问麦克风后，说话时草稿会更新，停顿时发送到当前会话。系统音色播报回复，说下一句会打断播报。录音、转写和播报均在本机运行。工作进行中继续说话，改稿指令会进入当前任务，Agent 可以继续编辑同一份文件并交付。

流式回复遇到自然停顿即可开口，小数和版本号连读，链接只读名称。已经收齐的短回复一次播完。
