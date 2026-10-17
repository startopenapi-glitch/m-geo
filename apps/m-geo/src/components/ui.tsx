import {cloneElement,isValidElement,useEffect,useId,useRef,useState,type ReactElement,type ReactNode,type FormEvent} from 'react';
import {AlertCircle,ArrowRight,CheckCircle2,LoaderCircle} from 'lucide-react';
export function Field({label,children,hint}:{label:string;children:ReactNode;hint?:string}){const id=useId();return <div className="field"><label htmlFor={id}>{label}</label>{isValidElement(children)?cloneElement(children as ReactElement<{id:string;'aria-describedby'?:string}>,{id,'aria-describedby':hint?`${id}-hint`:undefined}):children}{hint&&<small id={`${id}-hint`}>{hint}</small>}</div>}
export function Panel({title,description,children,action}:{title:string;description?:string;children:ReactNode;action?:ReactNode}){return <section className="panel"><div className="panel-head"><div><h2>{title}</h2>{description&&<p>{description}</p>}</div>{action}</div>{children}</section>}
export function Empty({title,children,action}:{title:string;children?:ReactNode;action?:ReactNode}){return <div className="empty"><div className="empty-symbol"><ArrowRight size={23}/></div><h3>{title}</h3><p>{children}</p>{action}</div>}
export function Badge({children,tone='neutral'}:{children:ReactNode;tone?:'neutral'|'blue'|'green'|'amber'}){return <span className={`badge ${tone}`}>{children}</span>}
export function useTask(){
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
 const alive=useRef(true),locked=useRef(false);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
 async function run<T>(fn:()=>Promise<T>,done?:(v:T)=>void,success:string|((value:T)=>string)='操作已完成'){
  if(locked.current)return;locked.current=true;setBusy(true);setError('');setMessage('');
  try{const value=await fn();if(alive.current){done?.(value);setMessage(typeof success==='function'?success(value):success)}return value}
  catch(e){if(alive.current)setError(e instanceof Error?e.message:'操作未完成，请重试')}
  finally{locked.current=false;if(alive.current)setBusy(false)}
 }
 return {busy,error,message,run,feedback:<>{busy&&<p className="feedback" role="status"><LoaderCircle className="spin" size={16}/>正在处理，请稍候…</p>}{error&&<p className="feedback error" role="alert"><AlertCircle size={17}/>{error}</p>}{message&&<p className="feedback success" role="status"><CheckCircle2 size={17}/>{message}</p>}</>};
}
export function submit(fn:()=>void){return (e:FormEvent)=>{e.preventDefault();fn()}}
export const now=()=>{const date=new Date(),pad=(value:number)=>String(value).padStart(2,'0');return `${date.getFullYear()}-${pad(date.getMonth()+1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`};
export const time=(value:string)=>value?new Date(value).toLocaleString('zh-CN',{dateStyle:'medium',timeStyle:'short'}):'—';
export function ClipboardButton({text,label='复制内容'}:{text:string;label?:string}){const task=useTask();return <><button type="button" disabled={task.busy||!text} onClick={()=>void task.run(()=>navigator.clipboard.writeText(text),undefined,'已复制到剪贴板')}>{label}</button>{task.feedback}</>}
