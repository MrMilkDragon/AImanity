'use client';

import {create} from '@bufbuild/protobuf';
import {ChatMessageSchema, type ChatAttachment, type ChatMessage} from '@ai-phone/protocol/ai_phone/v1/assistant_pb';
import {Download} from 'lucide-react';
import {memo, type ReactElement} from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type {OutboxEntry} from '@/lib/chat-outbox';
import {ExecutionTrace} from './execution-trace';
import {useI18n} from './locale-provider';
import {Button} from './ui/button';

export type ChatDisplayCut = {ids: string[]; runId: string; at: number; text: string};
type Delivery = OutboxEntry['status'];
const markdownPlugins = [remarkGfm];
const ChatMarkdown = memo(Markdown);

const ChatMessageRow = memo(function ChatMessageRow({message, text, traceAfterMs, runStatus, nextWakeAtMs, delivery, onPreview}: {
  message: ChatMessage;
  text: string;
  traceAfterMs: number;
  runStatus?: string;
  nextWakeAtMs: number;
  delivery?: Delivery;
  onPreview: (attachment: ChatAttachment) => void;
}) {
  const {t} = useI18n();
  return (
    <article
      data-message-id={message.id}
      className={`message message-${message.role}`}
      aria-label={message.role === 'user' ? t('你') : t('助手')}
    >
      {message.role === 'assistant' && message.runId && message.hasActivity &&
        <ExecutionTrace
          messageId={message.id}
          runId={message.runId}
          afterMs={traceAfterMs}
          active={message.active && runStatus !== 'paused'}
          status={runStatus}
          nextWakeAtMs={nextWakeAtMs}
        />}
      {message.attachments.length > 0 && (
        <div className="message-attachments">
          {message.attachments.map(file => file.mediaType.startsWith('image/') ? (
            <div key={file.id} className="inline-flex items-start gap-1">
              <button onClick={() => onPreview(file)} aria-label={t('查看') + file.name}>
                <img src={`/api/assistant/attachment/${file.id}`} alt={file.name} />
              </button>
              <Button asChild variant="ghost" size="icon" className="size-11">
                <a
                  href={`/api/assistant/attachment/${file.id}`}
                  download={file.name}
                  aria-label={`${t('下载')} ${file.name}`}
                  title={t('下载')}
                >
                  <Download aria-hidden="true" />
                </a>
              </Button>
            </div>
          ) : (
            <a key={file.id} href={`/api/assistant/attachment/${file.id}`} download={file.name}>
              {file.name}
            </a>
          ))}
        </div>
      )}
      {text && <ChatMarkdown remarkPlugins={markdownPlugins}>{text}</ChatMarkdown>}
      {delivery && delivery !== 'accepted' &&
        <span className="message-delivery" role="status">{t(delivery === 'failed' ? '未发送' : '发送中')}</span>}
    </article>
  );
});

export const ChatMessageList = memo(function ChatMessageList({messages, entries, displayCut, runStatus, nextWakeAtMs, onPreview}: {
  messages?: readonly ChatMessage[];
  entries: readonly OutboxEntry[];
  displayCut: ChatDisplayCut | null;
  runStatus?: string;
  nextWakeAtMs: number;
  onPreview: (attachment: ChatAttachment) => void;
}) {
  const cleared = new Set(displayCut?.ids);
  const received = entries.length ? new Set(messages?.map(message => message.requestId)) : undefined;
  const rows: ReactElement[] = [];

  for (const message of messages ?? []) {
    const hidden = cleared.has(message.id);
    const sameRun = displayCut && displayCut.runId === message.runId;
    if (hidden && (message.role !== 'assistant' || !sameRun)) continue;
    if (message.role === 'assistant' && !message.text && !message.hasActivity && !message.attachments.length) continue;

    rows.push(<ChatMessageRow
      key={message.requestId || message.id}
      message={message}
      text={hidden && displayCut?.text === message.text ? '' : message.text}
      traceAfterMs={sameRun ? displayCut.at : 0}
      runStatus={message.active ? runStatus : undefined}
      nextWakeAtMs={message.active ? nextWakeAtMs : 0}
      onPreview={onPreview}
    />);
  }
  for (const entry of entries) {
    if (received?.has(entry.id)) continue;
    const message = create(ChatMessageSchema, {
      id: entry.id,
      requestId: entry.id,
      role: 'user',
      text: entry.text,
      attachments: entry.attachments.map(file => ({...file, sizeBytes: BigInt(file.sizeBytes)})),
    });
    rows.push(<ChatMessageRow
      key={entry.id}
      message={message}
      text={entry.text}
      traceAfterMs={0}
      nextWakeAtMs={0}
      delivery={entry.status}
      onPreview={onPreview}
    />);
  }
  return rows;
});
