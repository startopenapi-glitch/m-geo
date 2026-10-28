import {useEffect,useRef,useState} from 'react';
import {FilePenLine,Plus,Sparkles,Download,ShieldCheck} from 'lucide-react';
import {call} from '../api';
import type {AiJob,Draft,DraftClaim,Kind} from '../types';
import {channelLabel,channelList} from '../channels';
import type {ChannelId} from '../channels';
import type {PageProps} from '../App';
import {Badge,Empty,Field,Panel,TaskForm,submit,time,useTask} from '../components/ui';
import DraftCopyButton from '../components/DraftCopyButton';
import KnowledgeRecall from '../components/KnowledgeRecall';
import GeoQualityPanel from '../components/GeoQualityPanel';
import {draftCanExport,draftIsCurrent,productHasCurrentFacts} from '../components/draftEligibility';
import {factIsApproved} from '../components/factReview';
import {inspectDraftQuality,localQualityInspectionDisclaimer} from '../qualityInspection';
export const kinds:Record<Kind,string>={description:'商品介绍',faq:'常见问题 FAQ',guide:'选购指南',comparison:'对比内容'};
type ChannelBatch={brandId:string;drafts:Draft[]};
type GenerationResult=Draft|ChannelBatch|null;
export default function Content(props:PageProps){
 const {brand,data,refresh}=props;
 const [productId,setProductId]=useState(''),[kind,setKind]=useState<Kind>('description'),[questionId,setQuestionId]=useState(''),[consent,setConsent]=useState(false),[draftId,setDraftId]=useState(''),[mode,setMode]=useState('local'),[selectedChannels,setSelectedChannels]=useState<ChannelId[]>([]);
 const task=useTask(),channelMode=selectedChannels.length>0;
 const product=data.products.find(p=>p.id===productId),draft=data.drafts.find(d=>d.id===draftId);
 async function generate():Promise<GenerationResult>{
  if(channelMode)return call<ChannelBatch>('draft.generateChannels',{brandId:brand.id,productId,channels:selectedChannels});
  if(mode==='ai'){await call('ai.start',{brandId:brand.id,productId,kind,questionId:questionId||null,consent:true});return null}
  return call<Draft>('draft.generateLocal',{brandId:brand.id,productId,kind,questionId:questionId||null});
 }
 function generated(result:GenerationResult){
  if(result&&'drafts' in result)setDraftId(result.drafts.at(-1)?.id??'');
  else if(result)setDraftId(result.id);
 }
 function toggleChannel(id:ChannelId){setSelectedChannels(current=>current.includes(id)?current.filter(item=>item!==id):[...current,id]);setConsent(false)}
 return <>
 <div className="page-heading"><div><div className="eyebrow">CONTENT STUDIO</div><h1>内容审核</h1><p>把已确认的事实，写成值得核对的内容。</p></div><Badge tone="blue">人工审核后导出</Badge></div>
 <div className="content-grid"><div>
 <Panel title="开始一份新内容" description="本地草稿无需模型；AI 仅使用已确认、公开且未过期的商品事实。">
 <TaskForm task={task} onSubmit={submit(()=>void task.runPersisted(generate,mode==='ai'&&!channelMode?async()=>{}:refresh,generated,channelMode?`已生成 ${selectedChannels.length} 个渠道草稿，请逐篇核对`:mode==='ai'?'AI 任务已创建，请查看下方进度':'本地草稿已生成，请核对正文'))}>
 <Field label="用于生成的商品"><select required value={productId} onChange={e=>{setProductId(e.target.value);setQuestionId('');setConsent(false)}}><option value="">选择商品</option>{data.products.map(p=><option key={p.id} value={p.id}>{p.name}{productHasCurrentFacts(p)?'':' · 待确认'}</option>)}</select></Field>
 <Field label="多渠道本地模板（可选）" hint="选择一个或多个渠道后，只生成独立本地草稿，不调用模型、不连接平台账号、不自动发布。">
 <div className="channel-options">{channelList.map(profile=><div className="fact-card" key={profile.id}>
 <label className="check"><input type="checkbox" checked={selectedChannels.includes(profile.id)} onChange={()=>toggleChannel(profile.id)}/><strong>{profile.label} · {profile.template}</strong></label>
 <p>{profile.description}</p><p className="inline-note">风险提示：{profile.reminder}</p>
 <a href={profile.ruleUrl} target="_blank" rel="noreferrer">{profile.ruleLabel} · {profile.checkedAt} 核对</a>
 </div>)}</div>
 </Field>
 {!channelMode&&<><div className="form-grid"><Field label="内容类型"><select value={kind} onChange={e=>setKind(e.target.value as Kind)}>{Object.entries(kinds).map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></Field><Field label="关联问题（可选）"><select value={questionId} onChange={e=>setQuestionId(e.target.value)}><option value="">不指定问题</option>{data.questions.filter(q=>q.productId===productId&&q.confirmed).map(q=><option key={q.id} value={q.id}>{q.text}</option>)}</select></Field></div>
 <Field label="生成方式"><select value={mode} onChange={e=>{setMode(e.target.value);setConsent(false)}}><option value="local">本地资料草稿 · 无模型费用</option><option value="ai">AI 辅助草稿 · 使用配置模型</option></select></Field></>}
 {product&&!productHasCurrentFacts(product)&&<p className="notice">请先在商品资料中确认当前版本的公开事实。</p>}
 {!channelMode&&mode==='ai'&&<label className="check"><input type="checkbox" checked={consent} onChange={e=>setConsent(e.target.checked)}/>同意发送当前商品已确认的公开事实与所选问题，了解模型可能计费</label>}
 <button className="primary" disabled={task.busy||!product||!productHasCurrentFacts(product)||(!channelMode&&mode==='ai'&&!consent)}>{!channelMode&&mode==='ai'?<Sparkles size={16}/>:<Plus size={16}/>} {channelMode?`生成 ${selectedChannels.length} 个渠道草稿`:`生成${mode==='ai'?' AI':'本地'}草稿`}</button>
 </TaskForm></Panel>
 <Panel title="草稿列表" description={`${data.drafts.length} 份内容，保留版本与审核状态`}>{data.drafts.length?<div className="draft-list">{data.drafts.map(d=><button key={d.id} className={d.id===draftId?'selected':''} onClick={()=>setDraftId(d.id)}><FilePenLine size={19}/><span><strong>{d.title}</strong><small>{d.channel?`${channelLabel(d.channel)} · 模板 v${d.channelTemplateVersion??1}`:kinds[d.kind]} · {d.generationMode==='ai'?'AI 辅助':'本地资料'} · 稿件 v{d.revision}</small></span><Badge tone={draftCanExport(data,d,brand.id)?'green':'amber'}>{draftCanExport(data,d,brand.id)?'已审核':'待核对'}</Badge></button>)}</div>:<Empty title="第一份内容，从事实开始">选择已确认商品，生成可编辑的本地资料草稿。</Empty>}</Panel>
 <Jobs {...props} openDraft={setDraftId}/>
 </div><div>{draft?<DraftEditor key={`${draft.id}-${draft.revision}-${draft.reviewedAt}`} {...props} draft={draft}/>:<Panel title="内容编辑与审核"><Empty title="选择一份草稿，开始核对">先检查事实、依据与表达，再审核并导出。内容预览以纯文本显示，资料中的 HTML 不会执行。</Empty><div className="editor-placeholder"><span/><span/><span/><span/></div></Panel>}</div></div>
 </>
}
export function DraftEditor({brand,data,refresh,draft}:PageProps&{draft:Draft}){
 const [title,setTitle]=useState(draft.title),[body,setBody]=useState(draft.body),[claims,setClaims]=useState<DraftClaim[]>(draft.claims??[]),[assetIds,setAssetIds]=useState<string[]>(draft.assetIds??[]),[format,setFormat]=useState('markdown');const task=useTask();const dirty=title!==draft.title||body!==draft.body||JSON.stringify(claims)!==JSON.stringify(draft.claims??[])||JSON.stringify(assetIds)!==JSON.stringify(draft.assetIds??[]);const current=draftIsCurrent(data,draft,brand.id);const exportable=draftCanExport(data,draft,brand.id)&&!dirty;const product=data.products.find(p=>p.id===draft.productId&&p.brandId===brand.id);const quality=product?inspectDraftQuality({...draft,title,body,claims},product,brand.id):null;const updateClaim=(index:number,key:'text'|'factId',value:string)=>setClaims(items=>items.map((claim,i)=>{if(i!==index)return claim;const {evidence,...editable}=claim;return {...editable,[key]:value}}));
 const assets=(data.assets??[]).filter(asset=>asset.brandId===brand.id&&(!asset.productId||asset.productId===draft.productId)&&(!asset.archivedAt||assetIds.includes(asset.id)));
 function toggleAsset(id:string){setAssetIds(current=>current.includes(id)?current.filter(item=>item!==id):[...current,id])}
 function useAsset(id:string){const asset=assets.find(item=>item.id===id);if(!asset||asset.archivedAt||asset.reviewStatus!=='approved')return;if(asset.kind==='title')setTitle(asset.text);else setBody(current=>current.includes(asset.text)?current:`${current}${current.trim()?'\n\n':''}${asset.text}`.slice(0,30000));setAssetIds(current=>current.includes(id)?current:[...current,id])}
 return <Panel title="核对并编辑内容" description={`${channelLabel(draft.channel)}${draft.channel?` · 模板 v${draft.channelTemplateVersion??1}`:''} · 稿件 v${draft.revision} · 来源商品 v${draft.sourceRevision} · ${time(draft.createdAt)}`} action={<Badge tone={exportable?'green':'amber'}>{exportable?'当前已审核':'待核对'}</Badge>}>
<TaskForm task={task} onSubmit={submit(()=>void task.runPersisted(()=>{const input=claims.map(({id,text,factId})=>({id,text,factId}));setClaims(input);return call('draft.save',{brandId:brand.id,id:draft.id,expectedRevision:draft.revision,title,body,claims:input,assetIds})},refresh,undefined,'修改已保存，请重新审核'))}>
<Field label="草稿标题">
<input required value={title} onChange={e=>setTitle(e.target.value)}/>
</Field>
<Field label="草稿正文" hint={`${body.length.toLocaleString()} / 30,000 字；预览禁用原始 HTML 与脚本`}>
<textarea className="draft-editor" required maxLength={30000} rows={17} value={body} onChange={e=>setBody(e.target.value)}/>
</Field>
<section aria-label="关键词与标题资产">
<div className="subheading"><h3>关键词与标题资产</h3><Badge tone="neutral">可选 · 非商品事实</Badge></div>
<p className="inline-note">仅可新选用已审核资产。点击“用于标题”或“插入正文”会实际使用并关联；仅勾选但未写入标题或正文时，原生审核会拦截。资产不是事实依据，关键主张仍须关联商品事实。</p>
{assets.length?<div className="draft-list">{assets.map(asset=><div className="fact-card" key={asset.id}>
<label className="check"><input type="checkbox" aria-label={`关联资产：${asset.text}`} checked={assetIds.includes(asset.id)} disabled={(!!asset.archivedAt||asset.reviewStatus!=='approved')&&!assetIds.includes(asset.id)} onChange={()=>toggleAsset(asset.id)}/><strong>{asset.kind==='title'?'标题':'关键词'} · {asset.text}</strong></label>
<p>{asset.source} · 资产 v{asset.revision} · {asset.archivedAt?'已归档，仅供历史快照追溯':asset.reviewStatus==='approved'?'已审核':asset.reviewStatus==='pending'?'待审核':'已驳回'}</p>
{!asset.archivedAt&&asset.reviewStatus==='approved'&&<button type="button" onClick={()=>useAsset(asset.id)}>{asset.kind==='title'?'用于标题':'插入正文'}</button>}
</div>)}</div>:<p className="inline-note">当前商品暂无可用的品牌通用或商品专属资产，可先在“内容资产”中录入并审核。</p>}
{!!draft.assetSnapshots?.length&&<div className="fact-card"><strong>本稿已冻结的资产版本</strong>{draft.assetSnapshots.map(snapshot=><p className="inline-note" data-testid={`asset-snapshot-${snapshot.assetId}`} key={snapshot.assetId}>{snapshot.kind==='title'?'标题':'关键词'}：{snapshot.text} · 来源：{snapshot.source} · 审核版本 v{snapshot.assetRevision} · {time(snapshot.assetReviewedAt)}</p>)}</div>}
</section>
<KnowledgeRecall brandId={brand.id} productId={draft.productId}/>
<section aria-label="关键主张与事实依据">
<div className="subheading">
<h3>关键主张与事实依据</h3>
<button type="button" onClick={()=>setClaims(items=>[...items,{id:crypto.randomUUID(),text:'',factId:''}])}>
<Plus size={14}/>登记关键主张</button>
</div>
<p className="inline-note">每条主张须与正文原文一致，并关联当前商品的一条已审核公开事实。审核通过时冻结依据快照；保存或修改后须重新审核，导出附带此依据清单。</p>{!claims.length&&<p className="notice">尚未登记关键主张，当前不能审核或导出。AI 与旧稿件需手动补齐；本地新生成稿件会自动关联。</p>}{claims.map((claim,index)=>
<div className="fact-card" key={claim.id}>
<Field label={`关键主张 ${index+1}`} hint="粘贴正文中的原文，最多 4,000 字">
<textarea rows={2} maxLength={4000} value={claim.text} onChange={e=>updateClaim(index,'text',e.target.value)}/>
</Field>
<Field label={`依据事实 ${index+1}`}>
<select value={claim.factId} onChange={e=>updateClaim(index,'factId',e.target.value)}>
<option value="">选择有来源的公开事实</option>{product?.facts.filter(f=>f.visibility==='public'&&factIsApproved(f)).map(f=>
<option key={f.id} value={f.id}>{f.label}：{f.value} · 来源：{f.source||'缺少来源'}</option>)}</select>
</Field>{claim.evidence&&<p className="inline-note" data-testid={`evidence-snapshot-${index}`}>审核快照：{claim.evidence.label}：{claim.evidence.value} · 来源：{claim.evidence.source} · 事实审核：{claim.evidence.factReviewedAt?time(claim.evidence.factReviewedAt):'旧版兼容记录'}</p>}<button type="button" onClick={()=>setClaims(items=>items.filter((_,i)=>i!==index))}>移除主张 {index+1}</button>
</div>)}</section>{!current&&<p className="notice">商品事实未确认、已过期、版本变化或关联问题未确认。请核对资料与问题并重新生成当前版本内容。</p>}<div className="actions">
<button className="primary" disabled={task.busy||!dirty||!title.trim()||!body.trim()}>保存修改</button>
<button type="button" disabled={task.busy||dirty||!current} onClick={()=>void task.runPersisted(()=>call('draft.review',{brandId:brand.id,id:draft.id,expectedRevision:draft.revision}),refresh,undefined,'当前稿件已审核')}>
<ShieldCheck size={16}/>确认内容并审核</button>
</div>
</TaskForm>
<details open={quality?.status==='blocked'}>
<summary>本地内容质检 · {quality?quality.status==='blocked'?'发现阻断缺口':quality.status==='review'?'需要人工复核':'未命中规则':'无法检查'}</summary>
{quality?<>
<p><Badge tone={quality.status==='pass'?'green':'amber'}>{quality.status==='pass'?'规则通过':quality.status==='blocked'?'存在阻断':'需要复核'}</Badge> 关键主张 {quality.claimsTotal} 条，已关联有来源的已审核公开事实 {quality.claimsWithEvidence} 条。</p>
{quality.issues.length?<ul>{quality.issues.map((issue,index)=><li key={`${issue.code}-${issue.claimId??'draft'}-${index}`}><strong>{issue.severity==='block'?'阻断':'复核'}：</strong>{issue.message}</li>)}</ul>:<p>未命中本地规则；仍须核对事实、适用范围和限制。</p>}
<p>核对价格、适用范围、限制与来源；竞品对比需要双方可核实的依据。</p>
</>:<p className="notice">找不到与当前品牌匹配的草稿商品，无法执行本地质检。</p>}
<p className="inline-note">{localQualityInspectionDisclaimer}</p>
</details>
<GeoQualityPanel draft={{body,kind:draft.kind,channel:draft.channel}} onApplyCleanedBody={setBody}/>
<details>
<summary>导出前正文预览</summary>
<pre className="preview">{body}</pre>
</details>
<div className="export-bar">
<Field label="导出格式">
<select value={format} onChange={e=>setFormat(e.target.value)}>
<option value="markdown">Markdown</option>
<option value="html">安全静态 HTML</option>
</select>
</Field>
<button disabled={task.busy||!exportable} onClick={()=>void task.run(()=>call<{saved:true}|null>('file.exportDraft',{brandId:brand.id,id:draft.id,expectedRevision:draft.revision,format}),undefined,result=>result?'内容已保存到所选文件':'已取消导出，未创建文件')}>
<Download size={16}/>导出已审核内容</button>{exportable&&<DraftCopyButton key={`${draft.id}-${draft.revision}`} brandId={brand.id} draft={draft}/>}</div>
<p className="inline-note">导出前请展开预览核对内容。本地导出文件不是公开网页；发布后需另行登记真实网址。</p>
</Panel>
}
const statusNames:Record<AiJob['status'],string>={queued:'等待处理',running:'生成中',succeeded:'已完成',failed:'失败',cancelled:'已取消',interrupted:'已中断'};
export function Jobs({brand,refresh,openDraft}:{brand:PageProps['brand'];refresh:PageProps['refresh'];openDraft?:(id:string)=>void}){
 const [jobs,setJobs]=useState<AiJob[]>([]),[error,setError]=useState(''),[candidate,setCandidate]=useState('');const task=useTask(),alive=useRef(true),seen=useRef(new Map<string,AiJob['status']>()),request=useRef(0);
 async function load(){const n=++request.current;try{const items=await call<AiJob[]>('ai.jobs',{brandId:brand.id});if(!alive.current||n!==request.current)return;const finished=items.filter(j=>j.status==='succeeded'&&seen.current.get(j.id)!=='succeeded');if(finished.some(j=>j.draftId))await refresh();if(!alive.current||n!==request.current)return;items.forEach(j=>seen.current.set(j.id,j.status));setJobs(items);setError('')}catch(e){if(alive.current&&n===request.current)setError((e as Error).message)}}
 useEffect(()=>{alive.current=true;void load();const id=window.setInterval(()=>void load(),4000);return()=>{alive.current=false;request.current++;clearInterval(id)}},[]);
 return <Panel title="AI 任务" description="任务不会自动重试；中断或失败后，需主动重新发起，可能再次计费。" action={<button className="text-button" onClick={()=>void load()}>刷新</button>}>{error&&<p role="alert" className="feedback error">{error}</p>}{!jobs.length&&!error&&<p className="inline-note">还没有 AI 任务。你可以随时使用本地资料草稿。</p>}<div className="job-list">{jobs.map(j=>
<div key={j.id}>
<div>
<Badge tone={j.status==='succeeded'?'green':'neutral'}>{statusNames[j.status]}</Badge>
<strong>{j.productId?'内容生成':'资料候选提取'}</strong>
<small>{j.message} · {time(j.createdAt)}</small>
</div>
<div className="actions">{['queued','running'].includes(j.status)&&<button disabled={task.busy} onClick={()=>void task.run(async()=>{await call('ai.cancel',{brandId:brand.id,id:j.id});await load()},undefined,'已请求本地取消；不代表上游停止处理或计费')}>取消任务</button>}{j.draftId&&openDraft&&<button onClick={()=>openDraft(j.draftId!)}>打开草稿</button>}{j.resultText&&!j.draftId&&<button onClick={()=>setCandidate(j.resultText!)}>查看候选</button>}</div>
</div>)}</div>{candidate&&<>
<div className="notice">候选仅供核对，不会自动更新商品事实。请将确认内容手动录入商品资料。</div>
<pre className="preview">{candidate}</pre>
<button onClick={()=>setCandidate('')}>关闭候选</button>
</>}{task.feedback}</Panel>
}
