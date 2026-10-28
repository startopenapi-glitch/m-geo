import {useEffect,useMemo,useRef,useState} from 'react';
import {Badge,Empty,Field,Panel,now,submit,time,useTask} from '../components/ui';
import type {WorkspaceSnapshot} from '../types';
import {
 aliasesFromText,aliasesToText,archiveResearch,confirmResearch,confirmResearchToQuestion,
 isoToLocalDateTime,listResearch,localDateTimeToIso,normalizeResearchQuestion,researchDuplicateKey,
 saveBrandProfile,saveResearch,
 type BrandProfile,type BrandWithProfile,type ResearchEntry,type ResearchIntent,
 type ResearchSaveRequest,type ResearchSourceKind,type ResearchStatus,
} from '../brandResearch';

export interface BrandResearchProps {
 brand:BrandWithProfile;
 data:WorkspaceSnapshot;
 refresh:()=>Promise<void>;
 onBrandSaved?:(brand:BrandWithProfile)=>void;
 onBusyChange?:(busy:boolean)=>void;
}
interface ResearchForm {
 productId:string;
 question:string;
 intent:ResearchIntent;
 sourceKind:ResearchSourceKind;
 sourceLabel:string;
 sourceUrl:string;
 observedAt:string;
 clusterKey:string;
}
interface Filters {status:''|ResearchStatus;sourceKind:''|ResearchSourceKind;productId:string;query:string;observedFrom:string;observedTo:string;clusterKey:string}
const emptyFilters:Filters={status:'',sourceKind:'',productId:'',query:'',observedFrom:'',observedTo:'',clusterKey:''};
const emptyResearch=():ResearchForm=>({productId:'',question:'',intent:'selection',sourceKind:'manual',sourceLabel:'',sourceUrl:'',observedAt:now(),clusterKey:''});
const statusLabels:Record<ResearchStatus,string>={candidate:'候选',confirmed:'已确认',archived:'已归档'};
const sourceLabels:Record<ResearchSourceKind,string>={manual:'手动录入',imported:'外部资料导入',suggested:'本地模板建议'};
const intentLabels:Record<ResearchIntent,string>={selection:'选购',comparison:'对比',usage:'使用',after_sales:'售后'};
const errorMessage=(error:unknown)=>error instanceof Error?error.message:'读取未完成，请重试';

export default function BrandResearch(props:BrandResearchProps){return <BrandResearchSession key={props.brand.id} {...props}/>}

function BrandResearchSession({brand,data,refresh,onBrandSaved,onBusyChange}:BrandResearchProps){
 const products=data.products.filter(product=>product.brandId===brand.id);
 const [profile,setProfile]=useState<BrandProfile>(()=>brand.profile??{aliases:[],officialUrl:'',manufacturer:'',category:'',description:'',updatedAt:''});
 const [aliasText,setAliasText]=useState(()=>aliasesToText(brand.profile?.aliases??[]));
 const [items,setItems]=useState<ResearchEntry[]>([]);
 const [truncated,setTruncated]=useState(false),[loading,setLoading]=useState(false),[loadError,setLoadError]=useState('');
 const [form,setForm]=useState<ResearchForm>(emptyResearch),[editing,setEditing]=useState<ResearchEntry|null>(null);
 const [filters,setFilters]=useState<Filters>(emptyFilters);
 const request=useRef(0),profileTask=useTask(),researchTask=useTask(),transitionTask=useTask();
 const busy=profileTask.busy||researchTask.busy||transitionTask.busy;

 async function load(){
  const sequence=++request.current;setLoading(true);setLoadError('');
  try{
   const result=await listResearch({brandId:brand.id,limit:5000});
   if(sequence!==request.current)return;
   setItems(result.items);setTruncated(result.truncated);
  }catch(error){if(sequence===request.current)setLoadError(errorMessage(error))}
  finally{if(sequence===request.current)setLoading(false)}
 }
 useEffect(()=>{void load();return()=>{request.current++}},[]);

 const visible=useMemo(()=>items.filter(item=>{
  const date=item.observedAt.slice(0,10),query=filters.query.trim().toLowerCase();
  return (!filters.status||item.status===filters.status)
   &&(!filters.sourceKind||item.sourceKind===filters.sourceKind)
   &&(!filters.productId||item.productId===filters.productId)
   &&(!filters.clusterKey||item.clusterKey===filters.clusterKey)
   &&(!filters.observedFrom||date>=filters.observedFrom)
   &&(!filters.observedTo||date<=filters.observedTo)
   &&(!query||item.question.toLowerCase().includes(query)||item.sourceLabel.toLowerCase().includes(query));
 }),[items,filters]);
 const visibleClusters=useMemo(()=>{
  const groups=new Map<string,string[]>();
  for(const item of visible)if(item.clusterKey)groups.set(item.clusterKey,[...groups.get(item.clusterKey)??[],item.id]);
  return [...groups].filter(([,ids])=>ids.length>1).map(([clusterKey,researchIds])=>({clusterKey,count:researchIds.length,researchIds}));
 },[visible]);
 const duplicate=useMemo(()=>{
  if(!form.question.trim())return null;
  const key=researchDuplicateKey(form.productId||null,form.question);
  return items.find(item=>item.id!==editing?.id&&researchDuplicateKey(item.productId,item.question)===key)??null;
 },[form.productId,form.question,items,editing]);
 const clusterPeers=useMemo(()=>form.clusterKey.trim()?items.filter(item=>item.id!==editing?.id&&item.clusterKey===form.clusterKey.trim()):[],[form.clusterKey,items,editing]);

 function updateProfile<K extends keyof Omit<BrandProfile,'aliases'|'updatedAt'>>(key:K,value:BrandProfile[K]){setProfile(current=>({...current,[key]:value}))}
 async function submitProfile(){
  const aliases=aliasesFromText(aliasText);
  onBusyChange?.(true);
  try{await profileTask.run(()=>{
   if(aliases.length>20)throw new Error('品牌别名最多保存20个');
   return saveBrandProfile({brandId:brand.id,aliases,officialUrl:profile.officialUrl.trim(),manufacturer:profile.manufacturer.trim(),category:profile.category.trim(),description:profile.description.trim()});
  },saved=>{
   const next=saved.profile;
   if(next){setProfile(next);setAliasText(aliasesToText(next.aliases))}
   onBrandSaved?.(saved);
  },'品牌级档案已保存')}finally{onBusyChange?.(false)}
 }
 function updateForm<K extends keyof ResearchForm>(key:K,value:ResearchForm[K]){
  setForm(current=>({...current,[key]:value}));
 }
 function changeSourceKind(sourceKind:ResearchSourceKind){
  setForm(current=>({...current,sourceKind,sourceLabel:sourceKind==='suggested'?'M- GEO 本地模板建议；非真实搜索数据':current.sourceKind==='suggested'?'':current.sourceLabel}));
 }
 function beginEdit(item:ResearchEntry){
  setEditing(item);setForm({productId:item.productId??'',question:item.question,intent:item.intent,sourceKind:item.sourceKind,sourceLabel:item.sourceLabel,sourceUrl:item.sourceUrl,observedAt:isoToLocalDateTime(item.observedAt),clusterKey:item.clusterKey});
 }
 function cancelEdit(){setEditing(null);setForm(emptyResearch())}
 async function submitResearch(){
  if(duplicate)return;
  await researchTask.run(()=>{
   const input:ResearchSaveRequest={brandId:brand.id,productId:form.productId||null,question:form.question.trim(),intent:form.intent,sourceKind:form.sourceKind,sourceLabel:form.sourceLabel.trim(),sourceUrl:form.sourceUrl.trim(),observedAt:localDateTimeToIso(form.observedAt),clusterKey:form.clusterKey.trim()};
   if(editing){input.id=editing.id;input.expectedRevision=editing.revision}
   return saveResearch(input);
  },saved=>{
   setItems(current=>[saved,...current.filter(item=>item.id!==saved.id)]);
   setEditing(null);setForm(emptyResearch());
  },editing?'研究问题修订已保存':'研究问题已保存为候选');
 }
 function replaceItem(saved:ResearchEntry){setItems(current=>current.map(item=>item.id===saved.id?saved:item))}
 async function changeStatus(item:ResearchEntry,target:'confirmed'|'archived'){
  const input={brandId:brand.id,id:item.id,expectedRevision:item.revision};
  await transitionTask.run(()=>target==='confirmed'?confirmResearch(input):archiveResearch(input),saved=>{
   replaceItem(saved);if(editing?.id===saved.id)cancelEdit();
  },target==='confirmed'?'研究问题已确认，可加入固定问题集':'研究问题已归档');
 }
 async function addToQuestions(item:ResearchEntry){
  await transitionTask.runPersisted(
   ()=>confirmResearchToQuestion({brandId:brand.id,id:item.id,expectedRevision:item.revision}),
   refresh,
   result=>replaceItem(result.research),
   (result)=>result.question.confirmed?'已加入固定问题集':'该规范化问题已在固定问题集中，保留了原确认状态'
  );
 }

 return <div className="attribution-workbench">
  <div className="page-heading"><div><h1>品牌档案与问题研究台</h1><p>{brand.name} · 维护品牌级资料，并将有来源的问题整理进固定问题集。</p></div></div>
  <p className="notice">仅支持手动录入、已有资料导入和本地模板建议；不抓取平台、不伪造热搜，本地建议不代表真实搜索需求。</p>

  <Panel title="品牌级档案" description="这些资料描述品牌主体；商品自身的官网、分类与事实仍在商品资料中维护。">
   <form onSubmit={submit(()=>void submitProfile())}><fieldset className="task-fields" disabled={busy}>
    <div className="form-grid">
     <Field label="品牌官网"><input type="url" maxLength={2000} value={profile.officialUrl} placeholder="https://example.com" onChange={event=>updateProfile('officialUrl',event.target.value)}/></Field>
     <Field label="制造商 / 运营主体"><input maxLength={200} value={profile.manufacturer} onChange={event=>updateProfile('manufacturer',event.target.value)}/></Field>
     <Field label="品牌品类"><input maxLength={200} value={profile.category} placeholder="例如：家用饮具" onChange={event=>updateProfile('category',event.target.value)}/></Field>
     <Field label="档案更新时间"><input value={profile.updatedAt?time(profile.updatedAt):'尚未保存'} disabled readOnly/></Field>
    </div>
    <Field label="品牌别名（每行一个）" hint="自动去重，最多 20 个；用于记录品牌常见称谓。"><textarea rows={3} maxLength={2100} value={aliasText} onChange={event=>setAliasText(event.target.value)}/></Field>
    <Field label="品牌简介"><textarea rows={4} maxLength={5000} value={profile.description} onChange={event=>updateProfile('description',event.target.value)}/></Field>
    <div className="actions"><button className="primary" disabled={busy}>保存品牌档案</button></div>
   </fieldset>{profileTask.feedback}</form>
  </Panel>

  <Panel title={editing?'修订研究问题':'录入研究问题'} description="来源与观察时间随问题保存；修订不会改变候选或已确认状态。">
   <form onSubmit={submit(()=>void submitResearch())}><fieldset className="task-fields" disabled={busy}>
    <div className="form-grid">
     <Field label="关联商品" hint="未关联商品的问题不能加入固定问题集。"><select value={form.productId} onChange={event=>updateForm('productId',event.target.value)}><option value="">品牌级问题（不关联商品）</option>{products.map(product=><option key={product.id} value={product.id}>{product.name} · {product.sku}</option>)}</select></Field>
     <Field label="购买意图"><select value={form.intent} onChange={event=>updateForm('intent',event.target.value as ResearchIntent)}>{Object.entries(intentLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
     <Field label="来源类型"><select value={form.sourceKind} onChange={event=>changeSourceKind(event.target.value as ResearchSourceKind)}>{Object.entries(sourceLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
     <Field label="来源名称"><input required maxLength={500} value={form.sourceLabel} placeholder="例如：2026 年 1 月客服访谈" onChange={event=>updateForm('sourceLabel',event.target.value)}/></Field>
     <Field label="来源公开链接（可选）"><input type="url" maxLength={2000} value={form.sourceUrl} onChange={event=>updateForm('sourceUrl',event.target.value)}/></Field>
     <Field label="观察时间"><input required type="datetime-local" value={form.observedAt} onChange={event=>updateForm('observedAt',event.target.value)}/></Field>
     <Field label="聚类名称（可选）" hint="相同名称用于人工归组，不代表语义模型判断。"><input maxLength={120} value={form.clusterKey} placeholder="例如：清洁保养" onChange={event=>updateForm('clusterKey',event.target.value)}/></Field>
    </div>
    <Field label="研究问题" hint={normalizeResearchQuestion(form.question)?`规范化预览：${normalizeResearchQuestion(form.question)}`:'去除标点、空白与常见疑问后缀后检查重复。'}><textarea required rows={3} maxLength={1000} value={form.question} placeholder="例如：星云杯如何正确清洗？" onChange={event=>updateForm('question',event.target.value)}/></Field>
    {duplicate&&<p className="feedback error" role="alert">同一商品范围内已有重复问法：{duplicate.question}</p>}
    {!!clusterPeers.length&&<p className="inline-note">当前聚类另有 {clusterPeers.length} 条：{clusterPeers.slice(0,3).map(item=>item.question).join('；')}</p>}
    <div className="actions"><button className="primary" disabled={busy||!!duplicate||!form.question.trim()||!form.sourceLabel.trim()||!form.observedAt}>{editing?'保存研究修订':'保存候选问题'}</button>{editing&&<button type="button" disabled={busy} onClick={cancelEdit}>取消修订</button>}</div>
   </fieldset>{researchTask.feedback}</form>
  </Panel>

  <Panel title="研究问题库" description="筛选在当前品牌内进行；聚类提示只归组，不会自动合并或确认。" action={<button disabled={loading||busy} onClick={()=>void load()}>重新读取</button>}>
   <div className="form-grid">
    <Field label="状态"><select value={filters.status} onChange={event=>setFilters(current=>({...current,status:event.target.value as Filters['status']}))}><option value="">全部状态</option>{Object.entries(statusLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
    <Field label="来源"><select value={filters.sourceKind} onChange={event=>setFilters(current=>({...current,sourceKind:event.target.value as Filters['sourceKind']}))}><option value="">全部来源</option>{Object.entries(sourceLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
    <Field label="商品"><select value={filters.productId} onChange={event=>setFilters(current=>({...current,productId:event.target.value}))}><option value="">全部商品范围</option>{products.map(product=><option key={product.id} value={product.id}>{product.name}</option>)}</select></Field>
    <Field label="关键词"><input value={filters.query} placeholder="搜索问题或来源" onChange={event=>setFilters(current=>({...current,query:event.target.value}))}/></Field>
    <Field label="观察开始日期"><input type="date" value={filters.observedFrom} onChange={event=>setFilters(current=>({...current,observedFrom:event.target.value}))}/></Field>
    <Field label="观察结束日期"><input type="date" value={filters.observedTo} onChange={event=>setFilters(current=>({...current,observedTo:event.target.value}))}/></Field>
   </div>
   {filters.clusterKey&&<p className="inline-note">正在筛选聚类“{filters.clusterKey}” <button className="text-button" onClick={()=>setFilters(current=>({...current,clusterKey:''}))}>清除聚类筛选</button></p>}
   {loading&&<p role="status">正在读取研究问题…</p>}
   {loadError&&<p className="feedback error" role="alert">{loadError}</p>}
   {truncated&&<p className="notice">记录超过单页上限，请使用来源与日期筛选缩小范围。</p>}
   {!loading&&!loadError&&!visible.length&&<Empty title="没有匹配的研究问题">调整筛选，或在上方录入第一条有来源的问题。</Empty>}
   {!!visibleClusters.length&&<div className="notice">聚类提示：{visibleClusters.map(cluster=><button className="text-button" key={cluster.clusterKey} onClick={()=>setFilters(current=>({...current,clusterKey:cluster.clusterKey}))}>{cluster.clusterKey}（{cluster.count}）</button>)}</div>}
   {!!visible.length&&<div className="compact-list">{visible.map(item=><article key={item.id} className="cohort">
    <div><Badge tone={item.status==='confirmed'?'green':item.status==='candidate'?'amber':'neutral'}>{statusLabels[item.status]}</Badge> <Badge>{sourceLabels[item.sourceKind]}</Badge> <Badge>{intentLabels[item.intent]}</Badge></div>
    <h3>{item.question}</h3>
    <p>{products.find(product=>product.id===item.productId)?.name??'品牌级问题'} · {item.clusterKey||'未聚类'} · 修订 {item.revision}</p>
    <small>来源：{item.sourceLabel} · 观察：{time(item.observedAt)}{item.sourceUrl&&<> · <a href={item.sourceUrl} target="_blank" rel="noreferrer">公开来源</a></>}</small>
    <div className="actions">
     <button disabled={busy||item.status==='archived'} onClick={()=>beginEdit(item)}>修订</button>
     {item.status==='candidate'&&<button disabled={busy} onClick={()=>void changeStatus(item,'confirmed')}>确认研究问题</button>}
     {item.status==='confirmed'&&<button className="primary" disabled={busy||!item.productId} title={item.productId?'加入现有固定问题集':'请先修订并关联商品'} onClick={()=>void addToQuestions(item)}>确认加入固定问题集</button>}
     {item.status!=='archived'&&<button className="danger" disabled={busy} onClick={()=>void changeStatus(item,'archived')}>归档</button>}
    </div>
   </article>)}</div>}
   {transitionTask.feedback}
  </Panel>
 </div>;
}
