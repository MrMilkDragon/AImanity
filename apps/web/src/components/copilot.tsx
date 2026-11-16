'use client';
import { LoadingOrb } from "@/components/ui/loading-orb";
import {createContext,useCallback,useContext,useEffect,useLayoutEffect,useRef,useState,useSyncExternalStore,type ReactNode} from 'react';
import {Mic,MicOff} from 'lucide-react';
import {CopilotSession,type CopilotBinding} from '@/lib/copilot-session';
import {useWorkbenchNotice} from './workbench-notices';
import {useI18n} from './locale-provider';
import {mountSpeakingScene} from './copilot-speaking';
import './copilot.css';

const Context=createContext<CopilotSession|null>(null);
export function CopilotProvider({currentDeviceId,onConfigure,children}:{currentDeviceId:string;onConfigure:(agentId:string)=>void;children:ReactNode}){
  const configure=useRef(onConfigure);configure.current=onConfigure;
  const [controller]=useState(()=>new CopilotSession(id=>configure.current(id)));
  const state=useSyncExternalStore(controller.subscribe,controller.snapshot,controller.snapshot);
  useLayoutEffect(()=>controller.select(currentDeviceId),[controller,currentDeviceId]);
  useEffect(()=>()=>controller.stop(),[controller]);
  useWorkbenchNotice(state.error,()=>{void controller.retry();},state.localLanguagePack?'安装本机语言包':undefined);
  return <Context.Provider value={controller}>{children}</Context.Provider>;
}
export function useCopilot(){const value=useContext(Context);if(!value)throw Error('CopilotProvider is required');return value;}
export function useCopilotBinding(binding:CopilotBinding){
  const controller=useCopilot();
  useLayoutEffect(()=>{controller.register(binding);},[controller,binding]);
  useEffect(()=>()=>controller.unregister(binding.deviceId),[controller,binding.deviceId]);
  const state=useSyncExternalStore(controller.subscribe,controller.snapshot,controller.snapshot);
  return state.drafts[binding.deviceId];
}
function useCopilotActivity(deviceId:string) {
  const controller = useCopilot();
  return useSyncExternalStore(controller.subscribe, useCallback(() => {
    const state = controller.snapshot();
    return state.enabled && state.currentDeviceId === deviceId ? state.activity : null;
  }, [controller, deviceId]), () => null);
}
export function CopilotBackdrop({deviceId}:{deviceId:string}) {
  const activity = useCopilotActivity(deviceId);
  return activity ? <canvas ref={mountSpeakingScene} className="copilot-backdrop" data-activity={activity} aria-hidden="true"/> : null;
}
export function CopilotWave({deviceId}:{deviceId:string}) {
  const activity = useCopilotActivity(deviceId);
  if (!activity) return null;

  return (
    <svg className="copilot-wave" data-activity={activity} aria-hidden="true" focusable="false" viewBox="0 0 720 480" preserveAspectRatio="none">
      <g>
        <path d="M-70 240C34 236 99 263 147 235S212 159 250 202 309 401 355 308 398 55 444 149 513 323 565 254 672 220 795 240"/>
        <path d="M-70 240C18 244 86 216 145 244S198 305 246 274 309 79 350 171 410 408 452 319 511 188 564 223 664 264 795 240"/>
        <path d="M-70 240C43 238 91 263 148 237S211 194 250 221 307 349 352 289 410 109 452 191 520 286 563 255 656 227 795 240"/>
        <path d="M-70 240C33 241 90 225 154 243S216 274 251 257 310 150 350 199 412 356 449 294 513 206 563 230 661 247 795 240"/>
      </g>
    </svg>
  );
}
export function CopilotToggle({deviceId,ready}:{deviceId:string;ready:boolean}){
  const controller=useCopilot(),state=useSyncExternalStore(controller.subscribe,controller.snapshot,controller.snapshot),{t}=useI18n();
  const active=(state.enabled||state.opening)&&state.currentDeviceId===deviceId;
  return <button className="copilot-toggle" type="button" aria-pressed={active} disabled={!ready&&!active} title={t(active?'关闭 Copilot':'开启 Copilot')} onClick={()=>{
    if(active)controller.stop();else if(state.enabled)controller.select(deviceId);else void controller.start(deviceId);
  }}>{state.opening&&active?<LoadingOrb size={15} />:active?<Mic size={15}/>:<MicOff size={15}/>}<span>Copilot</span></button>;
}
