"use client";
import {useCallback,useEffect,useLayoutEffect,useRef,useState,type RefObject} from 'react';
import {create,equals,fromBinary,type MessageShape} from '@bufbuild/protobuf';
import {ChatStateSchema,ChatVoiceInputSchema,type ChatMessage} from '@ai-phone/protocol/ai_phone/v1/assistant_pb';
import {ApiErrorSchema} from '@ai-phone/protocol/ai_phone/v1/system_pb';
import {ChatOutbox,type OutboxEntry,type OutboxInput} from '@/lib/chat-outbox';
import {encodeChatSend} from '@/lib/chat-wire';
import type {AcceptedInput} from '@/lib/chat-snapshot';
import {receiptBatch} from '@/lib/chat-window';

type Chat=MessageShape<typeof ChatStateSchema>;
type Binding={conversationId:string;box:ChatOutbox<AcceptedInput>;receiptOffset:number};

async function send(item:OutboxInput,signal:AbortSignal):Promise<AcceptedInput>{
  const response=await fetch('/api/assistant/send',{method:'POST',headers:{'content-type':'application/x-protobuf'},body:encodeChatSend(item),signal:AbortSignal.any([signal,AbortSignal.timeout(30000)])});
  const mediaType=response.headers.get('content-type')?.split(';',1)[0].trim().toLowerCase();
  if(mediaType!=='application/x-protobuf')throw Error('服务返回了无效的消息格式');
  const bytes=new Uint8Array(await response.arrayBuffer());
  if(!response.ok)throw Error(fromBinary(ApiErrorSchema,bytes).message||'请求失败');
  if(response.status!==202)throw Error('服务返回了无效的消息状态');
  const chat=fromBinary(ChatStateSchema,bytes);
  const messages=chat.messages.filter(message=>message.requestId===item.id&&message.role==='user');
  const message=messages[0];
  if(chat.conversationId!==item.conversationId||messages.length!==1||!message?.id||message.ordinal<=0n)throw Error('消息回执尚未确认，请重试');
  return{conversationId:item.conversationId,message};
}

function matchesInput(message:ChatMessage,item:OutboxInput){
  return message.text===item.text&&message.attachments.length===item.attachments.length
    &&message.attachments.every((file,index)=>file.id===item.attachments[index].id)
    &&(item.voice?!!message.voice&&equals(ChatVoiceInputSchema,message.voice,create(ChatVoiceInputSchema,item.voice)):!message.voice);
}

export function useChatOutbox(conversationId:string,chat:RefObject<Chat|null>,acknowledge:(receipt:AcceptedInput)=>void,refreshHistory:()=>void){
  const owner=useRef<Binding|null>(null);
  const [entries,setEntries]=useState<OutboxEntry[]>([]),[error,setError]=useState('');
  const project=useCallback((snapshot:Chat)=>{
    const current=owner.current;
    if(current?.conversationId!==snapshot.conversationId||!current.box.entries.length)return;
    const pending=new Map(current.box.entries.map(item=>[item.id,item]));
    const confirmed=new Set<string>();
    for(const message of snapshot.messages){
      const item=pending.get(message.requestId);
      if(item&&message.role==='user'&&message.id&&message.ordinal>0n&&matchesInput(message,item))confirmed.add(item.id);
    }
    if(!confirmed.size)return;
    try{current.box.projected(confirmed);setError('');}
    catch(value){setError(value instanceof Error?value.message:'待发送消息无法更新');}
  },[]);
  const receiptIds=useCallback(()=>{
    const current=owner.current;
    if(!current)return [];
    const batch=receiptBatch(current.box.entries,current.receiptOffset);
    current.receiptOffset=batch.next;
    return batch.ids;
  },[]);
  const disconnect=useCallback(()=>{
    const current=owner.current;
    if(current?.conversationId===conversationId){current.box.dispose();owner.current=null;}
  },[conversationId]);
  useLayoutEffect(()=>disconnect,[disconnect]);
  useEffect(()=>{
    setEntries([]);setError('');
    if(!conversationId)return;
    try{
      const key=`ai-phone:outbox:phone-local-01:${conversationId}`;
      const box=new ChatOutbox({read:()=>localStorage.getItem(key),write:value=>localStorage.setItem(key,value)},send,acknowledge,setEntries);
      owner.current={conversationId,box,receiptOffset:0};
      setEntries(box.entries);
      if(chat.current)project(chat.current);
      box.start();
      if(box.entries.some(item=>item.status==='accepted'))refreshHistory();
      return disconnect;
    }catch(value){setError(value instanceof Error?value.message:'待发送消息无法读取');}
  },[conversationId,chat,acknowledge,refreshHistory,project,disconnect]);
  const submit=useCallback((item:OutboxInput)=>{
    const current=owner.current;
    if(current?.conversationId!==conversationId||item.conversationId!==conversationId)throw Error('对话正在连接，请稍后重试');
    current.box.enqueue(item);
  },[conversationId]);
  const retry=useCallback(()=>{
    const current=owner.current;
    if(current?.conversationId!==conversationId)return;
    try{
      setError('');
      current.box.retry();
      if(current.box.entries.some(item=>item.status==='accepted'))refreshHistory();
    }catch(value){setError(value instanceof Error?value.message:'重试失败');}
  },[conversationId,refreshHistory]);
  const bound=owner.current?.conversationId===conversationId;
  return{entries:bound?entries:[],error,ready:bound,submit,retry,receiptIds,project};
}
