# Protocol boundaries

The native client sends local service requests as `application/x-protobuf`. Cargo compiles the schemas in `packages/protocol/proto` with Prost. The host and its Linux guest exchange length-prefixed protobuf messages during startup.

The shared chat subscribes to protobuf changes at `/v1/chat/events` before reading its first history page. A page contains up to 80 messages plus selected run heads. Queued changes share one refresh per batch. Typing and Copilot use the same send endpoint, which returns the committed message. Reconnection reads the current page. Older pages use a conversation-scoped cursor.

The media service streams JPEG frames and PCM audio. The Linux desktop bridge accepts JSON commands and returns protobuf observations with PNG evidence. Model adapters use each provider’s JSON and streaming protocol. The optional speech service transports PCM and transcript events over a protobuf WebSocket at `/v1/voice/stream`.
