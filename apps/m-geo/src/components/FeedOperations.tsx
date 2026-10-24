import {useLayoutEffect,useRef,useState} from 'react';
import {call} from '../api';
import type {FeedRequest} from '../feedTypes';
import {Field,Panel} from './ui';
type Operation='compare'|'simulate'|'receipt';
type Scenario='accepted'|'rejected'|'unknown';
interface Summary {added?:number;removed?:number;changed?:number;unchanged?:number;accepted?:number;rejected?:number;unknown?:number;missing?:number}
interface OperationResult {summary:Summary;rows?:{sku:string;change?:string;fields?:string[];status?:string;code?:string;message?:string}[];receipt?:unknown}
interface Preview {result:OperationResult;inputs:FeedRequest & {baseline?:string;scenario?:Scenario;content?:string};version:number}
const changeNames:Record<string,string>={added:'本地选择新增',removed:'从本地选择移除',changed:'有变化',unchanged:'无变化'};
const statusNames:Record<string,string>={accepted:'模拟接受',rejected:'模拟拒绝',unknown:'未知',missing:'缺失'};
const fieldNames:Record<string,string>={sku:'商品编码（SKU）',productRevision:'商品版本',draftId:'说明草稿',draftRevision:'草稿版本',localStatus:'本地就绪情况',validity:'资料有效期',id:'渠道商品标识',title:'商品标题',description:'商品说明',link:'购买链接',imageLink:'商品图片',price:'价格',currency:'币种',availability:'库存状态','catalog.brand':'制造品牌','catalog.kind':'规格类型','catalog.groupId':'规格组编号','catalog.groupTitle':'规格组名称','catalog.color':'颜色','catalog.size':'尺寸','catalog.material':'材质','catalog.pattern':'图案','catalog.gtin':'全球贸易商品代码（GTIN）','catalog.mpn':'制造商型号（MPN）','catalog.condition':'成色','catalog.titleOrigin':'标题来源','catalog.productClass':'商品类别'};
export default function FeedOperations({request,snapshotKey}:{request:FeedRequest|null;snapshotKey:string}) {
 const [baseline,setBaseline]=useState(''),[content,setContent]=useState(''),[scenario,setScenario]=useState<Scenario>('accepted');
 const [previews,setPreviews]=useState<Partial<Record<Operation,Preview>>>({}),[busy,setBusy]=useState(false),[feedback,setFeedback]=useState('');
 const generation=useRef(0),locked=useRef(false);
 const requestKey=JSON.stringify(request);
 useLayoutEffect(()=>{generation.current++;locked.current=false;setBusy(false);setPreviews({});setFeedback('');setBaseline('');setContent('');setScenario('accepted');return()=>{generation.current++}},[requestKey,snapshotKey]);
 function invalidate(){generation.current++;locked.current=false;setBusy(false);setPreviews({});setFeedback('')}
 function edit(kind:'baseline'|'receipt',value:string){invalidate();if(kind==='baseline')setBaseline(value);else setContent(value)}
 async function importText(kind:'baseline'|'receipt'){
  if(locked.current)return;const version=generation.current;locked.current=true;setBusy(true);setFeedback('');
  try{const file=await call<{name:string;content:string}|null>('file.openText',{kind:'evidence'});if(version!==generation.current)return;
   if(file===null){setFeedback('已取消导入，原内容已保留');return}
   if(typeof file.content!=='string'||new TextEncoder().encode(file.content).length>2*1024*1024)throw new Error('导入内容必须为不超过 2 MiB 的文本');
   edit(kind,file.content);setFeedback('文本已导入，请重新计算本地结果');
  }catch(error){if(version===generation.current)setFeedback(error instanceof Error?error.message:'导入失败')}
  finally{if(version===generation.current){locked.current=false;setBusy(false)}}
 }
 async function run(operation:Operation,exporting=false){
  if(!request||locked.current)return;
  const version=generation.current;
  const preview=previews[operation];if(exporting&&(!preview||preview.version!==version))return;
  const inputs=exporting?preview!.inputs:{...request,...(operation==='compare'?{baseline}:operation==='simulate'?{scenario}:{content})};
  locked.current=true;setBusy(true);setFeedback('');
  try{
   if(exporting){const saved=await call('file.exportFeedOperation',{...inputs,operation});if(version===generation.current)setFeedback(saved?'本地 ZIP 已保存':'已取消保存')}
   else{const action=operation==='receipt'?'feed.checkReceipt':`feed.${operation}`;const result=await call<OperationResult>(action,inputs);
    if(version===generation.current){setPreviews(previous=>{const next={...previous,[operation]:{result,inputs,version}};if(operation==='simulate'&&result.receipt)delete next.receipt;return next});if(operation==='simulate'&&result.receipt)setContent(JSON.stringify(result.receipt,null,2))}
   }
  }catch(error){if(version===generation.current)setFeedback(error instanceof Error?error.message:'本地计算失败')}
  finally{if(version===generation.current){locked.current=false;setBusy(false)}}
 }
 function result(operation:Operation){const preview=previews[operation];if(!preview||preview.version!==generation.current)return null;const value=preview.result,s=value.summary;
  return <div className="cohort">{operation==='compare'?<><p>本地新增 {s.added} · 本地移除 {s.removed} · 变化 {s.changed} · 无变化 {s.unchanged}</p><p>移除仅表示不在本次本地选择中，不代表平台删除。</p></>:<p>模拟接受 {s.accepted} · 模拟拒绝 {s.rejected} · 未知 {s.unknown} · 缺失 {s.missing}</p>}{value.rows?.map(row=><p key={row.sku}><strong>{row.sku}</strong> · {row.change?changeNames[row.change]:statusNames[row.status||'']} {row.fields?.map(field=>fieldNames[field]||field).join('、')}{row.code&&` · ${row.code}`}{row.message&&` · ${row.message}`}</p>)}</div>
 }
 return <Panel title="本地清单比较与模拟回执" description="比较保存的本地快照，模拟接受、拒绝或未知，并核对当前商品快照绑定。">
  <p>所有回执仅为本机模拟；商品尚未提交平台，不代表真实接受、收录或推荐。摘要校验用于发现内容变化，不是签名或平台认证。</p>
  {!request&&<p>请先选择商品和当前已审核的说明草稿。</p>}
  <Field label="基线清单 JSON" hint="导入之前保存的 feed-manifest.json；最多 2 MiB。"><textarea value={baseline} onChange={e=>edit('baseline',e.target.value)} rows={5}/></Field>
  <div className="actions"><button disabled={busy||!request} onClick={()=>void importText('baseline')}>导入基线清单</button><button disabled={busy||!request||!baseline.trim()} onClick={()=>void run('compare')}>比较当前本地选择</button><button disabled={busy||!request||!previews.compare} onClick={()=>void run('compare',true)}>导出比较 ZIP</button></div>
  {result('compare')}
  <Field label="模拟场景"><select value={scenario} onChange={e=>{invalidate();setScenario(e.target.value as Scenario)}}><option value="accepted">模拟全部接受</option><option value="rejected">模拟全部拒绝</option><option value="unknown">模拟全部未知</option></select></Field>
  <div className="actions"><button disabled={busy||!request} onClick={()=>void run('simulate')}>生成模拟回执</button><button disabled={busy||!request||!previews.simulate} onClick={()=>void run('simulate',true)}>导出模拟回执 ZIP</button></div>
  {result('simulate')}
  <Field label="模拟回执 JSON" hint="仅接受本应用的 simulated 回执；缺失商品保持缺失，不会推定成功。"><textarea value={content} onChange={e=>edit('receipt',e.target.value)} rows={7}/></Field>
  <div className="actions"><button disabled={busy||!request} onClick={()=>void importText('receipt')}>导入模拟回执</button><button disabled={busy||!request||!content.trim()} onClick={()=>void run('receipt')}>核对模拟回执</button><button disabled={busy||!request||!previews.receipt} onClick={()=>void run('receipt',true)}>导出回执核对 ZIP</button></div>
  {result('receipt')}
  {busy&&<p role="status">正在本地处理…</p>}{feedback&&<p role="status">{feedback}</p>}
 </Panel>
}
