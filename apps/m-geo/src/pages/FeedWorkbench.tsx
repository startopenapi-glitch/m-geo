import {useEffect,useRef,useState} from 'react';
import {call} from '../api';
import type {WorkspaceSnapshot} from '../types';
import type {FeedPackage,FeedRequest} from '../feedTypes';
import FeedOperations from '../components/FeedOperations';
import ChannelPackageInspector from '../components/ChannelPackageInspector';
import {Panel} from '../components/ui';
export default function FeedWorkbench({brandId,data,onBusyChange}:{brandId:string;data:WorkspaceSnapshot;onBusyChange?:(busy:boolean)=>void}){
 const snapshotKey=JSON.stringify(data);
 // Remount transient controls on a workspace change, including a return to an earlier brand.
 return <><ChannelPackageInspector key={brandId} brandId={brandId} onBusyChange={onBusyChange}/><FeedSession key={brandId+snapshotKey} brandId={brandId} data={data} snapshotKey={snapshotKey}/></>;
}
function FeedSession({brandId,data,snapshotKey}:{brandId:string;data:WorkspaceSnapshot;snapshotKey:string}){
 const [selection,setSelection]=useState<Record<string,string>>({});
 const [profile,setProfile]=useState<FeedRequest['profile']>('mgeo-catalog-v1');
 const [preview,setPreview]=useState<FeedPackage|null>(null),[busy,setBusy]=useState(false),[feedback,setFeedback]=useState('');
 const alive=useRef(true);const generation=useRef(0);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
 const products=data.products.filter(p=>p.brandId===brandId);
 const drafts=(productId:string)=>data.drafts.filter(d=>d.brandId===brandId&&d.productId===productId&&d.kind==='description'&&d.reviewedAt&&d.sourceRevision===products.find(p=>p.id===productId)?.revision);
 const items=products.filter(p=>Object.hasOwn(selection,p.id)).flatMap(p=>{const d=drafts(p.id).find(d=>d.id===selection[p.id]);return d?[{productId:p.id,expectedRevision:p.revision,draftId:d.id,expectedDraftRevision:d.revision}]:[]});
 const count=Object.keys(selection).length;
 const request:FeedRequest|null=count>0&&count<=200&&items.length===count&&!data.truncated?{brandId,profile,items}:null;
 const requestKey=JSON.stringify(request);const current=useRef(requestKey);current.current=requestKey;
 function invalidate(){generation.current++;setPreview(null);setFeedback('');setBusy(false)}
 async function run(exporting:boolean){if(!request)return;const key=requestKey,seq=++generation.current;setBusy(true);setFeedback('');try{if(exporting){const saved=await call<{saved:true}|null>('file.exportFeed',request);if(current.current===key&&generation.current===seq&&alive.current)setFeedback(saved?'本地文件已保存；尚未提交':'已取消保存');}else{const result=await call<FeedPackage>('feed.prepare',request);if(current.current===key&&generation.current===seq&&alive.current){setPreview(result);setFeedback('本地检查完成；平台资格仍待核实');}}}catch(error){if(current.current===key&&generation.current===seq&&alive.current)setFeedback(error instanceof Error?error.message:'检查未完成，请重试')}finally{if(generation.current===seq&&alive.current)setBusy(false)}}
 return <Panel title="商品 Feed 本地工作台" description="选择商品及当前已审核介绍，检查并保存本地草稿。平台状态始终为尚未提交。">
  <p className="inline-note">Google basic 仅支持明确单规格、新实物非服装商品，库存为有货或缺货。通用 M-GEO 目录支持完整变体资料。标识符、商家资格及地区品类要求仍待人工核实。</p>
  <label>草稿格式<select aria-label="草稿格式" value={profile} onChange={e=>{invalidate();setProfile(e.target.value as FeedRequest['profile'])}}><option value="mgeo-catalog-v1">M-GEO 通用商品目录</option><option value="google-merchant-basic-v1">Google Merchant basic XML 草稿</option></select></label>
  {data.truncated&&<p role="alert">资料列表不完整，请减少工作区资料或重新加载后再准备 Feed。</p>}
  <fieldset><legend>明确选择商品及已审核介绍 · 已选 {count} 件</legend>{products.map(p=><div className="cohort" key={p.id}><label className="check"><input type="checkbox" checked={Object.hasOwn(selection,p.id)} onChange={e=>{invalidate();setSelection(s=>{const next={...s};if(e.target.checked)next[p.id]='';else delete next[p.id];return next})}}/>{p.name} · {p.sku||'缺少 SKU'} · v{p.revision}</label>{Object.hasOwn(selection,p.id)&&<label>{p.name}的已审核介绍<select aria-label={`${p.name}的已审核介绍`} value={selection[p.id]} onChange={e=>{invalidate();setSelection(s=>({...s,[p.id]:e.target.value}))}}><option value="">请选择当前已审核介绍</option>{drafts(p.id).map(d=><option key={d.id} value={d.id}>{d.title} · v{d.revision} · {d.generationMode==='ai'?'AI 生成':'本地整理'}</option>)}</select>{!drafts(p.id).length&&<span>请先生成并审核当前资料版本的商品介绍。</span>}</label>}</div>)}</fieldset>
  <div className="actions"><button disabled={busy||!request} onClick={()=>void run(false)}>检查 Feed 草稿</button><button disabled={busy||!request} onClick={()=>void run(true)}>导出草稿与检查报告</button></div>
  {feedback&&<p role="status">{feedback}</p>}
  {preview&&<section aria-label="Feed 检查结果"><strong>已选 {preview.summary.selected} 件 · 本地就绪 {preview.summary.ready} 件 · 阻断 {preview.summary.blocked} 件</strong><p>{preview.summary.blocked?'存在阻断项：仅导出检查报告及清单，不包含可用商品 Feed 文件。':'本地字段检查通过，仍未核实平台资格；尚未提交。'}</p><p>仅包含选中商品，不能作为完整商家目录替换或删除授权。</p>{preview.rows.map(r=><div key={r.productId}><strong>{r.sku||'缺少 SKU'} · {r.localStatus==='ready'?'本地就绪':'待处理'}</strong><ul>{r.issues.map((i,n)=><li key={n}><span>{i.message}</span> <small>（{i.field}）</small></li>)}</ul></div>)}<ul>{preview.externalChecks.map((s,i)=><li key={i}>{s}</li>)}</ul>{preview.files.map(f=><details key={f.path}><summary>{f.path}</summary><pre className="preview">{f.content}</pre></details>)}</section>}
  <FeedOperations request={request} snapshotKey={snapshotKey}/>
 </Panel>;
}
