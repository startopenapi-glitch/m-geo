import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import {Field,Panel} from '../components/ui';
import {defaultP0Question,defaultP0Target,p0NodeLabel,p0SiteFixture,p0StatusLabel,validateP0Create,type P0Audit,type P0Create,type P0Mode,type P0Policy,type P0Preview,type P0Scenario,type P0Task} from '../p0Workflow';
import type {Brand,WorkspaceSnapshot} from '../types';
type Props={brand:Brand;data:WorkspaceSnapshot;refresh:()=>Promise<void>;onNavigate?:(section:'content')=>void;onBusyChange?:(busy:boolean)=>void};
const errorText=(e:unknown)=>e instanceof Error?e.message:'本地操作未完成，请重新加载核对';
const boundary='实际联网 0 · 实际模型调用 0 · 实际发布 0';
function Audit({value}:{value:P0Audit}){return <section aria-label="站点夹具审计结果"><p>通过 {value.passed} · 提示 {value.warning} · 阻断 {value.blocking}</p><ul>{value.checks.map(c=><li key={c.id}><strong>{c.label} · {{pass:'通过',warning:'提示',fail:'阻断'}[c.status]}</strong><p>{c.detail}</p></li>)}</ul><p>仅本地夹具审计，不代表真实网页可抓取、收录或推荐。</p></section>}
function Json({value,label}:{value:unknown;label:string}){return <pre className="p0-output" aria-label={label}>{JSON.stringify(value,null,2)}</pre>}
export default function P0WorkflowWorkbench(props:Props){return <Session key={props.brand.id} {...props}/>}
function Session({brand,data,refresh,onNavigate,onBusyChange}:Props){
 const products=data.products.filter(p=>p.brandId===brand.id&&p.confirmedAt);
 const [productId,setProductId]=useState(products[0]?.id??''),[name,setName]=useState(''),[question,setQuestion]=useState(defaultP0Question),[mode,setMode]=useState<P0Mode>('natural_discovery'),[targetUrl,setTargetUrl]=useState(defaultP0Target),[scenario,setScenario]=useState<P0Scenario>('success'),[maxUnits,setMaxUnits]=useState(10),[maxCalls,setMaxCalls]=useState(10);
 const [tasks,setTasks]=useState<P0Task[]>([]),[task,setTask]=useState<P0Task|null>(null),[selectedId,setSelectedId]=useState(''),[policy,setPolicy]=useState<P0Policy|null>(null),[dailyUnits,setDailyUnits]=useState(100),[dailyCalls,setDailyCalls]=useState(100);
 const [preview,setPreview]=useState<P0Preview|null>(null),[audit,setAudit]=useState<P0Audit|null>(null),[auditScenario,setAuditScenario]=useState<'success'|'robots_blocked'|'hidden_text'>('success');
 const [busy,setBusy]=useState(false),[reading,setReading]=useState(false),[needsReload,setNeedsReload]=useState(false),[uncertainCreate,setUncertainCreate]=useState(false),[retryCreate,setRetryCreate]=useState(false),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const [publishConfirm,setPublishConfirm]=useState(false),[reconcileConfirm,setReconcileConfirm]=useState(false),[rollbackConfirm,setRollbackConfirm]=useState(false);
 const alive=useRef(true),lock=useRef(false),polling=useRef(false),sequence=useRef(0),selected=useRef(''),currentTask=useRef<P0Task|null>(null),attempt=useRef<P0Create|null>(null),blocked=useRef(false),busyCallback=useRef(onBusyChange),policyDirty=useRef(false);
 busyCallback.current=onBusyChange;
 const product=products.find(p=>p.id===productId),validation=validateP0Create(brand,{name,product,question,mode,targetUrl,maxUnits,maxCalls});
 const previewValidation=validateP0Create(brand,{name:'请求预览',product,question,mode,targetUrl:defaultP0Target,maxUnits:10,maxCalls:10});
 const currentProduct=task?data.products.find(p=>p.brandId===brand.id&&p.id===task.productId):undefined;
 const approvalContext=JSON.stringify({product:currentProduct,draft:data.drafts.find(d=>d.brandId===brand.id&&d.id===task?.draftId)});
 const stale=!!task&&(!currentProduct?.confirmedAt||currentProduct.revision!==task.productRevision);
 const mutationBlocked=busy||reading||needsReload||uncertainCreate;
 const contentBlocked=mutationBlocked||!!data.truncated;
 const executionBlocked=contentBlocked||stale||!policy||policy.halted;
 const policyInvalid=[dailyUnits,dailyCalls].some(n=>!Number.isInteger(n)||n<1||n>10000);
 function resetConfirm(){setPublishConfirm(false);setReconcileConfirm(false);setRollbackConfirm(false)}
 function record(value:P0Task){
  if(value.brandId!==brand.id)throw Error('任务品牌不匹配，请重新加载');
  const prior=currentTask.current;
  if(prior?.id===value.id&&prior.revision>value.revision)return;
  if(prior?.id!==value.id||prior.revision!==value.revision)resetConfirm();
  selected.current=value.id;currentTask.current=value;setSelectedId(value.id);setTask(value);
  setTasks(old=>old.some(t=>t.id===value.id)?old.map(t=>t.id===value.id&&t.revision<=value.revision?value:t):[value,...old]);
 }
 function recordList(values:P0Task[]){setTasks(old=>values.map(value=>{const prior=old.find(t=>t.id===value.id);return prior&&prior.revision>value.revision?prior:value}))}
 function recordPolicy(value:P0Policy){setPolicy(old=>old&&old.revision>value.revision?old:value);if(!policyDirty.current){setDailyUnits(value.dailyMaxUnits);setDailyCalls(value.dailyMaxCalls)}}
 async function readList(){const result=await call<{tasks:P0Task[]}>('p0Workflow.list',{brandId:brand.id});return result.tasks.filter(t=>t.brandId===brand.id)}
 async function readTask(id:string){const value=await call<P0Task>('p0Workflow.get',{brandId:brand.id,taskId:id});if(value.brandId!==brand.id||value.id!==id)throw Error('任务标识不匹配，请重新加载');return value}
 useEffect(()=>{
  alive.current=true;
  async function poll(){
   if(lock.current||polling.current||blocked.current)return;
   polling.current=true;const seq=sequence.current,id=selected.current;
   try{
    const [values,p]=await Promise.all([readList(),call<P0Policy>('p0Workflow.policyGet',{brandId:brand.id})]);
    if(!alive.current||sequence.current!==seq||lock.current)return;
    recordList(values);recordPolicy(p);
    if(id){const value=await readTask(id);if(alive.current&&sequence.current===seq&&!lock.current&&selected.current===id)record(value)}
   }catch(e){if(alive.current&&sequence.current===seq&&!lock.current)setError(`演练读取失败，请重新加载核对。${errorText(e)}`)}finally{polling.current=false}
  }
  void poll();const timer=window.setInterval(()=>void poll(),1500);
  return()=>{alive.current=false;sequence.current++;window.clearInterval(timer);busyCallback.current?.(false)};
 },[]);
 useEffect(()=>{resetConfirm()},[approvalContext]);
 function begin(){if(lock.current||!alive.current)return false;lock.current=true;sequence.current++;setReading(false);setBusy(true);busyCallback.current?.(true);setError('');setFeedback('');return true}
 function finish(){lock.current=false;if(alive.current){setBusy(false);busyCallback.current?.(false)}}
 function uncertain(e:unknown,label='操作'){blocked.current=true;setNeedsReload(true);resetConfirm();setError(`${label}结果未确认，请重新加载演练核对，操作不会自动重放。${errorText(e)}`)}
 async function refreshWorkspace(){try{await refresh()}catch(e){if(alive.current){blocked.current=true;setNeedsReload(true);setError(`状态已保存，资料刷新失败，请重新加载演练。${errorText(e)}`)}}}
 async function selectTask(id:string){
  if(lock.current||blocked.current)return;const seq=++sequence.current;selected.current=id;currentTask.current=null;setSelectedId(id);setTask(null);setReading(!!id);setError('');setFeedback('');resetConfirm();if(!id)return;
  try{const value=await readTask(id);if(alive.current&&sequence.current===seq&&selected.current===id)record(value)}catch(e){if(alive.current&&sequence.current===seq)setError(errorText(e))}finally{if(alive.current&&sequence.current===seq)setReading(false)}
 }
 async function reload(){
  if(!begin())return;
  try{
   const [values,p]=await Promise.all([readList(),call<P0Policy>('p0Workflow.policyGet',{brandId:brand.id})]);if(!alive.current)return;
   recordList(values);recordPolicy(p);const original=attempt.current,match=original?values.find(t=>t.requestId===original.requestId):undefined;
   if(original&&!match){setRetryCreate(true);setFeedback('未找到原创建请求；可明确重试原编号与原内容');return}
   const id=match?.id||selected.current;if(id){const value=await readTask(id);if(!alive.current)return;record(value)}
   await refresh();if(!alive.current)return;
   attempt.current=null;blocked.current=false;setUncertainCreate(false);setRetryCreate(false);setNeedsReload(false);resetConfirm();setFeedback('已加载本地保存版本，请核对后明确继续');
  }catch(e){if(alive.current)setError(`重新加载失败，请再次核对。${errorText(e)}`)}finally{finish()}
 }
 async function submitCreate(payload:P0Create,retry=false){
  if((retry?(busy||reading||!!data.truncated||!retryCreate||attempt.current!==payload):contentBlocked)||!begin())return;
  setRetryCreate(false);attempt.current=payload;
  try{
   let value:P0Task;try{value=await call<P0Task>('p0Workflow.create',{...payload});if(alive.current)record(value)}catch(e){if(alive.current){setUncertainCreate(true);uncertain(e,'创建')}return}
   if(!alive.current)return;attempt.current=null;blocked.current=false;setNeedsReload(false);setUncertainCreate(false);setName('');setFeedback('演练已创建并暂停；需明确开始');await refreshWorkspace();
  }finally{finish()}
 }
 function create(){
  if(contentBlocked||validation||!product)return;
  const payload:P0Create={brandId:brand.id,requestId:crypto.randomUUID(),name:name.trim(),productId:product.id,question:question.trim(),mode,targetUrl:targetUrl.trim(),scenario,maxUnits,maxCalls,transport:'fixture',evidenceMode:'simulated'};
  void submitCreate(payload);
 }
 async function mutate(action:'resume'|'pause'|'cancel'|'retry'|'step'|'approvePublish'|'reconcile'|'rollback'){
  if(!task||mutationBlocked||(['resume','retry','step','approvePublish'].includes(action)&&executionBlocked)||!begin())return;
  const confirm=['approvePublish','reconcile','rollback'].includes(action);
  try{let value:P0Task;try{value=await call<P0Task>(`p0Workflow.${action}`,{brandId:brand.id,taskId:task.id,expectedRevision:task.revision,...confirm?{confirm:true}:{}});if(alive.current)record(value)}catch(e){if(alive.current)uncertain(e);return}if(!alive.current)return;setFeedback('演练状态已保存');await refreshWorkspace()}finally{finish()}
 }
 async function updatePolicy(halt=false,release=false){
  if(!policy||mutationBlocked||(!halt&&(!!data.truncated||policyInvalid))||!begin())return;
  try{
   await call(halt?'p0Workflow.haltAll':'p0Workflow.policyUpdate',{brandId:brand.id,expectedRevision:policy.revision,...halt?{}:{dailyMaxUnits:dailyUnits,dailyMaxCalls:dailyCalls,halted:release?false:policy.halted}});
   const value=await call<P0Policy>('p0Workflow.policyGet',{brandId:brand.id});
   if(!alive.current)return;policyDirty.current=false;recordPolicy(value);setFeedback(halt?'全局停止已保存；所有任务需逐个明确继续':'模拟额度策略已保存');
   const values=await readList();if(!alive.current)return;recordList(values);if(selected.current){const t=await readTask(selected.current);if(alive.current)record(t)}
  }catch(e){if(alive.current)uncertain(e,'策略操作')}finally{finish()}
 }
 async function previewRequest(){if(previewValidation||contentBlocked||!product||!begin())return;setPreview(null);try{const result=await call<P0Preview>('p0Workflow.previewMonitor',{brandId:brand.id,productId:product.id,mode,question:question.trim()});if(alive.current)setPreview(result)}catch(e){if(alive.current)setError(errorText(e))}finally{finish()}}
 async function auditSite(){if(mutationBlocked||!begin())return;setAudit(null);try{const value=await call<P0Audit>('p0Workflow.auditFixture',{brandId:brand.id,fixture:p0SiteFixture(defaultP0Target,auditScenario)});if(alive.current)setAudit(value)}catch(e){if(alive.current)setError(errorText(e))}finally{finish()}}
 async function goReview(){if(!onNavigate||!begin())return;let loaded=false;try{await refresh();loaded=true}catch(e){if(alive.current)setError(`候选稿资料刷新失败。${errorText(e)}`)}finally{finish()}if(loaded&&alive.current)onNavigate('content')}
 async function exportTask(){if(!task||contentBlocked||stale||!begin())return;try{const result=await call<{saved:true}|null>('file.exportP0Workflow',{brandId:brand.id,taskId:task.id,expectedRevision:task.revision});if(alive.current)setFeedback(result?.saved?'演练证据 ZIP 已保存':'已取消导出，任务记录保留')}catch(e){if(alive.current)uncertain(e,'导出')}finally{finish()}}
 return <div className="stack p0-workbench">
  <Panel title="接入演练" description="单商品、单问题，模拟请求 → 人工审核 → 模拟 CMS 发布与回读 → 模拟复测。">
   <p className="notice">仅模拟夹具 fixture · {boundary}</p><p className="inline-note">不连接真实模型、站点或账号。模拟额度不代表费用，模拟结果不能证明真实平台效果。退出、恢复备份或全局停止后，需明确继续任务。</p>
   {data.truncated&&<p role="alert">资料列表不完整；创建、继续、发布批准与导出暂不可用。仍可暂停、取消或全局停止已加载的演练。</p>}
   <div className="form-grid"><Field label="已确认商品"><select disabled={contentBlocked} value={productId} onChange={e=>{setProductId(e.target.value);setPreview(null)}}><option value="">请选择商品</option>{products.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select></Field><Field label="任务名称"><input maxLength={100} disabled={contentBlocked} value={name} onChange={e=>setName(e.target.value)}/></Field></div>
   {!products.length&&<p>先在商品资料中确认当前品牌商品。</p>}
   <Field label="测试模式"><select disabled={contentBlocked} value={mode} onChange={e=>{setMode(e.target.value as P0Mode);setPreview(null)}}><option value="natural_discovery">自然发现 · 中性问题</option><option value="informed_qa">已告知问答 · 公开商品上下文</option></select></Field>
   <p className="inline-note">{mode==='natural_discovery'?'自然发现不注入品牌或商品上下文；问题不得包含品牌、别名或完整商品名。':'已告知问答会注入公开资料；不能作为自然发现证据。'}</p>
   <Field label="单个问题"><textarea rows={2} maxLength={500} disabled={contentBlocked} value={question} onChange={e=>{setQuestion(e.target.value);setPreview(null)}}/></Field>
   <Field label="模拟目标 URL" hint="仅 HTTPS .invalid 目标；本页不会打开链接。"><input disabled={contentBlocked} value={targetUrl} onChange={e=>setTargetUrl(e.target.value)}/></Field>
   <div className="form-grid"><Field label="故障场景"><select disabled={contentBlocked} value={scenario} onChange={e=>setScenario(e.target.value as P0Scenario)}><option value="success">成功路径</option><option value="rate_limit_once">一次模拟限流</option><option value="publish_timeout">发布结果未知</option><option value="readback_mismatch">回读不一致</option><option value="robots_blocked">robots 阻断</option></select></Field><Field label="单任务模拟额度"><input type="number" min={1} max={100} disabled={contentBlocked} value={maxUnits} onChange={e=>setMaxUnits(Number(e.target.value))}/></Field><Field label="单任务模拟调用上限"><input type="number" min={1} max={100} disabled={contentBlocked} value={maxCalls} onChange={e=>setMaxCalls(Number(e.target.value))}/></Field></div>
   {name&&validation&&<p role="alert">{validation}</p>}<div className="actions"><button disabled={contentBlocked||!!previewValidation} onClick={()=>void previewRequest()}>预览模拟请求</button><button className="primary" disabled={contentBlocked||!!validation} onClick={()=>void create()}>创建暂停演练</button></div>
   {preview&&<section aria-label="模拟请求预览"><p>{preview.contextIncluded?'包含公开商品上下文，仅已告知问答':'不注入品牌或商品上下文'}</p><p className="pre">{preview.request.prompt}</p><p>fixture · simulated · {boundary}</p></section>}
   {retryCreate&&<button disabled={busy||reading||!!data.truncated} onClick={()=>{if(attempt.current)void submitCreate(attempt.current,true)}}>重试原创建请求</button>}
   {uncertainCreate&&<p className="inline-note">原请求编号：{attempt.current?.requestId}。重新加载将核对此编号，不会自动重复创建。</p>}
  </Panel>
  <Panel title="工作区模拟额度与全局停止" description="策略覆盖整个工作区所有品牌；单位仅用于演练，不是金额。">
   {policy?<><p>模拟日期 {policy.day} · 策略版本 {policy.revision} · 已用模拟额度单位 {policy.usedUnits} / {policy.dailyMaxUnits} · 已用模拟调用 {policy.usedCalls} / {policy.dailyMaxCalls}</p><p>{policy.halted?'全局执行已停止':'全局可执行，任务需逐个明确继续'}</p><div className="form-grid"><Field label="工作区每日模拟额度"><input type="number" min={1} max={10000} disabled={contentBlocked} value={dailyUnits} onChange={e=>{policyDirty.current=true;setDailyUnits(Number(e.target.value))}}/></Field><Field label="工作区每日模拟调用上限"><input type="number" min={1} max={10000} disabled={contentBlocked} value={dailyCalls} onChange={e=>{policyDirty.current=true;setDailyCalls(Number(e.target.value))}}/></Field></div>{policyInvalid&&<p role="alert">工作区上限须为 1–10000 的整数。</p>}<div className="actions"><button disabled={contentBlocked||policyInvalid} onClick={()=>void updatePolicy()}>保存模拟额度策略</button><button disabled={mutationBlocked||policy.halted} onClick={()=>void updatePolicy(true)}>全局停止演练</button><button disabled={contentBlocked||!policy.halted||policyInvalid} onClick={()=>void updatePolicy(false,true)}>解除全局停止</button></div><p className="inline-note">单任务上限耗尽时请新建演练。工作区额度不足会暂停推进；调整策略或解除全局停止后，仍须逐个明确继续。</p></>:<p>正在读取模拟额度策略…</p>}
  </Panel>
  <Panel title="已保存演练任务" description="轮询只读取本地状态；推进由桌面应用执行。">
   <button disabled={busy} onClick={()=>void reload()}>重新加载演练</button><Field label="已保存演练任务"><select disabled={busy||needsReload||uncertainCreate} value={selectedId} onChange={e=>void selectTask(e.target.value)}><option value="">请选择任务</option>{tasks.map(t=><option key={t.id} value={t.id}>{t.name} · {p0StatusLabel[t.status]} · v{t.revision}</option>)}</select></Field>
   {!tasks.length&&<p>尚无演练任务，创建后保持暂停。</p>}{reading&&<p role="status">正在读取演练任务…</p>}
   {task&&<section aria-label="演练任务明细"><h3>{task.name}</h3><p>状态：{p0StatusLabel[task.status]}</p><p>任务版本 {task.revision} · 商品版本 {task.productRevision} · {task.mode==='natural_discovery'?'自然发现':'已告知问答'}</p><p>模拟额度单位 {task.budget.usedUnits} / {task.budget.maxUnits} · 模拟调用 {task.budget.usedCalls} / {task.budget.maxCalls}</p><p>{boundary}</p><p>问题：{task.question}</p><p className="p0-wrap">模拟目标：{task.targetUrl}</p>
    {stale&&<p role="alert">商品资料已变化或不再确认；请新建演练。仍可暂停、取消或核对原模拟操作。</p>}{task.error&&<p role="alert">{task.error}</p>}
    <div className="actions"><button disabled={executionBlocked||!['paused','awaiting_review','budget_blocked'].includes(task.status)} onClick={()=>void mutate('resume')}>开始 / 继续演练</button><button disabled={mutationBlocked||task.status!=='running'} onClick={()=>void mutate('pause')}>暂停演练</button><button disabled={mutationBlocked||['completed','cancelled'].includes(task.status)} onClick={()=>void mutate('cancel')}>取消演练</button><button disabled={executionBlocked||!['failed','retry_wait'].includes(task.status)} onClick={()=>void mutate('retry')}>显式重试失败步骤</button><button disabled={executionBlocked||task.status!=='paused'} onClick={()=>void mutate('step')}>推进一个模拟步骤</button><button disabled={contentBlocked||stale||task.status!=='completed'} onClick={()=>void exportTask()}>导出演练证据 ZIP</button></div>
    {(task.draftId||task.status==='awaiting_review')&&<div className="cohort"><p>候选稿须在内容页人工审核；本页不会自动批准内容。内容通过后，还须单独批准当前版本的模拟发布。</p><p>候选稿编号：{task.draftId||'待生成'}</p>{onNavigate&&<button disabled={contentBlocked} onClick={()=>void goReview()}>前往内容审核</button>}</div>}
    {task.status==='awaiting_publish_approval'&&<div className="cohort"><label className="p0-confirm"><input type="checkbox" checked={publishConfirm} disabled={executionBlocked} onChange={e=>setPublishConfirm(e.target.checked)}/>我已核对当前商品、候选稿和模拟目标，仅批准此版本模拟发布</label><button disabled={executionBlocked||!publishConfirm} onClick={()=>void mutate('approvePublish')}>批准当前版本模拟发布</button><p>批准绑定当前商品、稿件与目标哈希；审核节点完成后若再次修改稿件，请取消旧任务并新建演练，重新审核与批准。</p></div>}
    {task.status==='awaiting_reconcile'&&<div className="cohort"><p>模拟发布结果未知。必须核对原操作编号，不能重复发布。</p><label className="p0-confirm"><input type="checkbox" checked={reconcileConfirm} disabled={mutationBlocked} onChange={e=>setReconcileConfirm(e.target.checked)}/>确认核对原操作，不创建新的发布请求</label><button disabled={mutationBlocked||!reconcileConfirm} onClick={()=>void mutate('reconcile')}>核对原发布操作</button></div>}
    {task.remote&&<div className="cohort"><h4>模拟 CMS 状态与回读</h4><Json value={task.remote} label="模拟 CMS 数据"/><label className="p0-confirm"><input type="checkbox" checked={rollbackConfirm} disabled={mutationBlocked} onChange={e=>setRollbackConfirm(e.target.checked)}/>确认回滚此模拟发布，仅处理原操作记录</label><button disabled={mutationBlocked||!rollbackConfirm||task.status==='awaiting_reconcile'} onClick={()=>void mutate('rollback')}>回滚模拟发布</button></div>}
    {task.approval&&<><h4>当前模拟发布批准记录</h4><Json value={task.approval} label="模拟发布批准记录"/></>}
    <h4>步骤证据</h4><ol className="queue-list">{task.nodes.map(node=><li className="cohort" key={node.id}><h4>{p0NodeLabel[node.kind]} · {{pending:'等待',completed:'已完成',blocked:'待处理',failed:'失败'}[node.status]}</h4>{node.error&&<p role="alert">{node.error}</p>}{node.result&&(node.kind==='audit'&&node.result.scope==='fixture_site'?<Audit value={node.result as unknown as P0Audit}/>:<Json value={node.result} label={`${p0NodeLabel[node.kind]}证据`}/>)}</li>)}</ol><h4>模拟操作账本</h4><Json value={task.ledger} label="模拟操作账本"/>
   </section>}
  </Panel>
  <Panel title="站点夹具审计" description="检查内置字符串夹具，不执行 HTML，不访问目标网址。">
   <Field label="站点审计夹具"><select disabled={mutationBlocked} value={auditScenario} onChange={e=>{setAuditScenario(e.target.value as typeof auditScenario);setAudit(null)}}><option value="success">正常可见正文</option><option value="robots_blocked">robots 禁止抓取</option><option value="hidden_text">仅注释或脚本包含正文</option></select></Field><button disabled={mutationBlocked} onClick={()=>void auditSite()}>检查站点夹具</button>{audit&&<Audit value={audit}/>}
  </Panel>
  {busy&&<p role="status">正在处理本地演练资料…</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
