import {useEffect,useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {Badge,Empty,Field,Panel,submit,time,useTask} from '../components/ui';
import type {KnowledgeDocument,KnowledgeDocumentSummary,KnowledgeFormat,KnowledgeImportRequest,KnowledgeReviewDecision,KnowledgeReviewRequest,KnowledgeReviewStatus,KnowledgeReviseRequest,KnowledgeRisk} from '../knowledgeTypes';

const reviewLabels:Record<KnowledgeReviewStatus,string>={pending:'待审核',approved:'已通过',rejected:'已驳回'};
const riskLabels:Record<KnowledgeRisk,string>={low:'低风险',medium:'中风险',high:'高风险'};
const formatLabels:Record<KnowledgeFormat,string>={markdown:'Markdown',text:'纯文本'};
interface KnowledgeForm {
 productId:string;
 title:string;
 source:string;
 format:KnowledgeFormat;
 effectiveAt:string;
 risk:KnowledgeRisk;
 content:string;
}
const emptyForm=():KnowledgeForm=>({productId:'',title:'',source:'',format:'markdown',effectiveAt:'',risk:'low',content:''});
const errorMessage=(error:unknown)=>error instanceof Error?error.message:'读取未完成，请重试';
function summarize(document:KnowledgeDocument):KnowledgeDocumentSummary{
 const {content:_content,chunks:_chunks,reviews:_reviews,...summary}=document;
 return summary;
}

export default function KnowledgeWorkbench(props:PageProps){
 return <KnowledgeSession key={props.brand.id} {...props}/>;
}

function KnowledgeSession({brand,data}:PageProps){
 const [documents,setDocuments]=useState<KnowledgeDocumentSummary[]>([]);
 const [selectedId,setSelectedId]=useState('');
 const [detail,setDetail]=useState<KnowledgeDocument|null>(null);
 const [listBusy,setListBusy]=useState(false),[listError,setListError]=useState('');
 const [detailBusy,setDetailBusy]=useState(false),[detailError,setDetailError]=useState('');
 const [reviewNote,setReviewNote]=useState('');
 const [editing,setEditing]=useState<{id:string;expectedRevision:number;requestId:string}|null>(null);
 const [form,setForm]=useState<KnowledgeForm>(emptyForm);
 const listRequest=useRef(0),detailRequest=useRef(0),selectedRef=useRef(''),createTask=useTask(),reviewTask=useTask();
 selectedRef.current=selectedId;
 const products=data.products.filter(product=>product.brandId===brand.id);
 const selected=documents.find(document=>document.id===selectedId)??null;
 const selectedDetail=detail?.id===selectedId?detail:null;
 const canCreate=!!form.title.trim()&&!!form.source.trim()&&!!form.content.trim();

 async function load(){
  const seq=++listRequest.current;
  setListBusy(true);setListError('');
  try{
   const result=await call<KnowledgeDocumentSummary[]>('knowledge.list',{brandId:brand.id});
   if(seq!==listRequest.current)return;
   setDocuments(result);
   setSelectedId(current=>result.some(document=>document.id===current)?current:'');
  }catch(error){
   if(seq===listRequest.current)setListError(errorMessage(error));
  }finally{
   if(seq===listRequest.current)setListBusy(false);
  }
 }
 useEffect(()=>{void load();return()=>{listRequest.current++;detailRequest.current++}},[]);

 async function loadDetail(){
  const documentId=selectedId,seq=++detailRequest.current;
  setDetail(null);setDetailError('');setReviewNote('');
  if(!documentId){setDetailBusy(false);return}
  setDetailBusy(true);
  try{
   const result=await call<KnowledgeDocument>('knowledge.list',{brandId:brand.id,documentId});
   if(seq===detailRequest.current&&selectedRef.current===documentId)setDetail(result);
  }catch(error){
   if(seq===detailRequest.current&&selectedRef.current===documentId)setDetailError(errorMessage(error));
  }finally{
   if(seq===detailRequest.current)setDetailBusy(false);
  }
 }
 useEffect(()=>{
  if(detail?.id===selectedId)return;
  void loadDetail();
  return()=>{detailRequest.current++};
 },[selectedId]);

 function update<K extends keyof KnowledgeForm>(key:K,value:KnowledgeForm[K]){
  setForm(current=>({...current,[key]:value}));
 }
 function beginEdit(document:KnowledgeDocument){
  setEditing({id:document.id,expectedRevision:document.revision,requestId:crypto.randomUUID()});
  setForm({productId:document.productId??'',title:document.title,source:document.source,format:document.format,effectiveAt:document.effectiveAt??'',risk:document.risk,content:document.content});
  setReviewNote('');
 }
 function cancelEdit(){setEditing(null);setForm(emptyForm())}
 async function saveDocument(){
  if(createTask.busy||reviewTask.busy||!canCreate)return;
  await createTask.run(()=>{
   const input:KnowledgeImportRequest={
    brandId:brand.id,
    productId:form.productId||null,
    title:form.title.trim(),
    source:form.source.trim(),
    format:form.format,
    effectiveAt:form.effectiveAt||null,
    risk:form.risk,
    content:form.content,
    requestId:editing?.requestId??crypto.randomUUID(),
   };
   if(editing){const payload:KnowledgeReviseRequest={...input,id:editing.id,expectedRevision:editing.expectedRevision};return call<KnowledgeDocument>('knowledge.revise',{...payload})}
   return call<KnowledgeDocument>('knowledge.import',{...input});
  },saved=>{
   detailRequest.current++;
   setDocuments(current=>[summarize(saved),...current.filter(document=>document.id!==saved.id)]);
   setDetail(saved);setDetailError('');setDetailBusy(false);setReviewNote('');
   setSelectedId(saved.id);setEditing(null);
   setForm(emptyForm());
  },editing?'知识文档已修订，旧审核已失效，请重新审核':'知识文档已创建，状态为待审核；请人工核对后再审核');
 }
 async function review(decision:KnowledgeReviewDecision){
  if(!selectedDetail||reviewTask.busy||createTask.busy||selectedDetail.reviewStatus!=='pending')return;
  const note=reviewNote.trim();
  if(decision==='rejected'&&!note)return;
  const documentId=selectedDetail.id;
  const payload:KnowledgeReviewRequest={brandId:brand.id,id:documentId,expectedRevision:selectedDetail.revision,decision,note};
  await reviewTask.run(()=>call<KnowledgeDocument>('knowledge.review',{...payload}),saved=>{
   setDocuments(current=>current.map(document=>document.id===saved.id?summarize(saved):document));
   if(selectedRef.current===documentId){setDetail(saved);setReviewNote('')}
   if(editing?.id===documentId)cancelEdit();
  },decision==='approved'?'知识文档已人工审核通过':'知识文档已人工驳回');
 }

 return <div className="attribution-workbench">
  <div className="page-heading"><div><h1>知识库</h1><p>{brand.name} · 本地保存品牌知识，按文档核对正文、切片与审核记录。</p></div></div>
  <p className="notice">知识保存在本机，不会自动联网。知识审核不替代商品事实审核。</p>
  <Panel title="知识文档" description="选择文档后按需读取正文、切片与审核历史。" action={<button disabled={listBusy} onClick={()=>void load()}>重试读取</button>}>
   {listBusy&&<p role="status">正在读取知识文档…</p>}
   {listError&&<p role="alert" className="feedback error">{listError}</p>}
   {!listBusy&&!listError&&!documents.length&&<Empty title="还没有知识文档">使用下方表单创建第一份知识文档。</Empty>}
   {!!documents.length&&<div className="compact-list">{documents.map(document=><button key={document.id} disabled={reviewTask.busy} aria-pressed={selectedId===document.id} onClick={()=>setSelectedId(document.id)}><span><strong>{document.title}</strong><small>{document.source} · 修订 {document.revision} · {document.chunkCount} 个切片</small></span><Badge tone={document.reviewStatus==='approved'?'green':document.reviewStatus==='pending'?'amber':'neutral'}>{reviewLabels[document.reviewStatus]}</Badge></button>)}</div>}
  </Panel>

  {selected&&<Panel title="文档预览" description="正文与切片均为只读预览；审核使用当前详情的修订号。">
   {detailBusy&&<p role="status">正在读取文档详情…</p>}
   {detailError&&<><p role="alert" className="feedback error">{detailError}</p><button onClick={()=>void loadDetail()}>重试详情</button></>}
   {!detailBusy&&!detailError&&!selectedDetail&&<p role="status">等待读取文档详情…</p>}
   {selectedDetail&&<section className="cohort" aria-label="文档详情">
    <h3>{selectedDetail.title}</h3>
    <dl className="attribution-metrics">
     <div><dt>审核状态</dt><dd>{reviewLabels[selectedDetail.reviewStatus]}</dd></div>
     <div><dt>风险等级</dt><dd>{riskLabels[selectedDetail.risk]}</dd></div>
     <div><dt>内容格式</dt><dd>{formatLabels[selectedDetail.format]}</dd></div>
     <div><dt>修订号</dt><dd>{selectedDetail.revision}</dd></div>
     <div><dt>关联商品</dt><dd>{products.find(product=>product.id===selectedDetail.productId)?.name??'未关联'}</dd></div>
     <div><dt>生效日期</dt><dd>{selectedDetail.effectiveAt||'未设置'}</dd></div>
     <div><dt>审核时间</dt><dd>{selectedDetail.reviewedAt?time(selectedDetail.reviewedAt):'尚未审核'}</dd></div>
     <div><dt>创建时间</dt><dd>{time(selectedDetail.createdAt)}</dd></div>
     <div><dt>更新时间</dt><dd>{time(selectedDetail.updatedAt)}</dd></div>
    </dl>
    <p>来源：{selectedDetail.source}</p>
    <div className="actions"><button disabled={createTask.busy||reviewTask.busy} onClick={()=>beginEdit(selectedDetail)}>修订此文档</button></div>
    <h3>正文预览</h3><pre className="preview">{selectedDetail.content}</pre>
    <h3>切片预览</h3>
    {selectedDetail.chunks.length?<ol className="queue-list">{selectedDetail.chunks.map(chunk=><li key={chunk.id}><strong>切片 {chunk.ordinal}</strong><p>{chunk.text}</p><small>SHA-256：{chunk.sha256}</small></li>)}</ol>:<p className="inline-note">本文档没有可预览的切片。</p>}
    <h3>审核历史</h3>
    {selectedDetail.reviews.length?<ol className="queue-list">{selectedDetail.reviews.map(review=><li key={review.id}><strong>{reviewLabels[review.decision]} · 修订 {review.revision}</strong><p>{review.note||'未填写审核理由'}</p><small>{time(review.createdAt)}</small></li>)}</ol>:<p className="inline-note">尚无审核记录。</p>}
    <Field label="审核理由（驳回必填）" hint="批准可留空；驳回必须填写理由。"><textarea rows={3} disabled={createTask.busy||reviewTask.busy||selectedDetail.reviewStatus!=='pending'} value={reviewNote} onChange={event=>setReviewNote(event.target.value)}/></Field>
    <div className="actions">
     <button className="primary" disabled={createTask.busy||reviewTask.busy||selectedDetail.reviewStatus!=='pending'} onClick={()=>void review('approved')}>人工审核通过</button>
     <button className="danger" disabled={createTask.busy||reviewTask.busy||selectedDetail.reviewStatus!=='pending'||!reviewNote.trim()} onClick={()=>void review('rejected')}>人工驳回</button>
    </div>
    {reviewTask.feedback}
   </section>}
  </Panel>}

  <Panel title={editing?'修订知识文档':'创建知识文档'} description={editing?'保存后生成新修订、保留历史，并回到待审核；失败会保留输入。':'提交失败会保留全部未提交输入；连续点击不会重复创建。'}>
   <form onSubmit={submit(()=>void saveDocument())}>
    <fieldset className="task-fields" disabled={createTask.busy||reviewTask.busy}>
     <div className="form-grid">
      <Field label="关联商品"><select value={form.productId} onChange={event=>update('productId',event.target.value)}><option value="">不关联商品</option>{products.map(product=><option key={product.id} value={product.id}>{product.name} · {product.sku}</option>)}</select></Field>
      <Field label="文档标题"><input required maxLength={300} value={form.title} placeholder="例如：虚构台灯保养指南" onChange={event=>update('title',event.target.value)}/></Field>
      <Field label="资料来源"><input required maxLength={2000} value={form.source} placeholder="例如：虚构产品手册" onChange={event=>update('source',event.target.value)}/></Field>
      <Field label="内容格式"><select value={form.format} onChange={event=>update('format',event.target.value as KnowledgeFormat)}><option value="markdown">Markdown</option><option value="text">纯文本</option></select></Field>
      <Field label="生效日期（可选）"><input type="date" value={form.effectiveAt} onChange={event=>update('effectiveAt',event.target.value)}/></Field>
      <Field label="风险等级"><select value={form.risk} onChange={event=>update('risk',event.target.value as KnowledgeRisk)}><option value="low">低风险</option><option value="medium">中风险</option><option value="high">高风险</option></select></Field>
     </div>
     <Field label="正文"><textarea required rows={9} value={form.content} placeholder={'# 虚构保养指南\n\n仅用于演示知识文档正文。'} onChange={event=>update('content',event.target.value)}/></Field>
     <div className="actions"><button className="primary" disabled={createTask.busy||reviewTask.busy||!canCreate}>{editing?'保存修订并重新审核':'创建待审核文档'}</button>{editing&&<button type="button" disabled={createTask.busy||reviewTask.busy} onClick={cancelEdit}>取消修订</button>}</div>
    </fieldset>
    {createTask.feedback}
   </form>
  </Panel>
 </div>;
}
