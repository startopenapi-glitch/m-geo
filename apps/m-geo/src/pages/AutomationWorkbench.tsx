import {useEffect,useMemo,useRef,useState} from 'react';
import {call} from '../api';
import {Field,Panel} from '../components/ui';
import type {EvidenceMode,Platform,WorkspaceSnapshot} from '../types';
import {AUTOMATION_NODE_KINDS,MIN_TREND_SAMPLES,compareTrendVersions,deriveTrendPoints,evidenceModeLabel,formatRate,makeAutomationCreatePayload,taskIsDue,type AutomationCreated,type AutomationNodeKind,type AutomationRunDue,type AutomationStatus,type AutomationTask,type TrendPoint} from '../automation';

const nodeLabels:Record<AutomationNodeKind,string>={fact_expiry_scan:'事实到期扫描',publication_readback_due:'发布回读到期提醒（仅标记）',visibility_retest_reminder:'可见度复测提醒'};
const statusLabels:Record<AutomationStatus,string>={active:'等待到期巡检',paused:'已暂停',cancelled:'已取消'};
const platformLabels:Record<Platform,string>={doubao:'豆包',yuanbao:'元宝',qianwen:'通义千问',baidu:'百度'};
const errorText=(error:unknown)=>error instanceof Error?error.message:'本地操作未完成，请重新加载核对';
const percent=(value:number|null)=>formatRate(value);

function TrendChart({points,metric}:{points:TrendPoint[];metric:'mentionRate'|'citationRate'|'accuracyReviewed'}){
 const dates=[...new Set(points.map(point=>point.date))].sort();
 const series=useMemo(()=>{
  const groups=new Map<string,TrendPoint[]>();
  for(const point of points){const key=JSON.stringify([point.evidenceMode,point.modelLabel,point.platform,point.campaignId]);groups.set(key,[...(groups.get(key)??[]),point])}
  return [...groups.values()].map(values=>values.sort((a,b)=>a.date.localeCompare(b.date)));
 },[points]);
 const colors=['#2563eb','#059669','#d97706','#dc2626','#7c3aed','#0891b2','#4f46e5','#be185d'];
 return <div aria-label="趋势图" role="img" style={{width:'100%',overflowX:'auto'}}>
  <svg viewBox="0 0 760 250" style={{width:'100%',minWidth:560,height:'auto'}} aria-hidden="true">
   <rect x="58" y="15" width="680" height="180" fill="#fafafa" stroke="#ddd"/>
   {[0,0.25,0.5,0.75,1].map(value=><g key={value}><line x1="58" x2="738" y1={195-value*180} y2={195-value*180} stroke="#e5e7eb"/><text x="50" y={199-value*180} textAnchor="end" fontSize="11">{value*100}%</text></g>)}
   {dates.map((date,index)=><text key={date} x={dates.length===1?398:58+index*680/(dates.length-1)} y="218" textAnchor="middle" fontSize="11">{date.slice(5)}</text>)}
   {series.map((values,seriesIndex)=>{
    const usable=values.filter(point=>point[metric]!==null);
    const coordinates=usable.map(point=>{const index=dates.indexOf(point.date);return {x:dates.length===1?398:58+index*680/(dates.length-1),y:195-(point[metric]??0)*180,point}});
    return <g key={values[0].key} color={colors[seriesIndex%colors.length]}>
     {coordinates.length>1&&<polyline points={coordinates.map(item=>`${item.x},${item.y}`).join(' ')} fill="none" stroke="currentColor" strokeWidth="2"/>}
     {coordinates.map(item=><circle key={item.point.key} cx={item.x} cy={item.y} r="4" fill="currentColor"><title>{item.point.date} · {evidenceModeLabel(item.point.evidenceMode)} · {item.point.modelLabel} · {platformLabels[item.point.platform]} · {item.point.campaignName}：{percent(item.point[metric])}，样本 {item.point.sampleCount}</title></circle>)}
    </g>})}
   <text x="398" y="242" textAnchor="middle" fontSize="12">日期；悬停数据点可查看样本性质、条件与样本数</text>
  </svg>
 </div>;
}

function SummaryNumbers({points}:{points:TrendPoint[]}){
 const summary=useMemo(()=>compareTrendVersions(points,points).baseline,[points]);
 return <div className="cohort"><p>样本 {summary.sampleCount} · 提及 {percent(summary.mentionRate)} · 引用 {percent(summary.citationRate)} · 已复核准确率 {percent(summary.accuracyReviewed)}（复核 {summary.accuracyReviewedCount} 条）</p>{summary.sampleInsufficient&&<p role="note">样本不足 {MIN_TREND_SAMPLES} 条，仅作描述，不作提升判断。</p>}</div>;
}

function TrendWorkbench({data}:{data:WorkspaceSnapshot}){
 const [mode,setMode]=useState<EvidenceMode|'all'>('all'),[model,setModel]=useState('all'),[platform,setPlatform]=useState<Platform|'all'>('all');
 const [from,setFrom]=useState(''),[to,setTo]=useState(''),[metric,setMetric]=useState<'mentionRate'|'citationRate'|'accuracyReviewed'>('mentionRate');
 const [baseline,setBaseline]=useState(''),[comparison,setComparison]=useState('');
 const points=useMemo(()=>deriveTrendPoints(data,{evidenceMode:mode,modelLabel:model,platform,from:from||undefined,to:to||undefined}),[data,mode,model,platform,from,to]);
 const models=[...new Set(data.observations.map(item=>item.modelLabel))].sort();
 const versions=useMemo(()=>[...new Map(points.map(point=>[point.campaignId??'__unassigned__',{id:point.campaignId,name:point.campaignName,mode:point.evidenceMode}])).values()],[points]);
 const selectedPoints=(id:string)=>points.filter(point=>(id==='__unassigned__'?null:id)===point.campaignId);
 const compared=baseline&&comparison&&baseline!==comparison?compareTrendVersions(selectedPoints(baseline),selectedPoints(comparison)):null;
 return <Panel title="证据趋势与版本比较" description="由当前工作区已有观测与固定计划本地派生；不生成新样本，不混合模拟与真实记录。">
  <p className="notice">提及率、引用率以全部样本为分母；已复核准确率只以“正确 / 不准确”样本为分母。每格保留样本性质、模型、平台、计划和日期。少于 {MIN_TREND_SAMPLES} 条时明确提示样本不足。</p>
  {data.truncated&&<p role="alert">工作区快照不完整，以下趋势不完整；请重新加载完整资料后再作判断。</p>}
  <div className="form-grid">
   <Field label="样本性质"><select value={mode} onChange={event=>setMode(event.target.value as EvidenceMode|'all')}><option value="all">全部（分格展示，不合并）</option><option value="recorded">人工实测记录</option><option value="simulated">模拟数据</option><option value="unspecified">历史未分类</option></select></Field>
   <Field label="模型"><select value={model} onChange={event=>setModel(event.target.value)}><option value="all">全部模型（分格展示）</option>{models.map(value=><option key={value}>{value}</option>)}</select></Field>
   <Field label="平台"><select value={platform} onChange={event=>setPlatform(event.target.value as Platform|'all')}><option value="all">全部平台（分格展示）</option>{Object.entries(platformLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
   <Field label="趋势指标"><select value={metric} onChange={event=>setMetric(event.target.value as typeof metric)}><option value="mentionRate">提及率</option><option value="citationRate">引用率</option><option value="accuracyReviewed">已复核准确率</option></select></Field>
   <Field label="开始日期"><input type="date" value={from} onChange={event=>setFrom(event.target.value)}/></Field>
   <Field label="结束日期"><input type="date" value={to} onChange={event=>setTo(event.target.value)}/></Field>
  </div>
  {!points.length&&<p className="inline-note">当前筛选下没有可用观测。请检查样本性质、固定计划关联与测量时间。</p>}
  {!!points.length&&<><TrendChart points={points} metric={metric}/><SummaryNumbers points={points}/>
   <div style={{overflowX:'auto'}}><table><thead><tr><th>日期</th><th>样本性质</th><th>模型 / 平台</th><th>固定计划</th><th>样本数</th><th>提及率</th><th>引用率</th><th>已复核准确率</th><th>判断</th></tr></thead><tbody>{points.map(point=><tr key={point.key}><td>{point.date}</td><td>{evidenceModeLabel(point.evidenceMode)}</td><td>{point.modelLabel} / {platformLabels[point.platform]}</td><td>{point.campaignName}</td><td>{point.sampleCount}</td><td>{percent(point.mentionRate)}</td><td>{percent(point.citationRate)}</td><td>{percent(point.accuracyReviewed)}（{point.accuracyReviewedCount}）</td><td>{point.sampleInsufficient?'样本不足':'描述性样本'}</td></tr>)}</tbody></table></div>
   <section className="cohort" aria-label="已有版本比较"><h3>已有固定计划版本比较</h3><p>选择两个已有计划，比较其当前筛选范围内的汇总观测。比较不代表两个版本之外的条件一致。</p>
    <div className="form-grid"><Field label="基准版本"><select value={baseline} onChange={event=>setBaseline(event.target.value)}><option value="">请选择</option>{versions.map(version=><option key={version.id??'__unassigned__'} value={version.id??'__unassigned__'}>{version.name} · {evidenceModeLabel(version.mode)}</option>)}</select></Field><Field label="对照版本"><select value={comparison} onChange={event=>setComparison(event.target.value)}><option value="">请选择</option>{versions.map(version=><option key={version.id??'__unassigned__'} value={version.id??'__unassigned__'}>{version.name} · {evidenceModeLabel(version.mode)}</option>)}</select></Field></div>
    {compared&&<><p>基准：样本 {compared.baseline.sampleCount}，提及 {percent(compared.baseline.mentionRate)}，引用 {percent(compared.baseline.citationRate)}，准确率 {percent(compared.baseline.accuracyReviewed)}。</p><p>对照：样本 {compared.comparison.sampleCount}，提及 {percent(compared.comparison.mentionRate)}，引用 {percent(compared.comparison.citationRate)}，准确率 {percent(compared.comparison.accuracyReviewed)}。</p><p>描述差值（对照 − 基准）：提及 {compared.mentionRateDelta===null?'—':`${(compared.mentionRateDelta*100).toFixed(1)} 个百分点`}；引用 {compared.citationRateDelta===null?'—':`${(compared.citationRateDelta*100).toFixed(1)} 个百分点`}；准确率 {compared.accuracyReviewedDelta===null?'—':`${(compared.accuracyReviewedDelta*100).toFixed(1)} 个百分点`}。</p><p className="inline-note">{compared.note}</p></>}
   </section>
  </>}
 </Panel>;
}

export default function AutomationWorkbench({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 return <AutomationSession key={brandId} brandId={brandId} data={data}/>;
}
function AutomationSession({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 const [tasks,setTasks]=useState<AutomationTask[]>([]),[selectedId,setSelectedId]=useState(''),[name,setName]=useState('每日只读巡检'),[intervalMinutes,setIntervalMinutes]=useState(1440);
 const [kinds,setKinds]=useState<AutomationNodeKind[]>([...AUTOMATION_NODE_KINDS]);
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const alive=useRef(true),attempt=useRef<ReturnType<typeof makeAutomationCreatePayload>|null>(null);
 const selected=tasks.find(task=>task.id===selectedId)??null;
 const brandData=useMemo(()=>({...data,observations:data.observations.filter(item=>item.brandId===brandId),campaigns:(data.campaigns??[]).filter(item=>item.brandId===brandId)}),[data,brandId]);
 async function reload(){setBusy(true);setError('');try{const result=await call<{tasks:AutomationTask[]}>('automation.list',{brandId});if(alive.current){setTasks(result.tasks);setFeedback('任务列表已重新加载；没有自动运行任务。')}}catch(reason){if(alive.current)setError(`任务列表读取失败。${errorText(reason)}`)}finally{if(alive.current)setBusy(false)}}
 useEffect(()=>{alive.current=true;void call<{tasks:AutomationTask[]}>('automation.list',{brandId}).then(result=>{if(alive.current)setTasks(result.tasks)}).catch(reason=>{if(alive.current)setError(`任务列表读取失败。${errorText(reason)}`)});return()=>{alive.current=false}},[brandId]);
 useEffect(()=>{
  let running=false;
  const check=async()=>{
   if(running||document.hidden)return;
   running=true;
   try{const result=await call<AutomationRunDue>('automation.runDue',{brandId});if(alive.current&&result.reports.length){setTasks(current=>{const changed=new Map(result.tasks.map(task=>[task.id,task]));return current.map(task=>changed.get(task.id)??task)});setFeedback(`应用内只读巡检已完成 ${result.reports.length} 项；网络请求 0，模型调用 0，发布写入 0。`)}}
   catch(reason){if(alive.current)setError(`自动到期检查未完成，不会自动重放或补跑。${errorText(reason)}`)}
   finally{running=false}
  };
  const timer=window.setInterval(()=>void check(),60_000);
  return()=>window.clearInterval(timer);
 },[brandId]);
 function record(task:AutomationTask){setTasks(old=>old.some(item=>item.id===task.id)?old.map(item=>item.id===task.id?task:item):[task,...old]);setSelectedId(task.id)}
 async function create(){
  if(busy||!kinds.length)return;setBusy(true);setError('');setFeedback('');
  try{const payload=attempt.current??makeAutomationCreatePayload(brandId,name,intervalMinutes,kinds,crypto.randomUUID());attempt.current=payload;const saved=await call<AutomationCreated>('automation.create',{...payload});if(!alive.current)return;record(saved);attempt.current=null;setFeedback(saved.alreadyExists?'已复用同一请求的巡检任务，没有重复创建。':'只读巡检已创建；仅到期且显式调用 runDue 时才运行。')}
  catch(reason){if(alive.current)setError(`创建结果未确认；重试会保留同一请求编号，请先重新加载列表核对。${errorText(reason)}`)}finally{if(alive.current)setBusy(false)}
 }
 async function mutate(action:'pause'|'resume'|'cancel',task:AutomationTask){
  if(busy)return;setBusy(true);setError('');setFeedback('');
  try{const saved=await call<AutomationTask>(`automation.${action}`,{brandId,taskId:task.id,expectedRevision:task.revision});if(alive.current){record(saved);setFeedback(action==='pause'?'任务已暂停，不会运行。':action==='resume'?'任务已恢复；过期的下一次时间已顺延，不补跑。':'任务已取消，历史报告保留。')}}catch(reason){if(alive.current)setError(`操作结果未确认，请重新加载列表核对，不会自动重放。${errorText(reason)}`)}finally{if(alive.current)setBusy(false)}
 }
 async function runDue(task:AutomationTask){
  if(busy)return;setBusy(true);setError('');setFeedback('');
  try{const result=await call<AutomationRunDue>('automation.runDue',{brandId,taskId:task.id});if(!alive.current)return;for(const saved of result.tasks)record(saved);setFeedback(result.reports.length?`已生成 ${result.reports.length} 份只读报告；网络请求 0，模型调用 0，发布写入 0。`:'任务当前未到期或不可运行，没有补跑。')}
  catch(reason){if(alive.current)setError(`到期巡检结果未确认，请重新加载列表核对。${errorText(reason)}`)}finally{if(alive.current)setBusy(false)}
 }
 const latest=selected?.reports.at(-1);
 return <div className="attribution-workbench automation-workbench" style={{minWidth:0,overflowWrap:'anywhere'}}>
  <Panel title="只读定时巡检" description="持久化显式 DAG；本页打开且可见时每 60 秒检查一次到期任务。离开本页或重启后不补跑，也不在系统后台唤醒。">
   <p className="notice">白名单仅含事实到期、发布回读到期标记、可见度复测提醒。禁止发布、付费 AI、Shell、任意 SQL 与 URL 抓取。真实 URL 回读仍须在发布记录中逐条主动执行。</p>
   <div className="form-grid"><Field label="巡检名称"><input maxLength={100} disabled={busy} value={name} onChange={event=>setName(event.target.value)}/></Field><Field label="间隔（分钟）" hint="至少15分钟；错过场次不补跑。"><input type="number" min={15} max={525600} step={1} disabled={busy} value={intervalMinutes} onChange={event=>setIntervalMinutes(Number(event.target.value))}/></Field></div>
   <fieldset disabled={busy}><legend>只读节点（按勾选顺序串联依赖）</legend>{AUTOMATION_NODE_KINDS.map(kind=><label key={kind} style={{display:'block'}}><input type="checkbox" checked={kinds.includes(kind)} onChange={event=>setKinds(old=>event.target.checked?[...old,kind]:old.filter(item=>item!==kind))}/> {nodeLabels[kind]}</label>)}</fieldset>
   <div className="actions"><button className="primary" disabled={busy||!name.trim()||!kinds.length||intervalMinutes<15} onClick={()=>void create()}>创建只读巡检</button><button disabled={busy} onClick={()=>void reload()}>重新加载任务列表</button></div>
   {attempt.current&&<p className="inline-note">原创建请求编号：{attempt.current.requestId}。请先重新加载核对；再次点击创建仍使用此编号。</p>}
  </Panel>
  <Panel title="巡检任务与报告" description="手动运行所选到期任务；暂停、恢复与取消只改变任务自身状态。">
   {!tasks.length&&<p className="inline-note">尚无巡检任务。创建不会立即运行。</p>}
   <Field label="已保存巡检"><select disabled={busy} value={selectedId} onChange={event=>setSelectedId(event.target.value)}><option value="">请选择任务</option>{tasks.map(task=><option key={task.id} value={task.id}>{task.name} · {statusLabels[task.status]} · 每{task.intervalMinutes}分钟 · 已运行{task.totalRuns}次</option>)}</select></Field>
   {selected&&<section className="cohort" aria-label="巡检任务明细"><h3>{selected.name} · {statusLabels[selected.status]}</h3><p>下一次：{selected.nextRunAt} · 最近运行：{selected.lastRunAt??'尚未运行'} · 版本 {selected.revision}</p><p>累计运行 {selected.totalRuns} 次；跳过历史场次 {selected.missedRuns} 次（只计数，不补跑）。保留最近 {selected.reports.length} 份报告。</p>
    <details><summary>显式 DAG 节点</summary><ol>{selected.nodes.map(node=><li key={node.id}>{node.id} · {nodeLabels[node.kind]} · 依赖：{node.dependsOn.join('、')||'无'}</li>)}</ol></details>
    <div className="actions"><button className="primary" disabled={busy||!taskIsDue(selected)} onClick={()=>void runDue(selected)}>手动运行到期任务</button>{selected.status==='active'&&<button disabled={busy} onClick={()=>void mutate('pause',selected)}>暂停</button>}{selected.status==='paused'&&<button disabled={busy} onClick={()=>void mutate('resume',selected)}>恢复</button>}{selected.status!=='cancelled'&&<button disabled={busy} onClick={()=>void mutate('cancel',selected)}>取消任务</button>}</div>
    {!taskIsDue(selected)&&selected.status==='active'&&<p className="inline-note">尚未到期；按钮不会强制提前运行。</p>}
    {latest&&<section aria-label="最新只读巡检报告"><h4>最新报告 · {latest.ranAt}</h4><p>模型调用 {latest.modelCalls} · 模型费用 {latest.modelFeeMinor} · 网络请求 {latest.networkRequests} · 发布写入 {latest.publicationWrites}</p>{latest.nodes.map(node=><div key={node.nodeId}><h4>{nodeLabels[node.kind]}</h4>{node.omitted>0&&<p>另有 {node.omitted} 条提醒未写入本报告，请缩小范围后人工核对。</p>}{!node.reminders.length&&<p>本次没有到期提醒；不代表效果已验证。</p>}<ul>{node.reminders.map((reminder,index)=><li key={`${reminder.kind}-${index}`}><strong>{reminder.title}</strong><p>{reminder.detail}</p><details><summary>本地引用</summary><pre>{JSON.stringify(reminder.refs,null,2)}</pre></details></li>)}</ul></div>)}</section>}
   </section>}
  </Panel>
  <TrendWorkbench data={brandData}/>
  {busy&&<p role="status">正在处理本地任务…</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
