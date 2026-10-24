import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import type {WorkspaceSnapshot} from '../types';
import type {AttributionApplyResult,AttributionDatasetSummary,AttributionPreview,AttributionReport,AttributionRequest,AttributionTemplate} from '../attributionTypes';
import {Field,Panel,useTask} from '../components/ui';
import AttributionResults,{modeLabel} from '../components/AttributionResults';
import AttributionPackageInspector from '../components/AttributionPackageInspector';

export default function AttributionWorkbench({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 // Keep frozen history independent of the live product/dataset snapshot.
 return <>
  <AttributionPackageInspector key={brandId} brandId={brandId}/>
  <AttributionSession key={brandId+JSON.stringify(data)} brandId={brandId} data={data}/>
 </>;
}
function utcField(value:string){return value.replace(/Z$/,'')}
function utcValue(value:string){const whole=value.replace(/\.0{1,3}$/,'');return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?$/.test(whole)?`${whole.length===16?`${whole}:00`:whole}Z`:''}
function AttributionSession({brandId,data}:{brandId:string;data:WorkspaceSnapshot}){
 const [name,setName]=useState(''),[content,setContent]=useState(''),[preview,setPreview]=useState<AttributionPreview|null>(null);
 const [datasets,setDatasets]=useState<AttributionDatasetSummary[]>([]),[datasetId,setDatasetId]=useState('');
 const [from,setFrom]=useState(''),[to,setTo]=useState(''),[lookback,setLookback]=useState('7'),[report,setReport]=useState<AttributionReport|null>(null);
 const [readBusy,setReadBusy]=useState(false),[listBusy,setListBusy]=useState(false),[feedback,setFeedback]=useState(''),[error,setError]=useState(''),[listError,setListError]=useState('');
 const datasetList=useRef<AttributionDatasetSummary[]>([]);
 const task=useTask(),alive=useRef(true),epoch=useRef(0),listEpoch=useRef(0),readLock=useRef(false);
 const products=data.products.filter(p=>p.brandId===brandId&&p.sku.trim());
 const dataset=datasets.find(d=>d.id===datasetId);
 const fromUtc=utcValue(from),toUtc=utcValue(to),days=Number(lookback);
 const validRange=!!dataset&&!!fromUtc&&!!toUtc&&Date.parse(fromUtc)<Date.parse(toUtc)&&Date.parse(fromUtc)>=Date.parse(dataset.coverageStart)&&Date.parse(toUtc)<=Date.parse(dataset.coverageEnd)&&Number.isInteger(days)&&days>=1&&days<=30;
 const request:AttributionRequest|null=validRange?{brandId,datasetId,from:fromUtc,to:toUtc,lookbackDays:days}:null;
 const byteLength=new TextEncoder().encode(content).length;
 const canInspect=!!products.length&&!!name.trim()&&name.trim().length<=100&&!!content.trim()&&byteLength<=512*1024&&!data.truncated;
 async function reloadList(){
  const seq=++listEpoch.current;setListBusy(true);setListError('');
  try{const result=await call<{datasets:AttributionDatasetSummary[]}>('attribution.list',{brandId});if(alive.current&&seq===listEpoch.current){datasetList.current=result.datasets;setDatasets(result.datasets)}}
  catch(e){if(alive.current&&seq===listEpoch.current)setListError(e instanceof Error?e.message:'资料列表加载失败');throw e}
  finally{if(alive.current&&seq===listEpoch.current)setListBusy(false)}
 }
 useEffect(()=>{alive.current=true;void reloadList().catch(()=>{});return()=>{alive.current=false;epoch.current++;listEpoch.current++}},[]);
 function invalidate(){epoch.current++;readLock.current=false;setReadBusy(false);setFeedback('');setError('')}
 function dirtyImport(){invalidate();setPreview(null)}
 function dirtyReport(){invalidate();setReport(null)}
 async function read<T>(fn:()=>Promise<T>,done:(result:T)=>void){if(readLock.current||task.busy)return;const seq=++epoch.current;readLock.current=true;setReadBusy(true);setFeedback('');setError('');try{const value=await fn();if(alive.current&&seq===epoch.current)done(value)}catch(e){if(alive.current&&seq===epoch.current)setError(e instanceof Error?e.message:'操作未完成，请重试')}finally{if(alive.current&&seq===epoch.current){readLock.current=false;setReadBusy(false)}}}
 function selectDataset(id:string){dirtyReport();setDatasetId(id);const selected=datasetList.current.find(d=>d.id===id);setFrom(selected?utcField(selected.coverageStart):'');setTo(selected?utcField(selected.coverageEnd):'');setLookback('7')}
 function save(){if(!preview||!canInspect||task.busy||readLock.current)return;const expectedHash=preview.contentHash;void task.runPersisted(async()=>{const saved=await call<AttributionApplyResult>('attribution.apply',{brandId,name:name.trim(),content,expectedHash});if(alive.current){dirtyImport();setReport(null)}return saved},reloadList,saved=>{selectDataset(saved.datasetId)},saved=>saved.alreadyImported?'已复用现有数据集，未重复保存':'数据集已保存')}
 const busy=readBusy||task.busy;
 return <div className="attribution-workbench">
 <Panel title="经营归因 · 本地工作台" description="导入不可变数据集，计算可复核的会话商品归因，保存本地证据包。">
 <p className="notice">仅在本机处理。模板为虚构模拟数据；“记录数据”也是导入者声明，须与模拟结果分开解读。来源标记与引荐由导入者声明，未经平台核实；归因不证明因果销量增量。不会连接账号、埋点、发布或调用模型。</p>
 {!products.length&&<p className="inline-note">请先为当前品牌创建带 SKU 的商品。</p>}
 {data.truncated&&<p role="alert">当前商品快照不完整，请重新加载完整资料后再导入。</p>}
 <fieldset className="task-fields" disabled={task.busy}>
 <div className="actions"><button disabled={busy||!products.length||!!data.truncated} onClick={()=>void read(()=>call<AttributionTemplate>('attribution.template',{brandId}),result=>{dirtyImport();setContent(JSON.stringify(result.exchange,null,2));setFeedback('已填入模拟模板，尚未保存')})}>填入模拟模板</button><button disabled={busy||!products.length||!!data.truncated} onClick={()=>void read(()=>call<{path:string}|null>('file.exportAttributionTemplate',{brandId}),saved=>setFeedback(saved?'模拟模板 ZIP 已保存，尚未导入':'已取消模板导出'))}>导出模拟模板 ZIP</button><button disabled={busy} onClick={()=>void read(()=>call<{content:string;name:string}|null>('file.openText',{kind:'evidence'}),file=>{if(!file){setFeedback('已取消选择，保留当前内容');return}dirtyImport();setContent(file.content);setFeedback('已读取本地文件，尚未保存')})}>选择本地 JSON</button></div>
 <Field label="数据集名称" hint="本地名称，1–100 字符。保存后内容与商品快照不可修改。"><input maxLength={100} value={name} onChange={e=>{dirtyImport();setName(e.target.value)}}/></Field>
 <Field label="交换 JSON" hint="m-geo-attribution-v1；最大 512 KiB，原始访问、订单行、退款合计最多 1000 行。仅不透明会话和订单标识，禁止姓名、电话、邮箱、IP、原始网址或设备标识。金额使用整数分；时间为 UTC 整秒。"><textarea rows={9} value={content} onChange={e=>{dirtyImport();setContent(e.target.value)}}/></Field>
 {byteLength>512*1024&&<p role="alert">文件超过 512 KiB，请缩小数据后重试。</p>}
 <div className="actions"><button disabled={busy||!canInspect} onClick={()=>{setPreview(null);void read(()=>call<AttributionPreview>('attribution.preview',{brandId,name:name.trim(),content}),setPreview)}}>检查导入预览</button><button className="primary" disabled={busy||!preview||!canInspect} onClick={save}>保存不可变数据集</button></div>
 </fieldset>
 {preview&&<section aria-label="导入预览" className="cohort"><h3>导入预览 · {modeLabel(preview.evidenceMode)}</h3><p>资料来源：{preview.sourceLabel} · 币种：{preview.currency}</p><p>覆盖区间（UTC）：[{preview.coverageStart}, {preview.coverageEnd})</p><p>访问 {preview.counts.visits} · 订单行 {preview.counts.orders} · 退款 {preview.counts.refunds} · 已去重 {preview.counts.duplicates}</p><p>{preview.alreadyImported?'相同内容已导入，将复用现有数据集':'预览通过，等待明确保存'}</p><details><summary>冻结商品快照与内容指纹</summary><p className="inline-note">{preview.contentHash}</p><ul>{preview.products.map(p=><li key={p.id}>{p.name} · {p.sku} · v{p.revision}</li>)}</ul></details></section>}
 {task.feedback}
 </Panel>
 <Panel title="已保存数据集与归因" description="每次只分析一个品牌、一个数据集及币种。时间字段均为 UTC，结束时间不包含在内。">
 <fieldset className="task-fields" disabled={task.busy}>
 <div className="actions"><button disabled={busy||listBusy} onClick={()=>void reloadList().catch(()=>{})}>重新加载数据集</button></div>
 {listBusy&&<p role="status">正在加载数据集…</p>}{listError&&<p role="alert">{listError}</p>}
 <Field label="已保存数据集"><select value={datasetId} onChange={e=>selectDataset(e.target.value)}><option value="">请选择不可变数据集</option>{datasets.map(d=><option key={d.id} value={d.id}>{d.name} · {modeLabel(d.evidenceMode)} · {d.currency}</option>)}</select></Field>
 {!listBusy&&!listError&&!datasets.length&&<p className="inline-note">尚无数据集，请先检查并明确保存。</p>}
 {dataset&&<p className="inline-note">{modeLabel(dataset.evidenceMode)} · 资料来源：{dataset.sourceLabel} · {dataset.currency} · 覆盖 [{dataset.coverageStart}, {dataset.coverageEnd}) · 访问 {dataset.counts.visits}／订单行 {dataset.counts.orders}／退款 {dataset.counts.refunds}</p>}
 <div className="form-grid"><Field label="开始时间（UTC，包含）"><input type="datetime-local" step="1" value={from} onChange={e=>{dirtyReport();setFrom(e.target.value)}}/></Field><Field label="结束时间（UTC，不含）"><input type="datetime-local" step="1" value={to} onChange={e=>{dirtyReport();setTo(e.target.value)}}/></Field><Field label="回溯天数" hint="1–30 天；默认 7 天仅为演练参数，尚未确认真实业务窗口。"><input type="number" min="1" max="30" step="1" value={lookback} onChange={e=>{dirtyReport();setLookback(e.target.value)}}/></Field></div>
 {dataset&&!validRange&&<p className="inline-note">请选择覆盖范围内的有效区间及 1–30 整数回溯天数。</p>}
 <div className="actions"><button className="primary" disabled={busy||!request} onClick={()=>{if(!request)return;setReport(null);void read(()=>call<AttributionReport>('attribution.report',{...request}),setReport)}}>计算归因</button><button disabled={busy||!report||!request} onClick={()=>{if(!request||!report)return;void read(()=>call<{path:string}|null>('file.exportAttribution',{...request}),saved=>setFeedback(saved?'归因证据 ZIP 已保存':'已取消导出'))}}>导出归因证据 ZIP</button></div>
 </fieldset>
 {report&&dataset&&<AttributionResults report={report} sourceLabel={dataset.sourceLabel}/>}
 </Panel>
 {readBusy&&<p role="status">正在本地处理…</p>}{error&&<p role="alert">{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 </div>;
}
