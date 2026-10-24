import {useLayoutEffect,useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {Field,Panel,useTask} from './ui';

interface Preview {
 canApply:boolean;
 token:string;
 readyCount:number;
 skippedCount:number;
 rows:{row:number;status:string;error?:string}[];
}
interface Packet {summary:{expected:number;completed:number;remaining:number;evidenceMode:string}}
const statusLabels:Record<string,string>={ready:'待保存',skipped:'空白跳过',error:'错误'};

export default function CollectionPanel({brand,data,refresh}:PageProps){
 const [campaignId,setCampaign]=useState('');
 const [phase,setPhase]=useState('baseline');
 const [content,setContent]=useState('');
 const [preview,setPreview]=useState<Preview|null>(null);
 const [packet,setPacket]=useState<Packet|null>(null);
 const generation=useRef(0);
 const task=useTask();
 const campaign=(data.campaigns||[]).find(c=>c.id===campaignId);
 const modeLabel=campaign?.evidenceMode==='simulated'
  ? '模拟证据：仅验证流程，不代表真实表现'
  : campaign?.evidenceMode==='recorded'?'人工实测证据':'历史证据模式未指定';
 const payload={brandId:brand.id,campaignId,phase};

 useLayoutEffect(()=>{
  generation.current++;
  setPreview(null);
  setPacket(null);
 },[brand.id,data,campaignId,phase,content]);

 async function read(){
  const version=generation.current;
  const file=await call<{content:string}|null>('file.openText',{kind:'evidence'});
  if(file&&version===generation.current)setContent(file.content);
 }
 function prepare(){
  const version=generation.current;
  void task.run(
   ()=>call<Packet>('collection.prepare',payload),
   result=>{if(version===generation.current)setPacket(result)},
   '采集清单已计算',
  );
 }
 function previewContent(){
  const version=generation.current;
  void task.run(
   ()=>call<Preview>('collection.preview',{...payload,content}),
   result=>{if(version===generation.current)setPreview(result)},
   '请核对预览',
  );
 }
 function apply(){
  if(!preview)return;
  void task.runPersisted(async()=>{
   const result=await call('collection.apply',{
    ...payload,content,token:preview.token,confirm:true,
   });
   setPreview(null);
   return result;
  },refresh,undefined,`${modeLabel}；原始证据已原子保存`);
 }

 return (
  <Panel title="人工采集与原始证据导入" description="固定消费者端测试条件，先预览全部行，再一次性保存。">
   <Field label="采集固定计划">
    <select value={campaignId} onChange={event=>{
     setCampaign(event.target.value);
     setContent('');
    }}>
     <option value="">选择固定计划</option>
     {(data.campaigns||[]).map(item=>(
      <option key={item.id} value={item.id}>{item.name}</option>
     ))}
    </select>
   </Field>
   {campaign&&<p className="inline-note" aria-label="采集证据模式">{modeLabel}</p>}
   <Field label="采集阶段">
    <select value={phase} onChange={event=>setPhase(event.target.value)}>
     <option value="baseline">优化前基线</option>
     <option value="followup">后续复测</option>
    </select>
   </Field>
   <div className="actions">
    <button disabled={task.busy||!campaignId} onClick={prepare}>计算采集清单</button>
    <button disabled={task.busy||!campaignId} onClick={()=>void task.run(
     ()=>call('file.exportCollection',payload),
     undefined,
     result=>result?'采集 ZIP 已保存到本机':'已取消保存采集包',
    )}>导出采集 ZIP</button>
   </div>
   {packet&&(
    <p>共 {packet.summary.expected} 个槽位 · 已完成 {packet.summary.completed} · 待采集 {packet.summary.remaining}</p>
   )}
   <button disabled={task.busy||!campaignId} onClick={()=>void task.run(read,undefined,'JSON 已读取，请预览')}>
    读取采集 JSON
   </button>
   <Field label="采集证据 JSON">
    <textarea rows={8} value={content} onChange={event=>setContent(event.target.value)}/>
   </Field>
   <button disabled={task.busy||!campaignId||!content.trim()} onClick={previewContent}>预览证据导入</button>
   {preview&&(
    <>
     <p>可导入 {preview.readyCount} 行 · 空白跳过 {preview.skippedCount} 行 · 共 {preview.rows.length} 行（最多显示前50行）</p>
     {preview.rows.slice(0,50).map(row=>(
      <p key={row.row}>
       第 {row.row} 行 · {statusLabels[row.status]||row.status}{row.error&&`：${row.error}`}
      </p>
     ))}
     <button className="primary" disabled={task.busy||!preview.canApply} onClick={apply}>确认原子导入</button>
    </>
   )}
   {task.feedback}
  </Panel>
 );
}
