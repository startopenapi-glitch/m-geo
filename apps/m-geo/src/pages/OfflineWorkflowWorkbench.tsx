import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import {Field,Panel} from '../components/ui';
import {defaultOfflineQuestions,offlineNodeLabel,offlineStatusLabel,validateOfflineCreate,validateOfflineHtml,type OfflineAudit,type OfflineCreate,type OfflineMonitor,type OfflineWorkflow,type OfflineWorkflowNode} from '../offlineWorkflow';
import type {Brand,WorkspaceSnapshot} from '../types';

type Props={brand:Brand;data:WorkspaceSnapshot;refresh:()=>Promise<void>;onNavigate?:(section:'content')=>void;onBusyChange?:(busy:boolean)=>void};
const errorText=(error:unknown)=>error instanceof Error?error.message:'本地操作未完成，请重新加载核对';
const nodeStatus={pending:'等待',completed:'已完成',blocked:'待人工处理',failed:'失败'};
const checkStatus={pass:'通过',warning:'提示',fail:'阻断'};

function AuditResult({audit}:{audit:OfflineAudit}){return <section aria-label="离线检查结果"><p>本地检查：通过 {audit.passed} · 提示 {audit.warning} · 阻断 {audit.blocking}</p><ul>{audit.checks.map(check=><li key={check.id}><strong>{check.label} · {checkStatus[check.status]}</strong><p>{check.detail}</p></li>)}</ul><p className="inline-note">仅检查离线字符串，不代表网页可抓取、被收录或被引用。</p></section>}
function NodeResult({node}:{node:OfflineWorkflowNode}){
 if(!node.result)return null;
 if(node.kind==='audit')return <AuditResult audit={node.result as unknown as OfflineAudit}/>;
 if(node.kind==='baseline'||node.kind==='followup'){
  const result=node.result as unknown as OfflineMonitor;
  return <><p>模拟样本 {result.sampleCount} · 提及 {result.mentionCount} · 引用 {result.citationCount}</p>{result.comparisonLabel&&<p>{result.comparisonLabel}</p>}{typeof result.mentionDelta==='number'&&<p>模拟提及差值：{result.mentionDelta}；不能证明真实平台提升。</p>}<ol>{result.samples.map((sample,index)=><li key={index}><p>{sample.question}</p><p className="pre">{sample.answer}</p><p>来源：模拟夹具 fixture · {sample.mentioned?'模拟提及':'模拟未提及'}</p><p>模拟引用：{sample.citations.join('、')||'无'}</p></li>)}</ol></>;
 }
 if(node.kind==='rewrite')return <p>候选稿：{String(node.result.title??'本地整理稿')} · 编号 {String(node.result.draftId??'')}</p>;
 return <p>人工审核状态由普通内容审核记录核对。</p>;
}

export default function OfflineWorkflowWorkbench(props:Props){return <OfflineSession key={props.brand.id} {...props}/>}
function OfflineSession({brand,data,refresh,onNavigate,onBusyChange}:Props){
 const [productId,setProductId]=useState(''),[name,setName]=useState(''),[questionsText,setQuestionsText]=useState(defaultOfflineQuestions),[repetitions,setRepetitions]=useState(1),[maxSamples,setMaxSamples]=useState(60);
 const [tasks,setTasks]=useState<OfflineWorkflow[]>([]),[task,setTask]=useState<OfflineWorkflow|null>(null),[selectedId,setSelectedId]=useState('');
 const [busy,setBusy]=useState(false),[reading,setReading]=useState(false),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const [needsReload,setNeedsReload]=useState(false),[uncertainCreate,setUncertainCreate]=useState(false),[retryCreate,setRetryCreate]=useState(false);
 const [html,setHtml]=useState(''),[audit,setAudit]=useState<OfflineAudit|null>(null);
 const alive=useRef(true),lock=useRef(false),polling=useRef(false),sequence=useRef(0),selected=useRef(''),attempt=useRef<OfflineCreate|null>(null),createBlocked=useRef(false),busyCallback=useRef(onBusyChange);
 busyCallback.current=onBusyChange;
 const products=data.products.filter(p=>p.brandId===brand.id&&p.confirmedAt),product=products.find(p=>p.id===productId);
 const validation=validateOfflineCreate(brand,{name,product,questionsText,repetitions,maxSamples}),htmlValidation=validateOfflineHtml(html);
 const frozen=busy||uncertainCreate||needsReload;
 const currentProduct=task?data.products.find(p=>p.brandId===brand.id&&p.id===task.productId):undefined;
 const stale=!!task&&(!currentProduct||!currentProduct.confirmedAt||currentProduct.revision!==task.productRevision);
 const mutationBlocked=busy||reading||uncertainCreate||needsReload||!!data.truncated;
 const executionBlocked=mutationBlocked||stale;

 function record(value:OfflineWorkflow){
  if(value.brandId!==brand.id)throw new Error('任务品牌不匹配，请重新加载');
  selected.current=value.id;setSelectedId(value.id);setTask(old=>old?.id===value.id&&old.revision>value.revision?old:value);
  setTasks(old=>old.some(t=>t.id===value.id)?old.map(t=>t.id===value.id&&t.revision<=value.revision?value:t):[value,...old]);
 }
 async function readList(){const values=await call<{tasks:OfflineWorkflow[]}>('offlineWorkflow.list',{brandId:brand.id});return values.tasks.filter(t=>t.brandId===brand.id)}
 async function readTask(id:string){const value=await call<OfflineWorkflow>('offlineWorkflow.get',{brandId:brand.id,taskId:id});if(value.brandId!==brand.id||value.id!==id)throw new Error('任务标识不匹配，请重新加载');return value}
 useEffect(()=>{
  alive.current=true;
  async function poll(){
   if(lock.current||polling.current||createBlocked.current)return;
   polling.current=true;const seq=sequence.current,id=selected.current;
   try{const values=await readList();if(!alive.current||sequence.current!==seq||lock.current)return;setTasks(old=>values.map(value=>{const prior=old.find(t=>t.id===value.id);return prior&&prior.revision>value.revision?prior:value}));if(id){const value=await readTask(id);if(alive.current&&sequence.current===seq&&!lock.current&&selected.current===id)record(value)}}
   catch(e){if(alive.current&&sequence.current===seq&&!lock.current)setError(`任务读取失败，请重新加载任务。${errorText(e)}`)}finally{polling.current=false}
  }
  void poll();const timer=window.setInterval(()=>void poll(),1500);
  return()=>{alive.current=false;sequence.current++;window.clearInterval(timer);busyCallback.current?.(false)};
 },[]);
 function begin(){if(lock.current||!alive.current)return false;lock.current=true;sequence.current++;setReading(false);setBusy(true);busyCallback.current?.(true);setError('');setFeedback('');return true}
 function finish(){lock.current=false;if(alive.current){setBusy(false);busyCallback.current?.(false)}}
 async function selectTask(id:string){
  if(lock.current||uncertainCreate||needsReload)return;
  const seq=++sequence.current;selected.current=id;setSelectedId(id);setTask(null);setError('');setFeedback('');setReading(!!id);if(!id)return;
  try{const value=await readTask(id);if(alive.current&&sequence.current===seq&&selected.current===id)record(value)}catch(e){if(alive.current&&sequence.current===seq)setError(errorText(e))}finally{if(alive.current&&sequence.current===seq)setReading(false)}
 }
 async function reload(){
  if(!begin())return;
  try{
   const values=await readList();if(!alive.current)return;setTasks(values);
   const original=attempt.current,match=original?values.find(t=>t.requestId===original.requestId):undefined;
   if(original&&!match){setRetryCreate(true);setFeedback('未找到原请求；可显式重试原创建请求');return}
   const id=match?.id||selected.current;
   if(id){const value=await readTask(id);if(!alive.current)return;record(value)}
   if(original){attempt.current=null;createBlocked.current=false;setUncertainCreate(false);setRetryCreate(false);setName('')}
   setNeedsReload(false);setFeedback(original?'已找到原请求保存的任务，请核对明细':'已加载本地保存版本，请核对后继续');
   try{await refresh()}catch(e){if(alive.current)setError(`任务已加载，工作区资料刷新失败。${errorText(e)}`)}
  }catch(e){if(alive.current){setRetryCreate(false);setError(`重新加载失败，请再次核对。${errorText(e)}`)}}finally{finish()}
 }
 async function submitCreate(payload:OfflineCreate){
  if(needsReload||!begin())return;
  setRetryCreate(false);
  try{
   let value:OfflineWorkflow;
   try{value=await call<OfflineWorkflow>('offlineWorkflow.create',{...payload})}catch(e){if(alive.current){attempt.current=payload;createBlocked.current=true;setUncertainCreate(true);setError(`创建结果未确认，请重新加载任务核对原请求。${errorText(e)}`)}return}
   if(!alive.current)return;record(value);attempt.current=null;createBlocked.current=false;setUncertainCreate(false);setName('');setFeedback('任务已创建并暂停；点击开始后才会运行');
   try{await refresh()}catch(e){if(alive.current)setError(`任务已创建，资料刷新失败；请重新加载任务，无需重复创建。${errorText(e)}`)}
  }finally{finish()}
 }
 function create(){if(frozen||validation||data.truncated||!product)return;const payload:OfflineCreate={brandId:brand.id,requestId:crypto.randomUUID(),name:name.trim(),productId:product.id,questions:questionsText.split(/\r?\n/).map(q=>q.trim()).filter(Boolean),repetitions,maxSamples,evidenceMode:'simulated'};attempt.current=payload;void submitCreate(payload)}
 async function mutate(action:'resume'|'pause'|'cancel'|'retry'){
  if(!task||mutationBlocked||(['resume','retry'].includes(action)&&stale)||!begin())return;
  try{
   let value:OfflineWorkflow;
   try{value=await call<OfflineWorkflow>(`offlineWorkflow.${action}`,{brandId:brand.id,taskId:task.id,expectedRevision:task.revision})}catch(e){if(alive.current){setNeedsReload(true);setError(`操作结果未确认，请重新加载任务核对，操作不会自动重放。${errorText(e)}`)}return}
   if(!alive.current)return;record(value);setFeedback('本地任务状态已保存');
   try{await refresh()}catch(e){if(alive.current)setError(`任务状态已保存，资料刷新失败；请重新加载任务。${errorText(e)}`)}
  }finally{finish()}
 }
 async function exportTask(){
  if(!task||executionBlocked||task.status!=='completed'||!begin())return;
  try{const result=await call<{saved:boolean}|null>('file.exportOfflineWorkflow',{brandId:brand.id,taskId:task.id,expectedRevision:task.revision});if(alive.current)setFeedback(result?.saved?'本地证据 ZIP 已保存':'已取消导出，任务记录保留')}
  catch(e){if(alive.current){setNeedsReload(true);setError(`导出未完成，请重新加载任务核对审核与商品版本。${errorText(e)}`)}}finally{finish()}
 }
 async function checkHtml(){
  if(htmlValidation||!begin())return;setAudit(null);
  try{const value=await call<OfflineAudit>('offlineGeo.audit',{brandId:brand.id,html});if(alive.current){setAudit(value);setFeedback('离线 HTML 检查完成，未执行或联网读取源码')}}catch(e){if(alive.current)setError(errorText(e))}finally{finish()}
 }
 async function goReview(){
  if(!onNavigate||!begin())return;
  let loaded=false;
  try{await refresh();loaded=true}catch(e){if(alive.current)setError(`候选稿资料刷新失败，请重试前往内容审核。${errorText(e)}`)}finally{finish()}
  if(loaded&&alive.current)onNavigate('content');
 }
 return <div className="stack">
  <Panel title="本地自动化" description="模拟基线 → 本地整理 → 离线检查 → 人工审核 → 模拟复测。">
   <p>模拟夹具专用 · 零模型调用 · 零联网 · 零发布</p><p className="inline-note">桌面应用运行期间推进任务；退出或睡眠时不运行，重新打开后需明确继续。模拟结果不能证明真实平台效果。</p>
   {data.truncated&&<p role="alert">资料列表不完整，请重新加载完整工作区后操作。</p>}
   <Field label="已确认商品"><select disabled={frozen} value={productId} onChange={e=>setProductId(e.target.value)}><option value="">请选择商品</option>{products.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select></Field>
   {!products.length&&<p>先在商品资料中确认当前品牌的商品。</p>}
   <Field label="任务名称"><input maxLength={100} value={name} disabled={frozen} onChange={e=>setName(e.target.value)}/></Field>
   <Field label="中性问题（每行一条）" hint="1–10 条，每条最多 500 字；不包含品牌、别名或完整商品名。"><textarea rows={5} value={questionsText} disabled={frozen} onChange={e=>setQuestionsText(e.target.value)}/></Field>
   <div className="form-grid"><Field label="每题重复次数"><input type="number" min={1} max={3} step={1} value={repetitions} disabled={frozen} onChange={e=>setRepetitions(Number(e.target.value))}/></Field><Field label="两轮样本预算"><input type="number" min={1} max={60} step={1} value={maxSamples} disabled={frozen} onChange={e=>setMaxSamples(Number(e.target.value))}/></Field></div>
   {productId&&name&&validation&&<p role="alert">{validation}</p>}
   <div className="actions"><button className="primary" disabled={frozen||!!validation||!!data.truncated} onClick={create}>创建暂停任务</button>{retryCreate&&<button disabled={busy||needsReload} onClick={()=>{if(attempt.current)void submitCreate(attempt.current)}}>重试原创建请求</button>}</div>
   {uncertainCreate&&<p className="inline-note">保留原请求编号：{attempt.current?.requestId}。先重新加载核对；重试只使用原编号和原内容。</p>}
  </Panel>
  <Panel title="已保存本地任务" description="离开此页面后，任务仍按已保存状态运行。">
   <div className="actions"><button disabled={busy} onClick={()=>void reload()}>重新加载任务</button></div>
   <Field label="已保存本地任务"><select disabled={busy||uncertainCreate||needsReload} value={selectedId} onChange={e=>void selectTask(e.target.value)}><option value="">请选择任务</option>{tasks.map(t=><option key={t.id} value={t.id}>{t.name} · {offlineStatusLabel[t.status]} · v{t.revision}</option>)}</select></Field>
   {!tasks.length&&<p>尚无本地任务；创建后保持暂停，需明确开始。</p>}{reading&&<p role="status">正在读取本地任务…</p>}
   {task&&<section aria-label="本地任务明细"><h3>{task.name}</h3><p>状态：{offlineStatusLabel[task.status]}</p><p>任务版本 {task.revision} · 商品版本 {task.productRevision} · 样本 {task.budget.usedSamples} / {task.budget.maxSamples}</p><p>模拟夹具专用 · 零模型调用 · 零联网 · 零发布</p>
    {stale&&<p role="alert">商品资料已变化或不再确认；请核对事实并新建任务，当前任务无法开始、重试或导出，仍可暂停或取消。</p>}{task.error&&<p role="alert">{task.error}</p>}
    <div className="actions"><button disabled={executionBlocked||!['paused','awaiting_review'].includes(task.status)} onClick={()=>void mutate('resume')}>开始 / 继续</button><button disabled={mutationBlocked||task.status!=='running'} onClick={()=>void mutate('pause')}>暂停任务</button><button disabled={mutationBlocked||['completed','cancelled'].includes(task.status)} onClick={()=>void mutate('cancel')}>取消任务</button><button disabled={executionBlocked||task.status!=='failed'} onClick={()=>void mutate('retry')}>重试失败任务</button><button disabled={executionBlocked||task.status!=='completed'} onClick={()=>void exportTask()}>导出本地证据 ZIP</button></div>
    {(task.draftId||task.status==='awaiting_review')&&<div className="cohort"><p>候选稿仍须在内容页人工审核；本页不会批准稿件。审核后返回本页点击“开始 / 继续”，由桌面应用核对并执行模拟复测。</p>{task.draftId&&<p>候选稿编号：{task.draftId}</p>}{onNavigate&&<button disabled={busy} onClick={()=>void goReview()}>前往内容审核</button>}</div>}
    <p>节点进度：{task.nodes.filter(n=>n.status==='completed').length} / {task.nodes.length}</p><ol className="queue-list">{task.nodes.map(node=><li className="cohort" key={node.id}><h4>{offlineNodeLabel[node.kind]} · {nodeStatus[node.status]}</h4><p>依赖：{node.dependsOn.map(id=>offlineNodeLabel[id as keyof typeof offlineNodeLabel]??id).join(' → ')||'起点'}</p>{node.error&&<p role="alert">{node.error}</p>}<NodeResult node={node}/></li>)}</ol>
   </section>}
  </Panel>
  <Panel title="离线 HTML 预览检查" description="仅接收源码，最多 256 KiB UTF-8；不执行脚本，不读取链接。">
   <Field label="离线 HTML 源码"><textarea rows={7} value={html} disabled={busy} onChange={e=>{setHtml(e.target.value);setAudit(null)}}/></Field>{html&&htmlValidation&&<p role="alert">{htmlValidation}</p>}
   <button disabled={busy||!!htmlValidation} onClick={()=>void checkHtml()}>检查离线 HTML</button>{audit&&<AuditResult audit={audit}/>}
  </Panel>
  {busy&&<p role="status">正在处理本地资料…</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
