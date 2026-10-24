import {useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {draftCanExport} from './draftEligibility';
import {Field,Panel,useTask} from './ui';
interface Package {readiness?:{status:string;warnings:number;blockers:number;checks:{code:string;status:string;message:string;productId?:string}[]};previewOnly:boolean;productCount:number;files:{path:string;content:string}[]}
interface Audit {checks:{code:string;status:string;message:string;path?:string}[];remoteChecks:{check:string;status:string}[];summary:{pass:number;warning:number;blocked:number}}
const readinessStatuses:Record<string,string>={pass:'通过',warning:'待补充',blocked:'待配置'};
const readinessFields:Record<string,string>={domain:'官网地址',brand:'商品品牌',image:'商品图片',audience:'适用人群',scenario:'使用场景',limits:'使用限制',afterSales:'售后说明',offer:'购买信息'};
export default function SiteExport({brand,data}:PageProps){
 const [audit,setAudit]=useState<Audit|null>(null);
 const [auditKey,setAuditKey]=useState('');
 const [baseUrl,setBase]=useState(''),[ids,setIds]=useState<string[]>([]),[preview,setPreview]=useState<Package|null>(null);const task=useTask();const snapshotKey=JSON.stringify({brand:brand.id,data});const currentSnapshot=useRef(snapshotKey);currentSnapshot.current=snapshotKey;const [previewKey,setPreviewKey]=useState('');const visiblePreview=previewKey===snapshotKey?preview:null;const eligible=data.drafts.filter(d=>draftCanExport(data,d,brand.id));
 const drafts=eligible.filter(d=>ids.includes(d.id)).map(d=>({id:d.id,expectedRevision:d.revision}));const payload={brandId:brand.id,baseUrl,drafts};const requestKey=JSON.stringify(payload);const currentRequest=useRef(requestKey);currentRequest.current=requestKey;
 const visibleAudit=auditKey===snapshotKey+requestKey?audit:null;
 return <Panel title="商品网站内容包" description="把审核后的商品介绍、FAQ、选购指南整合成可阅读的网页，附带结构化数据与通用商品目录。这里只保存本地文件。"><Field label="预期官网发布目录（可稍后提供）"><input type="url" value={baseUrl} onChange={e=>{setBase(e.target.value);setPreview(null);setAudit(null)}} placeholder="https://brand.example.com/products/"/></Field><p className="inline-note">留空导出本地预览，包含 noindex。提供官网后生成 canonical 与 sitemap；文件尚未发布。选择同一商品的多份稿件会合并到一个商品页。</p><details open><summary>选择当前已审核稿件 · 已选 {drafts.length} 份</summary>{eligible.map(d=><label className="check" key={d.id}><input type="checkbox" checked={ids.includes(d.id)} onChange={e=>{setIds(a=>e.target.checked?[...a,d.id]:a.filter(id=>id!==d.id));setPreview(null);setAudit(null)}}/>{d.title} · v{d.revision}</label>)}{!eligible.length&&<p>先确认商品资料并审核至少一份稿件。</p>}</details><div className="actions"><button disabled={task.busy||!drafts.length} onClick={()=>void task.run(()=>call<Package>('site.prepare',payload),result=>{if(currentSnapshot.current===snapshotKey&&currentRequest.current===requestKey){setPreview(result);setPreviewKey(snapshotKey)}},'内容包检查完成，尚未公开发布')}>检查网站内容包</button><button className="primary" disabled={task.busy||!drafts.length} onClick={()=>void task.run(()=>call<{saved:true}|null>('file.exportSite',payload),undefined,r=>r?'网站 ZIP 已保存到本机；尚未公开发布':'已取消保存网站包')}>导出网站 ZIP</button><button disabled={task.busy||!drafts.length} onClick={()=>void task.run(()=>call<Audit>('site.audit',payload),result=>{
  if(currentSnapshot.current===snapshotKey&&currentRequest.current===requestKey){
   setAudit(result);
   setAuditKey(snapshotKey+requestKey);
  }
 },'本地页面检查完成；远程检查仍待人工完成')}>运行本地页面检查</button>
 <button disabled={task.busy||!drafts.length} onClick={()=>void task.run(()=>call<{saved:true}|null>('file.exportSiteAudit',payload),undefined,r=>r?'部署检查单已保存；尚未发布':'已取消保存部署检查单')}>导出部署检查单</button>
 </div>{task.feedback}
 {visibleAudit&&<section aria-label="本地页面检查">
  <strong>本地页面检查：{visibleAudit.summary.pass} 项通过 · {visibleAudit.summary.warning} 项提醒 · {visibleAudit.summary.blocked} 项阻碍</strong>
  <p>尚未发布。预期官网地址尚未测试。本地结果不代表公开可访问、抓取许可、收录或推荐。</p>
  <ul>{visibleAudit.checks.slice(0,500).map((c,i)=><li key={i}>{({pass:'通过',warning:'提醒',blocked:'阻碍'} as Record<string,string>)[c.status]??'待核对'}：{c.message}{c.path?`（${c.path}）`:''}</li>)}</ul>
  <section aria-label="待人工部署检查">
   <strong>部署后远程检查 · 全部待人工完成</strong>
   <ul>{visibleAudit.remoteChecks.slice(0,20).map((c,i)=><li key={i}>待检查：{c.check}</li>)}</ul>
  </section>
 </section>}
 {visiblePreview&&<div className="cohort"><strong>{visiblePreview.previewOnly?'本地预览 · 尚缺官网地址':'已配置预期官网地址 · 待人工发布'} · {visiblePreview.productCount} 件商品</strong><p>发布状态：尚未发布。robots.txt 必须部署或合并到网站根目录；子目录文件不代表根目录 robots 策略。</p>{visiblePreview.readiness&&<section aria-label="本地完整性检查"><strong>本地完整性：{visiblePreview.readiness.warnings} 项提醒 · {visiblePreview.readiness.blockers} 项发布阻碍</strong><ul>{visiblePreview.readiness.checks.map((c,i)=><li key={i}>{readinessStatuses[c.status]??'待核对'} · {c.productId?`${data.products.find(p=>p.id===c.productId&&p.brandId===brand.id)?.name||'历史商品'} · `:''}{readinessFields[c.code]??'资料检查'}：{c.message}</li>)}</ul></section>}<p>通用目录用于后续 Feed 适配，尚未接入任何平台商家通道。</p>{visiblePreview.files.map(f=><details key={f.path}><summary>{f.path}</summary><pre className="preview">{f.content}</pre></details>)}</div>}</Panel>
}
