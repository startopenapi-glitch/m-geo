import {useState} from 'react';
import {call} from './api';
import {campaignMode,observationMode} from './evidence';
import type {Campaign,Observation,Platform,WorkspaceSnapshot} from './types';
import {Badge,Field,Panel,TaskForm,submit,useTask} from './components/ui';

export interface AiMonitorInput {
  brandId:string;
  campaignId:string;
  productId:string;
  questionId:string;
  platform:Platform;
  phase:'baseline'|'followup';
  repetition:number;
}
export interface AiMonitorPreparation extends AiMonitorInput {
  monitorVersion:1;
  preparationHash:string;
  surface:'api';
  evidenceMode:'recorded';
  sourceKind:'real_api';
  transport:'real_api';
  modelLabel:string;
  searchMode:string;
  configVersion:number;
  publicSnapshot:{brandName:string;productName:string;sku:string;facts:{label:string;value:string;source:string}[]};
}
export interface AiMonitorJob {
  id:string;
  brandId:string;
  kind:'monitor';
  status:'queued'|'running'|'succeeded'|'failed'|'cancelled'|'interrupted';
  message:string;
  resultText:string|null;
  responseModel?:string|null;
  providerRequestId?:string|null;
  citations?:string[];
  observationId?:string|null;
  resultIdentity?:string|null;
}
export function prepareAiMonitor(input:AiMonitorInput){
  return call<AiMonitorPreparation>('ai.monitor.prepare',input as unknown as Record<string,unknown>);
}
export function startAiMonitor(brandId:string,preparationHash:string){
  return call<AiMonitorJob>('ai.monitor.start',{brandId,preparationHash,consent:true});
}
export async function waitForAiMonitor(brandId:string,id:string,onUpdate?:(job:AiMonitorJob)=>void){
  for(let attempt=0;attempt<360;attempt++){
    const jobs=await call<AiMonitorJob[]>('ai.jobs',{brandId});
    const job=jobs.find(item=>item.id===id&&item.kind==='monitor');
    if(!job)throw new Error('真实 API 监测任务不存在或品牌不匹配');
    onUpdate?.(job);
    if(job.status==='succeeded')return job;
    if(['failed','cancelled','interrupted'].includes(job.status))throw new Error(job.message||'真实 API 监测未完成；未自动重试');
    await new Promise(resolve=>setTimeout(resolve,250));
  }
  throw new Error('监测任务仍在处理中，请稍后查看任务状态；不会自动重试');
}
export function observationOriginLabel(observation:Observation,campaigns:Campaign[]=[]){
  const source=observation as Observation&{sourceKind?:string;transport?:string};
  if(source.surface==='api'&&source.sourceKind==='real_api'&&source.transport==='real_api')return '真实 API';
  if(observationMode(observation,campaigns)==='simulated')return '模拟';
  return '手动导入';
}

export function AiMonitorPanel({brandId,data,refresh}:{brandId:string;data:WorkspaceSnapshot;refresh:()=>Promise<void>}){
  const campaigns=(data.campaigns||[]).filter(c=>campaignMode(c)==='recorded');
  const [campaignId,setCampaignId]=useState(''),[productId,setProductId]=useState(''),[questionId,setQuestionId]=useState('');
  const [platform,setPlatform]=useState<Platform>('doubao'),[phase,setPhase]=useState<'baseline'|'followup'>('baseline'),[repetition,setRepetition]=useState(1);
  const [preparation,setPreparation]=useState<AiMonitorPreparation|null>(null),[consent,setConsent]=useState(false),[job,setJob]=useState<AiMonitorJob|null>(null);
  const task=useTask();
  const campaign=campaigns.find(c=>c.id===campaignId);
  const products=Array.from(new Map((campaign?.questions||[]).map(q=>[q.productId,data.products.find(p=>p.id===q.productId)])).entries());
  const questions=(campaign?.questions||[]).filter(q=>q.productId===productId);
  const question=questions.find(q=>q.id===questionId);
  const resetPreparation=()=>{setPreparation(null);setConsent(false);setJob(null)};
  const input:AiMonitorInput={brandId,campaignId,productId,questionId,platform,phase,repetition};
  const prepare=()=>void task.run(()=>prepareAiMonitor(input),value=>{setPreparation(value);setConsent(false);setJob(null)},'已从本机固定问题与公开事实完成准备，尚未发送');
  const start=()=>{
    if(!preparation||!consent)return;
    void task.runPersisted(async()=>{
      const started=await startAiMonitor(brandId,preparation.preparationHash);setJob(started);
      return waitForAiMonitor(brandId,started.id,setJob);
    },refresh,result=>{setJob(result);setConsent(false)},'真实 API 回答已保存；提及为初判，事实准确性待人工复核');
  };
  return <Panel title="真实 API 知情问答测试" description="这是已提供商品公开事实的知情测试，只调用已保存的模型接口；不能作为自然发现或消费者平台推荐证据。不会读取或显示 API Key。" action={<Badge tone="green">真实 API</Badge>}>
    {!campaigns.length?<p className="inline-note">请先建立“人工实测记录”固定测试计划。模拟计划不能用于真实 API 监测。</p>:<TaskForm task={task} onSubmit={submit(prepare)}>
      <div className="form-grid">
        <Field label="真实监测固定计划"><select required value={campaignId} onChange={e=>{const next=campaigns.find(c=>c.id===e.target.value);setCampaignId(e.target.value);setProductId('');setQuestionId('');setRepetition(1);if(next)setPlatform(next.platforms[0]);resetPreparation()}}><option value="">选择人工实测记录计划</option>{campaigns.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></Field>
        <Field label="真实监测商品"><select required value={productId} disabled={!campaign} onChange={e=>{setProductId(e.target.value);setQuestionId('');resetPreparation()}}><option value="">选择固定商品</option>{products.map(([id,product])=><option key={id} value={id}>{product?.name||'历史商品'}</option>)}</select></Field>
        <Field label="真实监测固定问题"><select required value={questionId} disabled={!productId} onChange={e=>{setQuestionId(e.target.value);resetPreparation()}}><option value="">选择冻结问题</option>{questions.map(q=><option key={q.id} value={q.id}>{q.text}</option>)}</select></Field>
        <Field label="真实监测平台"><select value={platform} disabled={!campaign} onChange={e=>{setPlatform(e.target.value as Platform);resetPreparation()}}>{campaign?.platforms.map(id=><option key={id} value={id}>{id}</option>)}</select></Field>
        <Field label="真实监测阶段"><select value={phase} onChange={e=>{setPhase(e.target.value as 'baseline'|'followup');resetPreparation()}}><option value="baseline">优化前基线</option><option value="followup">后续复测</option></select></Field>
        <Field label="真实监测轮次"><input type="number" min={1} max={campaign?.repetitions||10} value={repetition} onChange={e=>{setRepetition(Number(e.target.value));resetPreparation()}}/></Field>
      </div>
      {question&&<div className="query-box"><Badge>冻结问题</Badge><p>{question.text}</p></div>}
      <button className="primary" disabled={task.busy||!campaign||!question}>准备公开资料（不发送）</button>
    </TaskForm>}
    {preparation&&<div className="query-box"><p><strong>{preparation.modelLabel}</strong> · 配置版本 {preparation.configVersion} · surface=api</p><p>仅包含 {preparation.publicSnapshot.facts.length} 条已确认公开事实；不会发送原始资料、内部备注或私密事实。</p><p>准备哈希：<code>{preparation.preparationHash}</code></p></div>}
    {job&&<div className="query-box" aria-live="polite"><Badge tone={job.status==='succeeded'?'green':'blue'}>真实 API · {job.status}</Badge><p>{job.message}</p>{job.responseModel&&<p>返回模型：{job.responseModel} · Provider request ID：{job.providerRequestId||'未提供'}</p>}{job.resultText&&<pre className="preview">{job.resultText}</pre>}{!!job.citations?.length&&<ul>{job.citations.map(url=><li key={url}>{url}</li>)}</ul>}</div>}
    {preparation&&<><label className="check"><input type="checkbox" checked={consent} disabled={task.busy} onChange={e=>setConsent(e.target.checked)}/>同意向已保存的模型服务发送上述公开资料，并了解本次真实 API 调用可能产生费用</label><div className="actions"><button type="button" className="primary" disabled={task.busy||!consent} onClick={start}>发送一次真实 API 监测</button></div></>}
  </Panel>;
}
