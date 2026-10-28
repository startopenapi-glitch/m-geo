import SiteExport from '../components/SiteExport';
import {useState} from 'react';
import {ExternalLink,RefreshCw,Send,Undo2} from 'lucide-react';
import {call} from '../api';
import type {PageProps} from '../App';
import type {Publication} from '../types';
import DraftCopyButton from '../components/DraftCopyButton';
import {draftCanExport} from '../components/draftEligibility';
import {Badge,Empty,Field,Panel,now,TaskForm,submit,time,useTask} from '../components/ui';
import {channelLabel,channelList} from '../channels';
import {latestReadback,publicationDisplayStatus,publicationLifecycle,publicationStatus,readbackCount,readbackErrorLabels,statusLabels,withdrawalOf,type PublicationStatus,type PublicationWithdrawalStatus} from '../publicationStatus';

function tone(status:PublicationStatus):'blue'|'green'|'neutral'|'amber'{
 if(status==='online'||status==='current'||status==='withdrawn')return 'green';
 if(status==='registered'||status==='historical')return 'neutral';
 if(status==='pending_readback'||status==='withdrawing')return 'blue';
 return 'amber';
}

export default function Publications({brand,data,refresh}:PageProps){
 const [draftId,setDraft]=useState(''),[channel,setChannel]=useState(''),[url,setUrl]=useState(''),[publishedAt,setDate]=useState(now()),[note,setNote]=useState(''),[snapshot,setSnapshot]=useState(''),[statusFilter,setStatusFilter]=useState('all');
 const [readbackId,setReadbackId]=useState(''),[withdrawalId,setWithdrawalId]=useState(''),[withdrawalStatus,setWithdrawalStatus]=useState<PublicationWithdrawalStatus>('withdrawing'),[withdrawalReason,setWithdrawalReason]=useState(''),[withdrawalConfirmed,setWithdrawalConfirmed]=useState(false);
 const task=useTask(),eligible=data.drafts.filter(d=>draftCanExport(data,d,brand.id)),draft=eligible.find(d=>d.id===draftId);
 const rows=data.publications.map(publication=>({publication,status:publicationDisplayStatus(data,publication,brand.id,readbackId===publication.id),version:publicationStatus(data,publication,brand.id)})),visible=statusFilter==='all'?rows:rows.filter(row=>row.status===statusFilter);
 const withdrawalPublication=data.publications.find(p=>p.id===withdrawalId);
 function selectDraft(id:string){setDraft(id);const selected=eligible.find(item=>item.id===id);setChannel(selected?.channel?channelLabel(selected.channel):'')}
 function openWithdrawal(p:Publication){const current=withdrawalOf(p);setWithdrawalId(p.id);setWithdrawalStatus(current?.status||'withdrawing');setWithdrawalReason('');setWithdrawalConfirmed(false)}
 function readback(p:Publication){
  setReadbackId(p.id);
  void task.runPersisted(()=>call('publication.readback',{brandId:brand.id,id:p.id}),refresh,undefined,'公开 URL 回读结果已保存；不代表平台收录、推荐或引用').finally(()=>setReadbackId(''));
 }
 return <>
 <div className="page-heading"><div><div className="eyebrow">PUBLICATION LOG</div><h1>发布记录</h1><p>留存真实网址、当时发布的内容版本，以及显式公开回读和人工撤回记录。</p></div><Badge tone="blue">手工登记 · 固定快照</Badge></div>
 <div className="notice">请先将内容人工发布到公开页面，再登记真实网址。未主动回读时不会联网核查，也不代表平台已抓取、推荐或引用。回读只核查该公开 URL 的响应与固定快照是否匹配，不会自动发布，也不代表平台已收录。撤回仅是人工登记，不宣称远端页面或平台缓存已删除。</div>
 <SiteExport brand={brand} data={data} refresh={refresh}/>
 <div className="content-grid">
 <Panel title="登记实际发布结果" description="仅可选用当前版本已审核的稿件。登记后快照固定保存。">
 <TaskForm task={task} onSubmit={submit(()=>void task.runPersisted(async()=>{if(!draft)throw Error('请选择当前已审核稿件');return call('publication.add',{brandId:brand.id,draftId:draft.id,expectedRevision:draft.revision,channel,url,publishedAt:new Date(publishedAt).toISOString(),note})},refresh,()=>{setUrl('');setNote('')},'发布记录与当前稿件快照已保存'))}>
 <Field label="已审核稿件"><select required value={draftId} onChange={e=>selectDraft(e.target.value)}><option value="">选择已审核稿件</option>{eligible.map(d=><option key={d.id} value={d.id}>{d.title} · v{d.revision}</option>)}</select></Field>
 <Field label="实际发布渠道" hint="默认采用草稿渠道；如实际发布位置不同，请按事实更正。"><select required value={channel} onChange={e=>setChannel(e.target.value)}><option value="">选择实际发布渠道</option>{channelList.map(profile=><option key={profile.id} value={profile.label}>{profile.label}</option>)}<option value="品牌官网">品牌官网</option><option value="其他">其他</option></select></Field>
 <Field label="真实公开网址"><input required type="url" value={url} onChange={e=>setUrl(e.target.value)} placeholder="https://"/></Field>
 <Field label="实际发布时间"><input required type="datetime-local" value={publishedAt} onChange={e=>setDate(e.target.value)}/></Field>
 <Field label="发布备注"><textarea rows={3} value={note} onChange={e=>setNote(e.target.value)}/></Field>
 {draft&&<><details><summary>即将登记的内容预览 · v{draft.revision}</summary><pre className="preview">{draft.body}</pre></details><DraftCopyButton key={`${draft.id}-${draft.revision}`} brandId={brand.id} draft={draft}/></>}
 <button className="primary" disabled={task.busy||!draft||!channel}><Send size={16}/>保存发布记录</button>
 </TaskForm></Panel>
 <Panel title="人工发布版本台账" description={`${data.publications.length} 条记录 · 公开状态与本地版本状态分别显示`}>
 <Field label="状态筛选"><select value={statusFilter} onChange={e=>setStatusFilter(e.target.value)}><option value="all">全部状态</option>{Object.entries(statusLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field>
 {visible.length?<div className="publication-list">{visible.map(({publication:p,status,version})=>{const linked=data.drafts.find(d=>d.id===p.draftId&&d.brandId===brand.id),product=data.products.find(item=>item.id===linked?.productId&&item.brandId===brand.id),latest=latestReadback(p),withdrawal=withdrawalOf(p),lifecycle=publicationLifecycle(p,readbackId===p.id);return <article key={p.id}>
 <div className="row-between"><div className="actions"><Badge tone="blue">{p.channel}</Badge><Badge tone={tone(lifecycle)}>{statusLabels[lifecycle]}</Badge><Badge tone={tone(version)}>{statusLabels[version]}</Badge></div><small>{time(p.publishedAt)}</small></div>
 <h3>{linked?.title||'历史稿件'}</h3>
 <p><strong>{product?.name||'历史商品'}</strong>{product&&<> · SKU：{product.sku}</>}</p>
 <p className="inline-note">发布稿件 v{p.draftRevision} · 当前稿件 {linked?`v${linked.revision}`:'未在当前快照中'}{product&&linked?` · 商品 v${product.revision}／稿件依据 v${linked.sourceRevision}`:''}</p>
 <a href={p.url} target="_blank" rel="noreferrer">{p.url}<ExternalLink size={13}/></a>{p.url.startsWith('http://')&&<p className="inline-note">HTTP 公开 URL：连接未加密；建议改用 HTTPS。</p>}{p.note&&<p>{p.note}</p>}
 {latest&&<div className="fact-card"><strong>最近回读：{time(latest.fetchedAt)}</strong><p>HTTP {latest.httpStatus??'无响应'} · 稿件 v{latest.draftRevision} · {latest.matchedContent?'固定快照匹配':'固定快照未匹配'}</p><p>最终 URL：{latest.finalUrl}{latest.transport==='http'?'（HTTP 未加密）':''}</p>{latest.title&&<p>页面标题：{latest.title}</p>}<p>正文 SHA256：{latest.bodySha256||'未生成'}</p>{latest.errorCategory&&<p role="alert">错误分类：{readbackErrorLabels[latest.errorCategory]||latest.errorCategory}</p>}<small>保留 {readbackCount(p)} 次回读；不保存网页正文。</small></div>}
 {withdrawal&&<div className="fact-card"><strong>{statusLabels[withdrawal.status]}</strong><p>原因：{withdrawal.reason}</p><small>首次登记 {time(withdrawal.requestedAt)} · 最近更新 {time(withdrawal.updatedAt)}。仅为人工记录，不证明远端删除或平台停止收录。</small></div>}
 <div className="actions"><button type="button" disabled={task.busy||!!withdrawal} title={withdrawal?'撤回流程中的记录不能普通回读为在线':'显式读取此公开 URL，不自动重试'} onClick={()=>readback(p)}><RefreshCw size={15}/>{readbackId===p.id?'正在回读':'回读此公开 URL'}</button><button type="button" disabled={task.busy} onClick={()=>openWithdrawal(p)}><Undo2 size={15}/>登记撤回</button><button type="button" onClick={()=>setSnapshot(p.contentSnapshot)}>查看固定稿件快照</button></div>
 </article>})}</div>:<Empty title={data.publications.length?'当前筛选下没有记录':'还没有发布记录'}>{data.publications.length?'切换状态筛选查看其他人工记录。':'完成内容审核、实际发布后，登记第一个公开网址。'}</Empty>}
 </Panel></div>
 {withdrawalPublication&&<Panel title={`登记撤回：${withdrawalPublication.channel}`} description="只记录你人工观察到或已执行的撤回情况，不会请求平台删除，也不证明平台已处理。" action={<button type="button" onClick={()=>setWithdrawalId('')}>关闭</button>}><form onSubmit={submit(()=>void task.runPersisted(()=>call('publication.withdraw',{brandId:brand.id,id:withdrawalPublication.id,status:withdrawalStatus,reason:withdrawalReason,confirm:withdrawalConfirmed}),refresh,()=>{setWithdrawalId('');setWithdrawalReason('');setWithdrawalConfirmed(false)},'撤回登记已保存；不代表远端已删除或平台停止收录'))}><fieldset className="task-fields" disabled={task.busy}><Field label="撤回状态"><select value={withdrawalStatus} onChange={e=>setWithdrawalStatus(e.target.value as PublicationWithdrawalStatus)}><option value="withdrawing">撤回中</option><option value="withdrawn">已撤回（人工登记）</option><option value="withdrawal_unverified">撤回待核查</option></select></Field><Field label="撤回原因与核查说明"><textarea required rows={3} maxLength={2000} value={withdrawalReason} onChange={e=>setWithdrawalReason(e.target.value)} placeholder="说明人工操作、观察结果及仍需核查的事项"/></Field><label className="check"><input type="checkbox" checked={withdrawalConfirmed} onChange={e=>setWithdrawalConfirmed(e.target.checked)}/>我确认以上为人工登记，不将其视为远端删除或平台收录状态的证明</label><button className="primary" disabled={!withdrawalConfirmed||!withdrawalReason.trim()}>保存撤回登记</button></fieldset></form></Panel>}
 {snapshot&&<Panel title="发布时的固定内容快照" action={<button onClick={()=>setSnapshot('')}>关闭</button>}><pre className="preview">{snapshot}</pre></Panel>}
 </>
}
