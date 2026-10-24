import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import type {WorkspaceSnapshot} from '../types';
import {Field,Panel,useTask} from '../components/ui';
import CsvDiagnostics,{checkedDiagnostic} from '../components/CsvDiagnostics';
import {csvMappingKeys,type CsvDelimiter,type CsvDiagnostic,type CsvMapping,type CsvMappingKey,type CsvMetadata,type CsvPrepared,type CsvTemplate,type LocalJob,type LocalJobCreated,type LocalJobSummary,type StagedEntry} from '../operationsTypes';

const emptyMapping=():CsvMapping=>Object.fromEntries(csvMappingKeys.map(key=>[key,null])) as CsvMapping;
const initialMetadata=():CsvMetadata=>({evidenceMode:'simulated',sourceLabel:'',currency:'CNY',amountUnit:'minor',coverageStart:'2026-10-01T00:00:00Z',coverageEnd:'2026-11-01T00:00:00Z'});
const mappingLabels:Record<CsvMappingKey,string>={kind:'记录类型',id:'记录编号',sessionId:'会话编号',sku:'商品 SKU',occurredAt:'发生时间',source:'来源',sourceEvidence:'来源证据',status:'订单状态',amount:'金额',orderId:'原订单编号'};
const stateLabels={paused:'已暂停',ready:'等待手动继续',completed:'已完成',cancelled:'已取消',pending:'待处理',succeeded:'成功',failed:'失败'};
const mode=(value:string)=>value==='simulated'?'模拟数据':'记录数据（导入者声明）';
function contentNature(content:string){try{const value=JSON.parse(content) as {evidenceMode?:string;currency?:string;sourceLabel?:string};if(['simulated','recorded'].includes(value.evidenceMode??'')&&['CNY','USD','EUR'].includes(value.currency??''))return `${mode(value.evidenceMode!)} · ${value.currency} · ${typeof value.sourceLabel==='string'?value.sourceLabel:''}`}catch{/* Core validates persisted exchanges; tolerate an unavailable display label. */}return '请展开查看该份资料的性质与币种'}
const message=(error:unknown)=>error instanceof Error?error.message:'本地操作未完成，请重新加载核对';
function summarize(job:LocalJob):LocalJobSummary{return {id:job.id,name:job.name,status:job.status,revision:job.revision,createdAt:job.createdAt,updatedAt:job.updatedAt,total:job.entries.length,succeeded:job.entries.filter(e=>e.status==='succeeded').length,failed:job.entries.filter(e=>e.status==='failed').length,pending:job.entries.filter(e=>e.status==='pending').length,cancelled:job.entries.filter(e=>e.status==='cancelled').length}}

export default function OperationsWorkbench({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 return <OperationsSession key={brandId} brandId={brandId} data={data}/>;
}
function OperationsSession({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 const [name,setName]=useState(''),[csv,setCsv]=useState(''),[delimiter,setDelimiter]=useState<CsvDelimiter>('comma');
 const [metadata,setMetadata]=useState(initialMetadata),[mapping,setMapping]=useState(emptyMapping),[headers,setHeaders]=useState<string[]>([]),[rowCount,setRowCount]=useState(0),[prepared,setPrepared]=useState<CsvPrepared|null>(null);
 const [diagnostic,setDiagnostic]=useState<CsvDiagnostic|null>(null);
 const [staged,setStaged]=useState<StagedEntry[]>([]),[jobName,setJobName]=useState(''),[jobs,setJobs]=useState<LocalJobSummary[]>([]),[selectedId,setSelectedId]=useState(''),[job,setJob]=useState<LocalJob|null>(null);
 const [readBusy,setReadBusy]=useState(false),[listBusy,setListBusy]=useState(false),[listError,setListError]=useState(''),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const [controlBusy,setControlBusy]=useState(false),[running,setRunning]=useState(false),[waitingControl,setWaitingControl]=useState(false),[needsJobReload,setNeedsJobReload]=useState(false);
 const alive=useRef(true),readEpoch=useRef(0),listEpoch=useRef(0),selectionEpoch=useRef(0),readLock=useRef(false),controlLock=useRef(false),run=useRef(false),intent=useRef<'pause'|'cancel'|null>(null),jobRef=useRef<LocalJob|null>(null),selectedRef=useRef('');
 const createTask=useTask();
 const products=data.products.filter(p=>p.brandId===brandId&&p.sku.trim());
 const catalog=JSON.stringify(data.products.filter(p=>p.brandId===brandId).map(p=>[p.id,p.sku,p.name,p.revision]));
 const bytes=new TextEncoder().encode(csv).length;
 const canInspect=!!csv.trim()&&bytes<=512*1024;
 const canPrepare=canInspect&&headers.length>0&&!!products.length&&!data.truncated;
 const pending=!!job?.entries.some(e=>e.status==='pending');
 const retryable=!!job?.entries.some(e=>e.status==='failed'&&e.attempts<5);
 function dirty(){readEpoch.current++;readLock.current=false;setReadBusy(false);setPrepared(null);setDiagnostic(null);setError('');setFeedback('')}
 function dirtyCsv(){dirty();setHeaders([]);setRowCount(0)}
 useEffect(()=>{dirty()},[catalog,data.truncated]);
 async function reloadList(){
  if(!alive.current)return;
  const seq=++listEpoch.current;setListBusy(true);setListError('');
  try{const result=await call<{jobs:LocalJobSummary[]}>('localJobs.list',{brandId});if(alive.current&&seq===listEpoch.current)setJobs(result.jobs)}
  catch(e){if(alive.current&&seq===listEpoch.current)setListError(message(e));throw e}
  finally{if(alive.current&&seq===listEpoch.current)setListBusy(false)}
 }
 useEffect(()=>{alive.current=true;void reloadList().catch(()=>{});return()=>{alive.current=false;run.current=false;intent.current=null;readEpoch.current++;listEpoch.current++;selectionEpoch.current++}},[]);
 async function read<T>(fn:()=>Promise<T>,done:(value:T)=>void){
  if(readLock.current||controlLock.current||createTask.busy)return;
  const seq=++readEpoch.current;readLock.current=true;setReadBusy(true);setError('');setFeedback('');
  try{const result=await fn();if(alive.current&&seq===readEpoch.current)done(result)}
  catch(e){if(alive.current&&seq===readEpoch.current){setPrepared(null);setDiagnostic(null);setError(message(e))}}
  finally{if(alive.current&&seq===readEpoch.current){readLock.current=false;setReadBusy(false)}}
 }
 function recordJob(value:LocalJob){jobRef.current=value;setJob(value);setJobs(old=>{const item=summarize(value);return old.some(j=>j.id===value.id)?old.map(j=>j.id===value.id?item:j):[item,...old]})}
 async function selectJob(id:string){
  if(controlLock.current||createTask.busy)return;
  const seq=++selectionEpoch.current;selectedRef.current=id;setSelectedId(id);jobRef.current=null;setJob(null);setNeedsJobReload(false);setError('');setFeedback('');
  if(!id)return;
  setControlBusy(true);controlLock.current=true;
  try{const value=await call<LocalJob>('localJobs.get',{brandId,jobId:id});if(alive.current&&seq===selectionEpoch.current&&selectedRef.current===id)recordJob(value)}
  catch(e){if(alive.current&&seq===selectionEpoch.current){setError(message(e));setNeedsJobReload(true)}}
  finally{controlLock.current=false;if(alive.current&&seq===selectionEpoch.current)setControlBusy(false)}
 }
 function current(id:string){return alive.current&&selectedRef.current===id}
 async function mutate(action:string,value:LocalJob){return call<LocalJob>(`localJobs.${action}`,{brandId,jobId:value.id,expectedRevision:value.revision})}
 // An uncertain mutation is reconciled by one read, never by repeating the mutation.
 async function reconcile(id:string,step:boolean,e:unknown){
  run.current=false;intent.current=null;
  if(!current(id))return;
  setNeedsJobReload(true);setError(`${step?'步骤响应未确认':'操作响应未确认'}；已停止后续处理。${message(e)}`);
  try{const value=await call<LocalJob>('localJobs.get',{brandId,jobId:id});if(current(id)){recordJob(value);setNeedsJobReload(false);setFeedback('已重新加载已保存进度；核对后请手动继续，不会自动重试。')}}
  catch{if(current(id))setFeedback('进度加载失败，请先重新加载当前队列。')}
 }
 async function start(){
  let value=jobRef.current;if(!value||controlLock.current||createTask.busy||needsJobReload||!value.entries.some(e=>e.status==='pending')||!['paused','ready'].includes(value.status))return;
  const id=value.id;controlLock.current=true;run.current=true;intent.current=null;setControlBusy(true);setRunning(true);setError('');setFeedback('');
  let step=false;
  try{
   if(value.status==='paused'){value=await mutate('resume',value);if(!current(id))return;recordJob(value)}
   while(current(id)&&run.current&&value.status==='ready'&&value.entries.some(e=>e.status==='pending')){
    step=true;value=await mutate('step',value);if(!current(id))return;recordJob(value);step=false;
   }
   if(!current(id))return;
   const requested=intent.current;intent.current=null;
   if(requested==='cancel'&&value.entries.some(e=>e.status==='pending')&&['ready','paused'].includes(value.status)){value=await mutate('cancel',value);if(!current(id))return;recordJob(value);setFeedback('已取消剩余条目，已保存结果保留')}
   else if(requested==='pause'&&value.status==='ready'){value=await mutate('pause',value);if(!current(id))return;recordJob(value);setFeedback('已暂停，等待手动继续')}
   else if(value.status==='completed')setFeedback('本次队列处理结束；请逐条核对成功与失败结果。');
  }catch(e){await reconcile(id,step,e)}
  finally{run.current=false;controlLock.current=false;if(current(id)){setControlBusy(false);setRunning(false);setWaitingControl(false)}}
 }
 async function control(action:'pause'|'cancel'|'retry'){
  const value=jobRef.current;if(!value||createTask.busy||needsJobReload)return;
  if(controlLock.current){if(run.current||running){run.current=false;intent.current=action==='cancel'?'cancel':'pause';setWaitingControl(true);setFeedback('正在等待当前条目返回，随后处理请求…')}return}
  controlLock.current=true;setControlBusy(true);setError('');setFeedback('');
  try{const saved=await mutate(action,value);if(current(value.id)){recordJob(saved);setFeedback(action==='cancel'?'已取消剩余条目，已保存结果保留':action==='retry'?'失败项已转为待处理，等待手动开始':'已暂停，等待手动继续')}}
  catch(e){await reconcile(value.id,false,e)}
  finally{controlLock.current=false;if(current(value.id))setControlBusy(false)}
 }
 function create(){
  if(!staged.length||staged.length>20||!jobName.trim()||controlLock.current||readLock.current||createTask.busy)return;
  const entries=staged.map(({name,content,expectedHash})=>({name,content,expectedHash}));
  void createTask.runPersisted(async()=>{const saved=await call<LocalJobCreated>('localJobs.create',{brandId,name:jobName.trim(),entries});if(alive.current){setStaged([]);setJobName('')}return saved},reloadList,saved=>{if(!alive.current)return;selectedRef.current=saved.id;setSelectedId(saved.id);setNeedsJobReload(false);recordJob(saved)},saved=>saved.alreadyExists?'已复用已有队列，等待手动开始':'队列已创建，等待手动开始');
 }
 const busy=readBusy||controlBusy||createTask.busy;
 return <div className="attribution-workbench operations-workbench">
 <Panel title="批量运营 · 本地 CSV 映射" description="先明确字段和数据性质，再准备预览并逐份加入待处理列表。">
 <p className="notice">仅在本机处理，不连接网站或账号，不发布、不调用模型。模型调用 0 次，模型费用 0。各数据集独立保留性质、币种与金额，不能合并解读为销量增量。</p>
 <p className="inline-note">CSV 最大 512 KiB、40 列、1000 数据行。只使用不透明编号，禁止个人信息、原始网址和设备标识。未映射列不会保存。</p>
 {!products.length&&<p className="inline-note">请先为当前品牌创建带 SKU 的商品。</p>}{data.truncated&&<p role="alert">商品快照不完整，请重新加载后再准备资料。</p>}
 <fieldset className="task-fields" disabled={controlBusy||createTask.busy}>
 <div className="actions"><button disabled={busy||!products.length||!!data.truncated} onClick={()=>void read(()=>call<CsvTemplate>('attribution.csvTemplate',{brandId}),value=>{dirtyCsv();setCsv(value.csv);setName(value.name);setDelimiter(value.delimiter);setMapping(value.mapping);setMetadata(value.metadata);setFeedback('已填入虚构模拟 CSV，尚未预览、加入列表或保存')})}>填入虚构CSV</button><button disabled={busy} onClick={()=>void read(()=>call<{name:string;content:string}|null>('file.openText',{kind:'csv'}),file=>{if(!file){setFeedback('已取消选择，保留当前内容');return}dirtyCsv();setCsv(file.content);setName(file.name.replace(/\.csv$/i,''));setMapping(emptyMapping());setFeedback('已读取本地 CSV，请检查表头并明确映射')})}>选择本地 CSV</button></div>
 <Field label="数据集名称"><input maxLength={100} value={name} onChange={e=>{dirty();setName(e.target.value)}}/></Field>
 <Field label="CSV 内容"><textarea rows={8} value={csv} onChange={e=>{dirtyCsv();setCsv(e.target.value)}}/></Field>
 {bytes>512*1024&&<p role="alert">CSV 超过 512 KiB，请缩小文件后重试。</p>}
 <div className="form-grid"><Field label="分隔符"><select value={delimiter} onChange={e=>{dirtyCsv();setDelimiter(e.target.value as CsvDelimiter)}}><option value="comma">逗号</option><option value="tab">制表符</option></select></Field><Field label="数据性质"><select value={metadata.evidenceMode} onChange={e=>{dirty();setMetadata({...metadata,evidenceMode:e.target.value as CsvMetadata['evidenceMode']})}}><option value="simulated">模拟数据</option><option value="recorded">记录数据（导入者声明）</option></select></Field><Field label="币种"><select value={metadata.currency} onChange={e=>{dirty();setMetadata({...metadata,currency:e.target.value as CsvMetadata['currency']})}}>{['CNY','USD','EUR'].map(value=><option key={value}>{value}</option>)}</select></Field><Field label="金额单位" hint="分为整数；元只接受最多两位小数，不舍入。"><select value={metadata.amountUnit} onChange={e=>{dirty();setMetadata({...metadata,amountUnit:e.target.value as CsvMetadata['amountUnit']})}}><option value="minor">分（整数最小单位）</option><option value="major">元（币种主要单位）</option></select></Field></div>
 <Field label="资料来源"><input maxLength={100} value={metadata.sourceLabel} onChange={e=>{dirty();setMetadata({...metadata,sourceLabel:e.target.value})}}/></Field>
 <div className="form-grid"><Field label="覆盖开始时间" hint="含时区的整秒 RFC3339，例如 2026-10-01T00:00:00Z。"><input value={metadata.coverageStart} onChange={e=>{dirty();setMetadata({...metadata,coverageStart:e.target.value})}}/></Field><Field label="覆盖结束时间（不含）"><input value={metadata.coverageEnd} onChange={e=>{dirty();setMetadata({...metadata,coverageEnd:e.target.value})}}/></Field></div>
 <div className="actions"><button disabled={busy||!canInspect} onClick={()=>{setPrepared(null);setDiagnostic(null);setHeaders([]);setRowCount(0);void read(()=>call<unknown>('attribution.csvInspectDetailed',{brandId,csv,delimiter}),result=>{const value=checkedDiagnostic(result,'structure');setDiagnostic(value);if(value.status!=='ready')return;setHeaders(value.headers);setRowCount(value.rowCount!);setMapping(old=>Object.fromEntries(csvMappingKeys.map(key=>[key,old[key]&&value.headers.includes(old[key]!)?old[key]:null])) as CsvMapping)})}}>检查 CSV 表头</button></div>
 {!!headers.length&&<><p className="inline-note">已检查 {rowCount} 条数据记录。记录类型必须为 visits / orders / refunds；同一列只能映射一次。访问必需会话和 SKU；订单必需 SKU、状态、金额；退款必需原订单编号、金额。时间必须明确时区，来源和证据成对填写。</p><div className="form-grid">{csvMappingKeys.map(key=><Field key={key} label={`映射：${mappingLabels[key]}`}><select value={mapping[key]??''} onChange={e=>{dirty();setMapping({...mapping,[key]:e.target.value||null})}}><option value="">{['kind','id','occurredAt'].includes(key)?'必须选择表头':'不映射'}</option>{headers.map(header=><option key={header} value={header}>{header}</option>)}</select></Field>)}</div></>}
 <div className="actions"><button disabled={busy||!canPrepare} onClick={()=>{setPrepared(null);setDiagnostic(null);void read(()=>call<unknown>('attribution.csvDiagnose',{brandId,name:name.trim(),csv,delimiter,mapping,metadata}),result=>{const value=checkedDiagnostic(result,'mapped');setDiagnostic(value);if(!value.headers.length){setHeaders([]);setRowCount(0)}if(value.status==='ready'&&value.prepared)setPrepared(value.prepared)})}}>准备映射预览</button><button disabled={busy||!prepared||staged.length>=20} onClick={()=>{if(!prepared||staged.length>=20)return;setStaged(old=>[...old,{name:name.trim(),content:prepared.content,expectedHash:prepared.preview.contentHash,preview:structuredClone(prepared.preview)}]);setFeedback('已加入独立待处理资料，尚未创建队列')}}>加入待处理列表</button></div>
 </fieldset>
 {diagnostic&&<CsvDiagnostics value={diagnostic}/>}
 {prepared&&<section aria-label="映射预览" className="cohort"><h3>映射预览 · {mode(prepared.preview.evidenceMode)}</h3><p>{prepared.preview.sourceLabel} · {prepared.preview.currency} · [{prepared.preview.coverageStart}, {prepared.preview.coverageEnd})</p><p>访问 {prepared.preview.counts.visits} · 订单 {prepared.preview.counts.orders} · 退款 {prepared.preview.counts.refunds} · 去重 {prepared.preview.counts.duplicates}</p><p className="inline-note">内容指纹：{prepared.preview.contentHash}</p><details><summary>只读规范化内容与商品快照</summary><Field label="规范化内容"><textarea rows={7} readOnly value={prepared.content}/></Field><ul>{prepared.preview.products.map(p=><li key={p.id}>{p.name} · {p.sku} · v{p.revision}</li>)}</ul></details></section>}
 </Panel>
 <Panel title="待处理列表" description="加入的资料为不可变副本。创建后默认为暂停，须手动开始。">
 <p>待处理：{staged.length} / 20</p>
 <ul className="queue-list">{staged.map((entry,index)=><li key={index}><strong>{entry.name}</strong><p>{mode(entry.preview.evidenceMode)} · {entry.preview.currency} · {entry.preview.sourceLabel}</p><p className="inline-note">{entry.expectedHash}</p><button disabled={busy} aria-label={`移除待处理资料 ${index+1}`} onClick={()=>setStaged(old=>old.filter((_,i)=>i!==index))}>移除</button></li>)}</ul>
 <Field label="队列名称"><input maxLength={100} disabled={createTask.busy||controlBusy} value={jobName} onChange={e=>setJobName(e.target.value)}/></Field><div className="actions"><button className="primary" disabled={busy||!staged.length||!jobName.trim()} onClick={create}>创建暂停队列</button></div>{createTask.feedback}
 </Panel>
 <Panel title="已保存队列" description="每次只处理一个条目。离开页面或切换品牌后停止后续步骤，回来须手动继续。">
 <div className="actions"><button disabled={busy||listBusy} onClick={()=>void reloadList().catch(()=>{})}>重新加载队列列表</button></div>{listBusy&&<p role="status">正在加载队列…</p>}{listError&&<p role="alert">{listError}</p>}
 <Field label="已保存队列"><select disabled={busy} value={selectedId} onChange={e=>void selectJob(e.target.value)}><option value="">请选择队列</option>{jobs.map(item=><option key={item.id} value={item.id}>{item.name} · {stateLabels[item.status]} · 成功 {item.succeeded}/{item.total} · 失败 {item.failed}</option>)}</select></Field>
 {!listBusy&&!listError&&!jobs.length&&<p className="inline-note">尚无已保存队列。</p>}
 {selectedId&&<div className="actions"><button disabled={busy} onClick={()=>void selectJob(selectedId)}>重新加载当前队列</button></div>}
 {job&&<section aria-label="队列明细" className="cohort"><h3>{job.name}</h3><p>状态：<span>{stateLabels[job.status]}</span> · 版本 {job.revision}</p><p className="inline-note">手动开始后按条顺序执行；暂停与取消会等待当前条目返回。失败最多尝试 5 次；成功结果与取消记录保留，不汇总不同资料的销售指标。</p>
 <div className="actions"><button className="primary" disabled={busy||needsJobReload||!pending||!['paused','ready'].includes(job.status)} onClick={()=>void start()}>开始/继续</button><button disabled={createTask.busy||needsJobReload||waitingControl||(!running&&(controlBusy||job.status!=='ready'))} onClick={()=>void control('pause')}>暂停</button><button disabled={createTask.busy||needsJobReload||waitingControl||!pending||!['paused','ready'].includes(job.status)||(controlBusy&&!running)} onClick={()=>void control('cancel')}>取消剩余</button><button disabled={busy||needsJobReload||!retryable||!['paused','completed'].includes(job.status)} onClick={()=>void control('retry')}>重试失败项</button><button disabled={busy||needsJobReload} onClick={()=>void read(()=>call<{path:string}|null>('file.exportLocalJob',{brandId,jobId:job.id}),result=>setFeedback(result?'队列证据 ZIP 已保存':'已取消导出，队列记录保留'))}>导出队列证据 ZIP</button></div>
 <ol className="queue-list">{job.entries.map(entry=><li key={entry.id}><strong>{entry.name} · {stateLabels[entry.status]}</strong><p>{contentNature(entry.content)}</p><p>尝试 {entry.attempts} 次 / 5</p>{entry.datasetId&&<p>已保存数据集：{entry.datasetId}{entry.alreadyImported?'（复用已有数据集）':''}</p>}{entry.error&&<p role="alert">{entry.error}</p>}<details><summary>独立资料与冻结快照</summary><p className="inline-note">内容指纹：{entry.expectedHash}</p><ul>{entry.products.map(p=><li key={p.id}>{p.name} · {p.sku} · v{p.revision}</li>)}</ul><Field label={`规范化资料：${entry.name}`}><textarea rows={5} readOnly value={entry.content}/></Field></details></li>)}</ol>
 </section>}
 </Panel>
 {readBusy&&<p role="status">正在本地处理…</p>}{running&&<p role="status">{waitingControl?'正在等待当前条目结束…':'正在逐条处理本地队列…'}</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
