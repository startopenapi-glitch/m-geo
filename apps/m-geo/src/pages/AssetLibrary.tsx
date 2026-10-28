import {useEffect,useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {Badge,Empty,Field,Panel,submit,time,useTask} from '../components/ui';
import type {AssetArchiveRequest,AssetCreateRequest,AssetKind,AssetReviewDecision,AssetReviewRequest,AssetReviewStatus,AssetReviseRequest,ContentAsset} from '../assetTypes';

const kindLabels:Record<AssetKind,string>={keyword:'关键词',title:'标题'};
const statusLabels:Record<AssetReviewStatus,string>={pending:'待审核',approved:'已通过',rejected:'已驳回'};
const actionLabels={created:'人工创建',revised:'人工修订',approved:'审核通过',rejected:'审核驳回',archived:'归档保留',restored:'恢复使用'} as const;
interface AssetForm {kind:AssetKind;text:string;source:string;productId:string;requestId:string}
interface Editing {id:string;expectedRevision:number}
const emptyForm=():AssetForm=>({kind:'keyword',text:'',source:'',productId:'',requestId:crypto.randomUUID()});
const errorMessage=(error:unknown)=>error instanceof Error?error.message:'操作未完成，请重试';

export default function AssetLibrary(props:PageProps){
 return <AssetLibrarySession key={props.brand.id} {...props}/>;
}

function AssetLibrarySession({brand,data}:PageProps){
 const [assets,setAssets]=useState<ContentAsset[]>([]),[selectedId,setSelectedId]=useState('');
 const [filter,setFilter]=useState<''|AssetKind>(''),[listBusy,setListBusy]=useState(false),[listError,setListError]=useState('');
 const [form,setForm]=useState<AssetForm>(emptyForm),[editing,setEditing]=useState<Editing|null>(null);
 const [reviewNote,setReviewNote]=useState(''),[archiveNote,setArchiveNote]=useState('');
 const request=useRef(0),saveTask=useTask(),reviewTask=useTask(),archiveTask=useTask();
 const products=data.products.filter(product=>product.brandId===brand.id);
 const selected=assets.find(asset=>asset.id===selectedId)??null;
 const busy=saveTask.busy||reviewTask.busy||archiveTask.busy;

 async function load(){
  const seq=++request.current;setListBusy(true);setListError('');
  try{
   const result=await call<ContentAsset[]>('asset.list',filter?{brandId:brand.id,kind:filter}:{brandId:brand.id});
   if(seq!==request.current)return;
   setAssets(result);setSelectedId(current=>result.some(asset=>asset.id===current)?current:'');
  }catch(error){if(seq===request.current)setListError(errorMessage(error))}
  finally{if(seq===request.current)setListBusy(false)}
 }
 useEffect(()=>{void load();return()=>{request.current++}},[filter]);

 function update<K extends keyof AssetForm>(key:K,value:AssetForm[K]){
  setForm(current=>({...current,[key]:value,requestId:crypto.randomUUID()}));
 }
 function beginEdit(asset:ContentAsset){
  setEditing({id:asset.id,expectedRevision:asset.revision});
  setForm({kind:asset.kind,text:asset.text,source:asset.source,productId:asset.productId??'',requestId:crypto.randomUUID()});
  setReviewNote('');
 }
 function cancelEdit(){setEditing(null);setForm(emptyForm())}
 function savedAsset(saved:ContentAsset){
  setAssets(current=>[saved,...current.filter(asset=>asset.id!==saved.id)]);
  setSelectedId(saved.id);setEditing(null);setForm(emptyForm());
 }
 async function save(){
  if(saveTask.busy||reviewTask.busy||!form.text.trim()||!form.source.trim())return;
  if(editing){
   const payload:AssetReviseRequest={brandId:brand.id,id:editing.id,expectedRevision:editing.expectedRevision,text:form.text.trim(),source:form.source.trim(),productId:form.productId||null};
   await saveTask.run(()=>call<ContentAsset>('asset.revise',{...payload}),savedAsset,'内容资产已修订并回到待审核');
  }else{
   const payload:AssetCreateRequest={brandId:brand.id,kind:form.kind,text:form.text.trim(),source:form.source.trim(),productId:form.productId||null,requestId:form.requestId};
   await saveTask.run(()=>call<ContentAsset>('asset.create',{...payload}),savedAsset,'内容资产已创建，状态为待审核');
  }
 }
 async function review(decision:AssetReviewDecision){
  if(!selected||selected.reviewStatus!=='pending'||busy)return;
  const note=reviewNote.trim();if(decision==='rejected'&&!note)return;
  const assetId=selected.id;
  const payload:AssetReviewRequest={brandId:brand.id,id:assetId,expectedRevision:selected.revision,decision,note};
  await reviewTask.run(()=>call<ContentAsset>('asset.review',{...payload}),saved=>{
   setAssets(current=>current.map(asset=>asset.id===saved.id?saved:asset));
   setReviewNote('');if(editing?.id===assetId)cancelEdit();
  },decision==='approved'?'内容资产已人工审核通过':'内容资产已人工驳回');
 }
 async function setArchived(){
  if(!selected||busy)return;
  const assetId=selected.id,archived=!selected.archivedAt;
  const payload:AssetArchiveRequest={brandId:brand.id,id:assetId,expectedRevision:selected.revision,archived,note:archived?archiveNote.trim():''};
  await archiveTask.run(()=>call<ContentAsset>('asset.setArchived',{...payload}),saved=>{
   setAssets(current=>current.map(asset=>asset.id===saved.id?saved:asset));
   setArchiveNote('');if(editing?.id===assetId)cancelEdit();
  },archived?'内容资产已归档，历史稿件快照仍保留':'内容资产已恢复，可继续修订或按当前审核状态使用');
 }

 return <div className="attribution-workbench">
  <div className="page-heading"><div><h1>内容资产</h1><p>{brand.name} · 人工维护可复用的关键词与标题，保留每次修订和审核历史。</p></div></div>
  <p className="notice">仅供人工复用，不是已审核商品事实。内容资产不会自动联网、AI 生成或发布，也不替代商品事实与稿件审核。</p>
  <Panel title="资产列表" description="按当前品牌隔离保存；类型筛选只影响当前列表。" action={<button disabled={listBusy||busy} onClick={()=>void load()}>重新读取</button>}>
   <Field label="资产类型筛选"><select disabled={busy} value={filter} onChange={event=>setFilter(event.target.value as ''|AssetKind)}><option value="">全部类型</option><option value="keyword">关键词</option><option value="title">标题</option></select></Field>
   {listBusy&&<p role="status">正在读取内容资产…</p>}
   {listError&&<p role="alert" className="feedback error">{listError}</p>}
   {!listBusy&&!listError&&!assets.length&&<Empty title="还没有内容资产">使用下方表单人工创建关键词或标题。</Empty>}
   {!!assets.length&&<div className="compact-list">{assets.map(asset=><button key={asset.id} disabled={busy} aria-pressed={selectedId===asset.id} onClick={()=>{setSelectedId(asset.id);setReviewNote('');setArchiveNote('')}}><span><strong>{asset.text}</strong><small>{kindLabels[asset.kind]} · 修订 {asset.revision} · {asset.source}</small></span><Badge tone={asset.archivedAt?'neutral':asset.reviewStatus==='approved'?'green':asset.reviewStatus==='pending'?'amber':'neutral'}>{asset.archivedAt?'已归档':statusLabels[asset.reviewStatus]}</Badge></button>)}</div>}
  </Panel>

  {selected&&<Panel title="资产详情与审核" description="审核只改变资产自身状态，不会确认商品事实或稿件。">
   <section className="cohort" aria-label="内容资产详情">
    <h3>{selected.text}</h3>
    <dl className="attribution-metrics">
     <div><dt>类型</dt><dd>{kindLabels[selected.kind]}</dd></div>
     <div><dt>审核状态</dt><dd>{statusLabels[selected.reviewStatus]}</dd></div>
     <div><dt>修订号</dt><dd>{selected.revision}</dd></div>
     <div><dt>关联商品</dt><dd>{products.find(product=>product.id===selected.productId)?.name??'未关联'}</dd></div>
     <div><dt>审核时间</dt><dd>{selected.reviewedAt?time(selected.reviewedAt):'尚未审核'}</dd></div>
     <div><dt>创建时间</dt><dd>{time(selected.createdAt)}</dd></div>
     <div><dt>更新时间</dt><dd>{time(selected.updatedAt)}</dd></div>
    </dl>
    <p>来源：{selected.source}</p>
    {selected.reviewNote&&<p>审核理由：{selected.reviewNote}</p>}
    {selected.archivedAt&&<p className="notice">已于 {time(selected.archivedAt)} 归档。不可修订、审核或用于新稿件；历史稿件仍按原审核快照回读和导出。{selected.archiveNote?` 归档说明：${selected.archiveNote}`:''}</p>}
    <div className="actions"><button disabled={busy||!!selected.archivedAt} onClick={()=>beginEdit(selected)}>编辑此资产</button></div>
    <h3>修订、审核与归档历史</h3>
    <ol className="queue-list">{[...selected.history].reverse().map(entry=><li key={entry.revision}><strong>{actionLabels[entry.action]} · 修订 {entry.revision} · {statusLabels[entry.reviewStatus]}</strong><p>{entry.text}</p><small>来源：{entry.source}{entry.reviewNote?` · 理由：${entry.reviewNote}`:''} · {time(entry.changedAt)}</small></li>)}</ol>
    <Field label="审核理由（驳回必填）" hint="批准可留空；驳回必须填写理由。审核失败会保留输入。"><textarea rows={3} maxLength={2000} disabled={busy||!!selected.archivedAt||selected.reviewStatus!=='pending'} value={reviewNote} onChange={event=>setReviewNote(event.target.value)}/></Field>
    <div className="actions">
     <button className="primary" disabled={busy||!!selected.archivedAt||selected.reviewStatus!=='pending'} onClick={()=>void review('approved')}>人工审核通过</button>
     <button className="danger" disabled={busy||!!selected.archivedAt||selected.reviewStatus!=='pending'||!reviewNote.trim()} onClick={()=>void review('rejected')}>人工驳回</button>
    </div>
    {reviewTask.feedback}
    {!selected.archivedAt&&<Field label="归档说明（可选）" hint="归档不是删除，不清除资产、审核历史或稿件快照。"><textarea rows={2} maxLength={2000} disabled={busy} value={archiveNote} onChange={event=>setArchiveNote(event.target.value)}/></Field>}
    <div className="actions"><button disabled={busy} onClick={()=>void setArchived()}>{selected.archivedAt?'恢复此资产':'归档此资产'}</button></div>
    {archiveTask.feedback}
   </section>
  </Panel>}

  <Panel title={editing?'修订内容资产':'人工创建内容资产'} description={editing?'保存后生成新修订并回到待审核，历史版本仍然保留。':'提交失败会保留输入；同一未修改请求重试会复用创建请求标识。'}>
   <form onSubmit={submit(()=>void save())}>
    <fieldset className="task-fields" disabled={busy}>
     <div className="form-grid">
      <Field label="资产类型"><select disabled={!!editing} value={form.kind} onChange={event=>update('kind',event.target.value as AssetKind)}><option value="keyword">关键词</option><option value="title">标题</option></select></Field>
      <Field label="关联商品（可选）"><select value={form.productId} onChange={event=>update('productId',event.target.value)}><option value="">不关联商品</option>{products.map(product=><option key={product.id} value={product.id}>{product.name} · {product.sku}</option>)}</select></Field>
      <Field label="关键词或标题文本"><input required maxLength={500} value={form.text} placeholder="例如：虚构保温杯通勤选购" onChange={event=>update('text',event.target.value)}/></Field>
      <Field label="人工记录来源"><input required maxLength={2000} value={form.source} placeholder="例如：虚构运营资料表 v1" onChange={event=>update('source',event.target.value)}/></Field>
     </div>
     <div className="actions"><button className="primary" disabled={busy||!form.text.trim()||!form.source.trim()}>{editing?'保存修订并回到待审核':'创建待审核资产'}</button>{editing&&<button type="button" disabled={busy} onClick={cancelEdit}>取消修订</button>}</div>
    </fieldset>
    {saveTask.feedback}
   </form>
  </Panel>
 </div>;
}
