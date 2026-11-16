'use client';
import { LoadingOrb } from "@/components/ui/loading-orb";
import {useWorkbenchNotice} from './workbench-notices';
import {useI18n} from "./locale-provider";
import {createContext,useContext,useCallback,useEffect,useId,useRef,useState,type ReactNode} from 'react';
import {createPortal} from 'react-dom';
import type {MessageShape} from '@bufbuild/protobuf';
import {GoalUpdateRequestSchema,ProactiveRequestSchema,ProactiveStateSchema,ProactiveUpdateRequestSchema} from '@ai-phone/protocol/ai_phone/v1/proactive_pb';
import {settingsRpc} from '@/lib/settings-client';
import {Compass,Pause,Target} from 'lucide-react';
import {FloatingWindow,WindowTrigger} from './floating-window';
import {Button} from './ui/button';
import './proactive-controls.css';
type State=MessageShape<typeof ProactiveStateSchema>;
type Options=Pick<State,'exploring'>;
type Mutation='liberty'|'goal';
const statusLabel:Record<string,string>={exploring:'Liberty 运行中',notification:'查看手机动态',user_busy:'正在处理你的任务',paused:'任务已暂停',waiting:'等待下一轮',idle:'待机'};
type Controller={
  state:State|null;
  error:string;
  goalError:string;
  saving:Mutation|null;
  visible:boolean;
  update:(patch:Partial<Options>)=>Promise<State|null>;
  updateGoal:(goal:string,deviceId:string)=>Promise<State|null>;
};
const ProactiveContext=createContext<Controller|null>(null);
function useProactive(){const value=useContext(ProactiveContext);if(!value)throw new Error('ProactiveProvider is required');return value;}
const readState=(signal?:AbortSignal)=>settingsRpc('state',ProactiveRequestSchema,{},ProactiveStateSchema,'proactive',signal);
export function ExplorationAura({active,paused,kind="phone"}:{active:boolean;paused:boolean;kind?:"phone"|"desktop"}){
 const goalActive=useContext(ProactiveContext)?.state?.goalActive;
 const showing=active&&!goalActive;
 const [mounted,setMounted]=useState(false);
 useEffect(()=>{if(showing){setMounted(true);return;}const timer=setTimeout(()=>setMounted(false),700);return()=>clearTimeout(timer);},[showing]);
 return mounted?createPortal(<div className="exploration-aura" aria-hidden="true" data-kind={kind} data-enabled={showing} data-paused={paused}><span className="exploration-edge"><span className="exploration-halo"/><span className="exploration-rim"/></span></div>,document.body):null;
}
export function ProactiveProvider({children}:{children:ReactNode}){
 const [state,setState]=useState<State|null>(null),[connectionError,setConnectionError]=useState(''),[saveError,setSaveError]=useState(''),[goalError,setGoalError]=useState(''),[saving,setSaving]=useState<Mutation|null>(null),[visible,setVisible]=useState(true);
 const current=useRef<State|null>(null),updating=useRef(false),generation=useRef(0);
 const receive=useCallback((next:State)=>{if(current.current&&next.revision<current.current.revision)return;current.current=next;setState(next);},[]);
 useEffect(()=>{
  const abort=new AbortController();let timer:ReturnType<typeof setTimeout>;
  const refresh=async()=>{const version=generation.current;if(!document.hidden&&!updating.current)try{const next=await readState(abort.signal);if(!abort.signal.aborted&&version===generation.current){receive(next);setConnectionError('');}}catch(e){if(!abort.signal.aborted)setConnectionError(e instanceof Error?e.message:'主动服务暂不可用');}if(!abort.signal.aborted)timer=setTimeout(refresh,2500);};
  const visibility=()=>{setVisible(!document.hidden);if(!document.hidden){clearTimeout(timer);void refresh();}};
  void refresh();document.addEventListener('visibilitychange',visibility);return()=>{abort.abort();clearTimeout(timer);document.removeEventListener('visibilitychange',visibility);};
 },[receive]);
 async function mutate(kind:Mutation,request:(state:State)=>Promise<State>){
  const snapshot=current.current;
  if(!snapshot||updating.current)return null;
  const setError=kind==='goal'?setGoalError:setSaveError;
  updating.current=true;
  generation.current++;
  setSaving(kind);
  setError('');
  try{
   const next=await request(snapshot);
   receive(next);
   return next;
  }catch(error){
   setError(error instanceof Error?error.message:'保存失败');
   try{receive(await readState());}catch{}
   return null;
  }finally{
   updating.current=false;
   setSaving(null);
  }
 }
 function update(patch:Partial<Options>){
  return mutate('liberty',value=>settingsRpc('update',ProactiveUpdateRequestSchema,{
   expectedRevision:value.revision,exploring:value.exploring,
   notificationsEnabled:value.notificationsEnabled,forwardNotifications:value.forwardNotifications,
   autonomousReply:value.autonomousReply,autonomousInstall:value.autonomousInstall,
   intervalSeconds:value.intervalSeconds,wechatWatch:value.wechatWatch,...patch,
  },ProactiveStateSchema,'proactive'));
 }
 function updateGoal(goal:string,deviceId:string){
  return mutate('goal',value=>settingsRpc('goal',GoalUpdateRequestSchema,{
   expectedRevision:value.revision,goal,deviceId:value.goalActive?value.goalDeviceId:deviceId,
  },ProactiveStateSchema,'proactive'));
 }
 return <ProactiveContext.Provider value={{state,error:saveError||connectionError,goalError:goalError||connectionError,saving,visible,update,updateGoal}}>
  {children}
  <ExplorationAura active={!!state?.exploring} paused={!visible||!!connectionError}/>
 </ProactiveContext.Provider>;
}
/** Both device types expose the same Liberty control and failure feedback. */
export function ExplorationControl({active,ready,saving,error,status,onToggle}:{active:boolean;ready:boolean;saving:boolean;error:string;status:string;onToggle:()=>void}){
 const {t}=useI18n();
 const {state,saving:mutation}=useProactive();
 const blocked=!!state?.goalActive&&!active;
 useWorkbenchNotice(error);
 return <div className="exploration-control"><div className="proactive-dock" data-exploring={active}><button className="exploration-toggle" type="button" aria-pressed={active} disabled={!ready||saving||blocked||mutation==='goal'} onClick={onToggle} title={t(blocked?'目标运行中':status)}>{saving?<LoadingOrb size={16} />:active?<Pause size={15}/>:<Compass size={16}/>}<span>Liberty</span></button></div></div>;
}
export function ExplorationToggle(){
 const {state,saving,error,update}=useProactive();
 return <ExplorationControl active={!!state?.exploring} ready={!!state} saving={saving==='liberty'} error={error||state?.lastError||''} status={state?statusLabel[state.status]||state.status:'连接主动服务'} onToggle={()=>void update({exploring:!state?.exploring})}/>;
}

const goalTitle='Task & Goal';
const goalLimit=12000;
const goalStatusLabels:Record<string,string>={
 queued:'待继续',running:'正在处理',paused:'任务已暂停',
 failed:'执行失败',succeeded:'已完成',canceled:'已停止',
};

export function TaskGoal({open,deviceId,onOpen,onClose}:{
 open:boolean;deviceId:string;onOpen:()=>void;onClose:()=>void;
}){
 const {t}=useI18n();
 const {state,goalError,saving,updateGoal}=useProactive();
 const [draft,setDraft]=useState<string|null>(null);
 const saved=state?.goal??'';
 const status=state?.goalStatus;
 const value=draft??saved;
 const goal=value.trim();
 const active=!!state?.goalActive;
 const changed=goal!==saved;
 const tooLong=new TextEncoder().encode(goal).length>goalLimit;
 const canSave=!!state&&!saving&&!!goal&&!tooLong&&(!active||changed);

 async function save(next:string){
  const updated=await updateGoal(next,deviceId);
  if(!updated)return;
  setDraft(null);
 }

 return <>
  <span className="goal-trigger" data-active={active}>
   <WindowTrigger label={goalTitle} open={open} onOpen={onOpen}><Target size={18}/></WindowTrigger>
  </span>
  <FloatingWindow id="task-goal" title={goalTitle} open={open} onClose={onClose} width={650} height={450} fitContent>
   <form className="task-goal-form" aria-busy={saving==='goal'} onSubmit={event=>{
    event.preventDefault();
    if(canSave)void save(goal);
   }}>
    <textarea
     aria-label={t('目标与要求')}
     placeholder={t('填入你的目标与要求')}
     value={value}
     rows={8}
     maxLength={goalLimit}
     aria-invalid={tooLong}
     disabled={!state||!!saving}
     onChange={event=>setDraft(event.target.value)}
    />
    {(goalError||tooLong)&&<p className="task-goal-error" role="alert">{tooLong?t('目标内容不能超过 12 KB'):goalError}</p>}
    <div className="task-goal-actions">
     {saving==='goal'&&<LoadingOrb size={17} label={t('保存中…')}/>}
     {saved&&status&&<span className="task-goal-status" role="status">{t(goalStatusLabels[status]??status)}</span>}
     {active&&<Button type="button" variant="outline" disabled={!!saving} onClick={()=>void save('')}>{t('关闭目标')}</Button>}
     {(!active||changed)&&<Button type="submit" disabled={!canSave}>{t(active?'更新目标':'激活')}</Button>}
    </div>
   </form>
  </FloatingWindow>
 </>;
}

export function TaskGoalBackdrop(){
 const {state,visible}=useProactive();
 const metal=useId();
 if(!state?.goalActive)return null;
 const paint=`url(#${metal})`;
 return (
  <svg className="task-goal-backdrop" data-paused={!visible||state.goalStatus==='paused'||state.goalStatus==='failed'} aria-hidden="true" focusable="false" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice">
   <defs>
    <linearGradient id={metal} x1="0" y1="1" x2="1" y2="0">
     <stop offset="0" stopColor="#8768b2"/>
     <stop offset=".42" stopColor="#8098ba"/>
     <stop offset=".8" stopColor="#c18c5d"/>
     <stop offset="1" stopColor="#f3b682"/>
    </linearGradient>
   </defs>
   <g className="goal-engraving" fill="none" stroke={paint}>
    <path d="M-40 610H193L557 303H955l139-100h386M-40 704H181l340-283h493l176 141h295"/>
    <path d="M-20 564h175l377-331h330M1447 619h-249l-128-106H920"/>
   </g>
   <g transform="translate(291 485) scale(1.29)" stroke={paint} fill="none">
    <circle className="goal-guide" r="326"/>
    <circle className="goal-guide" r="294"/>
    <circle className="goal-guide" r="160"/>
    <g className="goal-orbit">
     <circle className="goal-ticks" r="294" pathLength="360"/>
     <circle className="goal-sweep" r="272" pathLength="360"/>
     <circle className="goal-thread" r="260" pathLength="360"/>
     <circle className="goal-node" cx="272" r="3" fill={paint}/>
    </g>
    <g className="goal-orbit goal-orbit-reverse">
     <circle className="goal-ticks" r="195" pathLength="360"/>
     <circle className="goal-sweep" r="171" pathLength="360"/>
     <circle className="goal-node" cy="-171" r="3.5" fill={paint}/>
    </g>
   </g>
   <g transform="translate(1370 544) scale(1.18)" stroke={paint} fill="none">
    <circle className="goal-guide" r="218"/>
    <circle className="goal-guide" r="137"/>
    <g className="goal-orbit goal-orbit-secondary">
     <circle className="goal-ticks" r="218" pathLength="360"/>
     <circle className="goal-sweep" r="207" pathLength="360"/>
     <circle className="goal-thread" r="189" pathLength="360"/>
     <circle className="goal-node" cx="-207" r="3" fill={paint}/>
    </g>
   </g>
  </svg>
 );
}
