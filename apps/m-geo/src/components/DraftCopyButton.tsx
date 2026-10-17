import {useEffect,useRef} from 'react';
import {call} from '../api';
import type {Draft} from '../types';
import {useTask} from './ui';
/** Unlike copying a fixed test question, copying publishable content requires the native revision/fact gate. */
export default function DraftCopyButton({brandId,draft}:{brandId:string;draft:Draft}){
 const task=useTask(),alive=useRef(true);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
 return <><button type="button" disabled={task.busy} onClick={()=>void task.run(async()=>{
  const result=await call<{fileName:string;content:string}>('draft.export',{brandId,id:draft.id,expectedRevision:draft.revision,format:'markdown'});
  if(alive.current)await navigator.clipboard.writeText(result.content);
 },undefined,'已核验当前稿件并复制到剪贴板')}>复制内容</button>{task.feedback}</>;
}
