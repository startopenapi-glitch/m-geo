import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import {Field,Panel} from '../components/ui';
import type {WorkspaceSnapshot,EvidenceMode} from '../types';
import type {OptimizationAction,OptimizationCreated,OptimizationCreatePayload,OptimizationDiagnosis,OptimizationPlan,OptimizationPlanSummary,OptimizationSection,OptimizationStatus} from '../optimizationTypes';

type Props={brandId:string;data:WorkspaceSnapshot;onNavigate?:(section:OptimizationSection)=>void};
const evidenceLabel=(mode:EvidenceMode)=>mode==='simulated'?'模拟数据':mode==='recorded'?'记录数据（记录者声明）':'未声明数据性质';
const errorText=(error:unknown)=>error instanceof Error?error.message:'本地读取失败，请重新加载核对';
const statuses:Record<OptimizationStatus,string>={open:'待处理',in_progress:'处理中',done:'已处理，等待验证',dismissed:'已忽略（人工说明）'};
function summarize(plan:OptimizationPlan):OptimizationPlanSummary{return {id:plan.id,requestId:plan.requestId,name:plan.name,campaignId:plan.campaignId,evidenceMode:plan.evidenceMode,revision:plan.revision,createdAt:plan.createdAt,updatedAt:plan.updatedAt,checkpointCount:plan.checkpoints.length,actionCount:plan.actions.length,doneCount:plan.actions.filter(a=>a.status==='done').length}}

function Metrics({diagnosis,label}:{diagnosis:OptimizationDiagnosis;label:string}){
 const {metrics:m}=diagnosis;
 return <section aria-label={label} className="optimization-metrics cohort"><h3>{label} · {evidenceLabel(diagnosis.evidenceMode)}</h3><p>冻结日期：{diagnosis.asOf} · 诊断阶段：{diagnosis.phase==='baseline'?'基线':'复测'}</p>
 <p>基线样本：{m.baseline.samples} / {m.expectedPerPhase} · 复测样本：{m.followup.samples} / {m.expectedPerPhase}</p>
 {(['baseline','followup'] as const).map(phase=>{const value=m[phase];return <p key={phase}>{phase==='baseline'?'基线':'复测'} · {evidenceLabel(diagnosis.evidenceMode)}：已判读 {value.assessed}，未知 {value.unknown}；推荐 {value.recommended} / {value.assessed}；有排名 {value.ranked}，前三 {value.top3} / {value.ranked}；引用 {value.cited} / {value.samples}；不准确 {value.incorrect} / {value.samples}</p>})}
 <p>已配对样本：{m.matchedPairs} · 已配对且已判读：{m.matchedAssessedPairs}；配对基线推荐 {m.matchedBaselineRecommended} / {m.matchedAssessedPairs}，配对复测推荐 {m.matchedFollowupRecommended} / {m.matchedAssessedPairs}</p>
 <p>配对推荐率差：{m.deltaPercentagePoints===null?'无法计算（缺少已判读配对）':`${m.deltaPercentagePoints.toFixed(2)} 个百分点`} · {evidenceLabel(diagnosis.evidenceMode)}</p>
 <p className="inline-note">未知不计为否；没有排名不代表前三以外。差值只使用已配对且已判读分母，不能证明因果增量、收录或真实平台成效。</p></section>;
}
type FrozenProduct=OptimizationDiagnosis['products'][number];
function targetLabel(productId:string|null,product:FrozenProduct|undefined){return product?`商品：${product.name} · SKU：${product.sku||'未填写'} · ID：${product.id}`:productId?`商品 ID：${productId}（冻结名称/SKU缺失）`:'固定测量计划（非单一商品）'}
function actionTarget(plan:OptimizationPlan,action:OptimizationAction){return plan.checkpoints.find(cp=>cp.diagnosis.findings.some(f=>f.key===action.key))?.diagnosis.products.find(p=>p.id===action.productId)}
function ActionEditor({action,target,currentTarget,detected,disabled,onSave,onNavigate}:{action:OptimizationAction;target:string;currentTarget:string;detected:boolean;disabled:boolean;onSave:(status:OptimizationStatus,note:string)=>void;onNavigate?:Props['onNavigate']}){
 const [draft,setDraft]=useState({status:action.status,note:action.note,baseStatus:action.status,baseNote:action.note,conflict:false});
 useEffect(()=>{setDraft(old=>{
  if(old.baseStatus===action.status&&old.baseNote===action.note)return old;
  const matches=old.status===action.status&&old.note===action.note,dirty=old.status!==old.baseStatus||old.note!==old.baseNote;
  if(dirty&&!matches)return {...old,conflict:true};
  return {status:action.status,note:action.note,baseStatus:action.status,baseNote:action.note,conflict:false};
 })},[action.status,action.note]);
 const {status,note,conflict}=draft;
 const valid=note.length<=1000&&(!['done','dismissed'].includes(status)||!!note.trim());
 const changed=status!==action.status||note!==action.note,label=`${action.title} · ${target} · ${action.key}`;
 return <li className="cohort" style={{minWidth:0,overflowWrap:'anywhere'}} aria-label={`行动：${label}`}><h4>{action.title} · {action.priority==='high'?'优先处理':'常规'}</h4><p>首次冻结目标：{target}</p>{target!==currentTarget&&<p>最新检查点目标：{currentTarget}</p>}<p>{action.detail}</p><p>人工状态：{statuses[action.status]}</p><p>最新检查点：{detected?'仍检测到':'未检测到'}</p><p className="inline-note">首次证据引用：{action.evidenceIds.length?action.evidenceIds.join('、'):'无已有证据编号（请按问题说明补全）'}</p>
 <Field label={`处理状态：${label}`}><select disabled={disabled} value={status} onChange={e=>setDraft(old=>({...old,status:e.target.value as OptimizationStatus}))}>{(Object.entries(statuses) as [OptimizationStatus,string][]).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
 <Field label={`处理备注：${label}`} hint="最多 1000 字；标记已处理或忽略必须说明。备注由用户填写，会包含在导出文件中。"><textarea rows={3} maxLength={1000} disabled={disabled} value={note} onChange={e=>setDraft(old=>({...old,note:e.target.value}))}/></Field>
 {conflict&&<div role="alert"><p>此行动的保存版本已变化，本地草稿已保留。请核对保存版本：{statuses[action.status]}；备注：{action.note||'空'}。</p><button disabled={disabled} aria-label={`保留本地草稿并以当前版本继续：${label}`} onClick={()=>setDraft(old=>({...old,baseStatus:action.status,baseNote:action.note,conflict:false}))}>保留本地草稿并以当前版本继续</button><button disabled={disabled} aria-label={`使用已保存版本：${label}`} onClick={()=>setDraft({status:action.status,note:action.note,baseStatus:action.status,baseNote:action.note,conflict:false})}>使用已保存版本（放弃此草稿）</button></div>}
 <div className="actions"><button disabled={disabled||conflict||!valid||!changed} aria-label={`保存行动：${label}`} onClick={()=>onSave(status,note)}>保存行动</button>{onNavigate&&<button disabled={disabled} aria-label={`前往来源区域：${label}`} onClick={()=>onNavigate(action.section)}>前往来源区域</button>}</div></li>;
}

export default function OptimizationWorkbench(props:Props){return <OptimizationSession key={props.brandId} {...props}/>}
function OptimizationSession({brandId,data,onNavigate}:Props){
 const [campaignId,setCampaignId]=useState(''),[name,setName]=useState(''),[preview,setPreview]=useState<OptimizationDiagnosis|null>(null);
 const [plans,setPlans]=useState<OptimizationPlanSummary[]>([]),[selectedId,setSelectedId]=useState(''),[plan,setPlan]=useState<OptimizationPlan|null>(null);
 const [busy,setBusy]=useState(false),[listBusy,setListBusy]=useState(false),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const [needsReload,setNeedsReload]=useState(false),[refreshFailed,setRefreshFailed]=useState(false),[uncertainCreate,setUncertainCreate]=useState(false),[retryOriginal,setRetryOriginal]=useState(false);
 const alive=useRef(true),lock=useRef(false),listSequence=useRef(0),readSequence=useRef(0),attempt=useRef<OptimizationCreatePayload|null>(null);
 const campaigns=(data.campaigns??[]).filter(c=>c.brandId===brandId);
 const signature=JSON.stringify([campaigns,data.products.map(p=>[p.id,p.revision,p.confirmedAt]),data.drafts.map(d=>[d.id,d.revision,d.sourceRevision,d.reviewedAt]),data.observations.map(o=>[o.id,o.assessmentRevision])]);
 useEffect(()=>{setPreview(null);readSequence.current++},[signature]);
 function record(value:OptimizationPlan){setPlan(value);setSelectedId(value.id);setPlans(old=>{const item=summarize(value);return old.some(p=>p.id===item.id)?old.map(p=>p.id===item.id?item:p):[item,...old]})}
 async function fetchList(){const seq=++listSequence.current;setListBusy(true);try{const values=await call<OptimizationPlanSummary[]>('optimization.list',{brandId});if(alive.current&&seq===listSequence.current)setPlans(values);return values}finally{if(alive.current&&seq===listSequence.current)setListBusy(false)}}
 useEffect(()=>{alive.current=true;void fetchList().catch(e=>{if(alive.current)setError(`计划列表读取失败，请重新加载计划列表。${errorText(e)}`)});return()=>{alive.current=false;listSequence.current++;readSequence.current++}},[]);
 function begin(){if(lock.current||listBusy||!alive.current)return false;lock.current=true;setBusy(true);setError('');setFeedback('');return true}
 function finish(){lock.current=false;if(alive.current)setBusy(false)}
 async function reloadList(){
  if(!begin())return;
  try{const values=await fetchList();if(!alive.current)return;
   // Read the affected current plan before reconciling a retained create request.
   if(needsReload&&attempt.current){setFeedback('计划列表已刷新；请先重新加载当前计划核对操作结果，再核对保留的创建请求。');return}
   const original=attempt.current;
   if(original){
    const match=values.find(p=>p.requestId===original.requestId);
    setRetryOriginal(false);
    if(match){const saved=await call<OptimizationPlan>('optimization.get',{brandId,planId:match.id});if(!alive.current)return;record(saved);attempt.current=null;setUncertainCreate(false);setNeedsReload(false);setPreview(null);setName('');setFeedback('已找到原请求保存的计划，请核对明细')}
    else{setUncertainCreate(true);setRetryOriginal(true);setFeedback('未找到原请求；可重试原内容，或保留原编号重新诊断编辑')}
   }else{setRefreshFailed(false);setFeedback('已重新加载计划列表')}
  }catch(e){if(alive.current){setRetryOriginal(false);if(attempt.current&&!needsReload)setUncertainCreate(true);setError(`重新加载失败，请再次加载核对；不会自动提交。${errorText(e)}`)}}finally{finish()}
 }
 async function diagnose(){
  if(!campaignId||needsReload||uncertainCreate||refreshFailed||data.truncated||!begin())return;
  const seq=++readSequence.current;setPreview(null);
  try{const value=await call<OptimizationDiagnosis>('optimization.diagnose',{brandId,campaignId,phase:'baseline'});if(alive.current&&seq===readSequence.current){setPreview(value);setFeedback('只读诊断完成，尚未保存计划')}}catch(e){if(alive.current&&seq===readSequence.current)setError(`诊断未完成，请核对固定测量与样本后重新诊断。${errorText(e)}`)}finally{finish()}
 }
 async function submitCreate(payload:OptimizationCreatePayload){
  if(needsReload||!begin())return;
  setRetryOriginal(false);
  let saved:OptimizationCreated;
  try{saved=await call<OptimizationCreated>('optimization.create',{...payload})}
  catch(e){if(alive.current){attempt.current=payload;setUncertainCreate(true);setError(`创建结果未确认，请先重新加载计划列表核对原请求，不能另建或直接重复提交。${errorText(e)}`)}finish();return}
  if(!alive.current){finish();return}
  // Retain the acknowledged full result before any list refresh can fail.
  record(saved);attempt.current=null;setUncertainCreate(false);setPreview(null);setName('');setFeedback(saved.alreadyExists?'已复用原请求的优化计划':'优化计划已保存');
  try{await fetchList()}catch(e){if(alive.current){setRefreshFailed(true);setError(`计划已保存，但列表刷新失败。请重新加载计划列表，无需重复提交。${errorText(e)}`)}}finally{finish()}
 }
 function create(){if(!preview||!name.trim()||name.trim().length>100||needsReload||uncertainCreate||refreshFailed||lock.current)return;const payload:OptimizationCreatePayload={brandId,name:name.trim(),campaignId:preview.campaign.id,diagnosisFingerprint:preview.fingerprint,requestId:attempt.current?.requestId??crypto.randomUUID()};attempt.current=payload;void submitCreate(payload)}
 async function loadPlan(id:string){
  if(uncertainCreate||(needsReload&&id!==selectedId)||!begin())return;
  if(!id){setSelectedId('');setPlan(null);finish();return}
  setSelectedId(id);if(id!==selectedId)setPlan(null);
  try{const saved=await call<OptimizationPlan>('optimization.get',{brandId,planId:id});if(alive.current){record(saved);setNeedsReload(false);setFeedback('已加载保存版本，请核对后手动操作')}}catch(e){if(alive.current){setNeedsReload(true);setError(`计划加载失败，请重新加载当前计划。${errorText(e)}`)}}finally{finish()}
 }
 async function mutate(action:'update'|'check',fields:Record<string,unknown>={}){
  if(!plan||needsReload||uncertainCreate||refreshFailed||!begin())return;
  try{const saved=await call<OptimizationPlan>(`optimization.${action}`,{brandId,planId:plan.id,expectedRevision:plan.revision,...fields});if(alive.current){record(saved);setFeedback(action==='update'?'行动状态已保存；检测结论未因此改变':'复测检查点已保存；请对照冻结指标核对')}}catch(e){if(alive.current){setNeedsReload(true);setError(`操作结果未确认，请重新加载当前计划核对；不会自动重放操作。${errorText(e)}`)}}finally{finish()}
 }
 async function exportPlan(){if(!plan||needsReload||uncertainCreate||!begin())return;try{const value=await call<{saved:true}|null>('file.exportOptimization',{brandId,planId:plan.id});if(alive.current)setFeedback(value?.saved?'优化计划 ZIP 已保存':'已取消导出，计划记录保留')}catch(e){if(alive.current)setError(`导出未完成，计划记录保留。${errorText(e)}`)}finally{finish()}}
 const pending=busy||listBusy,creationBlocked=pending||needsReload||uncertainCreate||refreshFailed;
 const mutationBlocked=pending||needsReload||uncertainCreate||refreshFailed;
 const first=plan?.checkpoints[0],latest=plan?.checkpoints[plan.checkpoints.length-1];
 return <div className="attribution-workbench optimization-workbench" style={{minWidth:0,overflowWrap:'anywhere'}}>
 <Panel title="优化闭环 · 本地诊断" description="从固定测量证据建立行动计划，人工处理后再明确采集复测检查点。">
 <p className="notice">仅使用本地冻结证据，不连接网站、不发布、不生成观测。模型调用 0 次，模型费用 0。人工标记已处理不等于问题已消失，也不证明 GEO 成效。</p>
 {!campaigns.length&&<p className="inline-note">请先在可见度建立固定测量计划。{onNavigate&&<button disabled={pending} onClick={()=>onNavigate('visibility')}>前往可见度</button>}</p>}
 {data.truncated&&<p role="alert">工作区快照不完整，请重新加载工作区后再诊断。</p>}
 <Field label="固定测量计划"><select disabled={creationBlocked} value={campaignId} onChange={e=>{if(lock.current||needsReload||uncertainCreate||refreshFailed)return;setCampaignId(e.target.value);setPreview(null);setFeedback('');setError('');readSequence.current++}}><option value="">请选择固定测量计划</option>{campaigns.map(c=><option key={c.id} value={c.id}>{c.name} · {evidenceLabel(c.evidenceMode??'unspecified')}</option>)}</select></Field>
 <div className="actions"><button disabled={creationBlocked||!campaignId||!!data.truncated} onClick={()=>void diagnose()}>只读诊断基线</button></div>
 {preview&&<section aria-label="未保存诊断预览"><Metrics diagnosis={preview} label="诊断冻结指标"/><h3>待处理问题 · {evidenceLabel(preview.evidenceMode)}</h3>{!preview.findings.length&&<p>本次规则未检测到问题；不代表真实平台效果已验证。</p>}<ul>{preview.findings.map(f=><li key={f.key} aria-label={`诊断问题：${f.title} · ${targetLabel(f.productId,preview.products.find(p=>p.id===f.productId))} · ${f.key}`}><strong>{f.title}</strong><p>冻结目标：{targetLabel(f.productId,preview.products.find(p=>p.id===f.productId))}</p><p>{f.detail}</p><p className="inline-note">证据：{f.evidenceIds.join('、')||'当前缺少对应资料'}</p></li>)}</ul><p className="inline-note">诊断指纹：{preview.fingerprint}</p></section>}
 <Field label="优化计划名称"><input maxLength={100} disabled={creationBlocked} value={name} onChange={e=>setName(e.target.value)}/></Field>
 <div className="actions"><button className="primary" disabled={creationBlocked||!preview||!name.trim()||name.trim().length>100} onClick={create}>保存优化计划</button>{retryOriginal&&<button disabled={pending||needsReload} onClick={()=>{const payload=attempt.current;if(payload&&retryOriginal)void submitCreate(payload)}}>重试原保存请求</button>}{retryOriginal&&<button disabled={pending||needsReload} onClick={()=>{if(!attempt.current||!retryOriginal||lock.current)return;setUncertainCreate(false);setRetryOriginal(false);setPreview(null);setError('');setFeedback('原请求编号保留，请重新诊断并核对名称后手动保存；迟到的原保存会由同编号冲突保护阻止重复。');readSequence.current++}}>保留原请求编号，重新诊断编辑</button>}</div>
 {attempt.current&&<p className="inline-note">原保存请求编号：{attempt.current?.requestId}。结果未确认时请先重新加载计划列表；只有成功确认未找到，才可显式重试原内容或保留原编号重新诊断编辑。</p>}
 </Panel>
 <Panel title="已保存优化计划" description="进入本页只读取列表；不会自动创建、更新或采集复测。">
 <div className="actions"><button disabled={pending} onClick={()=>void reloadList()}>重新加载计划列表</button></div>
 <Field label="已保存优化计划"><select value={selectedId} disabled={pending||uncertainCreate||needsReload} onChange={e=>void loadPlan(e.target.value)}><option value="">请选择优化计划</option>{plans.map(p=><option key={p.id} value={p.id}>{p.name} · {evidenceLabel(p.evidenceMode)} · v{p.revision} · 检查点 {p.checkpointCount} · 行动 {p.doneCount}/{p.actionCount}</option>)}</select></Field>
 {!listBusy&&!plans.length&&<p className="inline-note">尚无已保存优化计划。只读诊断后可明确保存。</p>}
 {selectedId&&<div className="actions"><button disabled={pending||uncertainCreate} onClick={()=>void loadPlan(selectedId)}>重新加载当前计划</button></div>}
 {plan&&first&&latest&&<section aria-label="优化计划明细"><h3>{plan.name} · {evidenceLabel(plan.evidenceMode)}</h3><p>版本 {plan.revision} · 检查点 {plan.checkpoints.length} / 8 · 行动 {plan.actions.length} / 1000</p><p className="inline-note">固定测量：{first.diagnosis.campaign.name} · 初始保存并不代表测量时间；两个阶段的已有观测均随检查点冻结。历史指标不跟随当前商品修改。</p>
 <div className="actions"><button disabled={mutationBlocked||plan.checkpoints.length>=8} onClick={()=>void mutate('check')}>采集复测检查点</button><button disabled={mutationBlocked} onClick={()=>void exportPlan()}>导出优化计划 ZIP</button></div>
 <Metrics diagnosis={first.diagnosis} label="初始冻结指标"/><Metrics diagnosis={latest.diagnosis} label="最新冻结指标"/>
 <p className="inline-note">行动状态是人工自报；“已处理，等待验证”须与最新检测分别核对。最新检查点未检测到也不证明因果关系或真实平台提升。导出含用户填写的行动备注。</p>
 <ol className="optimization-actions queue-list">{plan.actions.map(action=><ActionEditor key={`${plan.id}:${action.key}`} action={action} target={targetLabel(action.productId,actionTarget(plan,action))} currentTarget={targetLabel(action.productId,latest.diagnosis.products.find(p=>p.id===action.productId))} detected={latest.diagnosis.findings.some(f=>f.key===action.key)} disabled={mutationBlocked} onSave={(status,note)=>void mutate('update',{actionKey:action.key,status,note})} onNavigate={onNavigate}/>)}</ol>
 {!plan.actions.length&&<p>当前计划没有行动项。请按需人工采集复测，不能据此认定平台效果。</p>}
 <details><summary>冻结检查点与证据指纹</summary><ol>{plan.checkpoints.map(cp=><li key={cp.id}>{cp.capturedAt} · {evidenceLabel(cp.diagnosis.evidenceMode)} · {cp.diagnosis.fingerprint}</li>)}</ol></details>
 </section>}
 </Panel>
 {pending&&<p role="status">正在处理本地资料…</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
