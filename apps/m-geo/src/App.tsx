import OptimizationWorkbench from './pages/OptimizationWorkbench';
import OperationsWorkbench from './pages/OperationsWorkbench';
import AttributionWorkbench from './pages/AttributionWorkbench';
import FeedWorkbench from './pages/FeedWorkbench';
import TrialPreparation from './pages/TrialPreparation';
import KnowledgeWorkbench from './pages/KnowledgeWorkbench';
import AssetLibrary from './pages/AssetLibrary';
import BrandResearch from './pages/BrandResearch';
import MediaLibrary from './pages/MediaLibrary';
import AutomationWorkbench from './pages/AutomationWorkbench';
import OfflineWorkflowWorkbench from './pages/OfflineWorkflowWorkbench';
import P0WorkflowWorkbench from './pages/P0WorkflowWorkbench';
import {evidenceCounts} from './evidence';
import {geoWorkflow} from './workflow';
import {useCallback,useEffect,useRef,useState} from 'react';
import {LayoutDashboard,Package,FilePenLine,ChartNoAxesCombined,Settings,Plus,ArrowUpRight,ShieldCheck,RefreshCw,ChevronDown} from 'lucide-react';
import {call} from './api';
import type {Brand,WorkspaceSnapshot} from './types';
import {Badge,Empty,Field,Panel,submit,useTask} from './components/ui';
import Products from './pages/Products';
import {draftCanExport,productHasCurrentFacts} from './components/draftEligibility';
import Content from './pages/Content';
import Publications from './pages/Publications';
import Visibility from './pages/Visibility';
import SettingsPage,{BackupPanel,type RestoreProps} from './pages/Settings';
import './styles.css';
import {version} from '../package.json';
export type Section='dashboard'|'pilot'|'feeds'|'products'|'research'|'knowledge'|'assets'|'media'|'content'|'publications'|'visibility'|'offlineWorkflow'|'p0Workflow'|'attribution'|'operations'|'optimization'|'automation'|'settings';
type PrimarySection='dashboard'|'products'|'content'|'visibility'|'settings';
const nav=[['dashboard','首页',LayoutDashboard],['products','事实与证据',Package],['content','问题与内容',FilePenLine],['visibility','AI 测试',ChartNoAxesCombined],['settings','数据与设置',Settings]] as const;
const taskNav:Record<Exclude<PrimarySection,'dashboard'>,[Section,string][]> = {
 products:[['products','商品事实'],['research','品牌与问题研究'],['knowledge','知识库'],['assets','内容资产'],['media','图片与视频'],['feeds','渠道资料']],
 content:[['pilot','问题与诊断'],['content','内容审核'],['publications','发布与撤回']],
 visibility:[['visibility','可见度测试'],['offlineWorkflow','本地自动化'],['p0Workflow','接入演练'],['automation','巡检与趋势'],['optimization','优化闭环']],
 settings:[['settings','备份与设置'],['operations','批量运营'],['attribution','访问与订单']],
};
function primarySection(section:Section):PrimarySection{
 if(section==='dashboard')return 'dashboard';
 for(const [key,items] of Object.entries(taskNav) as [Exclude<PrimarySection,'dashboard'>,[Section,string][]][])if(items.some(([target])=>target===section))return key;
 return 'dashboard';
}
function sectionLabel(section:Section):string{
 for(const items of Object.values(taskNav)){const match=items.find(([target])=>target===section);if(match)return match[1]}
 return '首页';
}
export interface PageProps{brand:Brand;data:WorkspaceSnapshot;refresh:()=>Promise<void>}
export default function App(){
 const [brands,setBrands]=useState<Brand[]>([]),[selected,setSelected]=useState(''),[loading,setLoading]=useState(true),[error,setError]=useState(''),[section,setSection]=useState<Section>('dashboard'),[adding,setAdding]=useState(false),[name,setName]=useState(''),[reset,setReset]=useState(0),[recoveryPath,setRecoveryPath]=useState('');
 const [channelBusy,setChannelBusy]=useState(false);
 const channelLock=useRef(false);
 const onChannelBusyChange=useCallback((busy:boolean)=>{channelLock.current=busy;setChannelBusy(busy)},[]);
 const request=useRef(0),restoreNotice=useRef<HTMLDivElement>(null),create=useTask(),restoreTask=useTask(),recoveryTask=useTask();
 async function load(){const seq=++request.current;setLoading(true);setError('');try{const items=await call<Brand[]>('brand.list');if(seq!==request.current)return;setBrands(items);setSelected(current=>items.some(b=>b.id===current)?current:items[0]?.id||'')}catch(e){if(seq===request.current)setError((e as Error).message)}finally{if(seq===request.current)setLoading(false)}}
 useEffect(()=>{void load();return()=>{request.current++}},[reset]);
 useEffect(()=>{if(!restoreTask.busy&&(restoreTask.message||restoreTask.error))restoreNotice.current?.focus()},[restoreTask.busy,restoreTask.message,restoreTask.error]);
 async function restoreWorkspace(password:string){
  if(create.busy||channelLock.current)return;
  await restoreTask.run(()=>call<{restored:true;recoveryPath:string}|null>('backup.restore',{password,confirm:true}),result=>{
   if(!result)return;
   // A restore replaces every brand, including records whose IDs happen to match.
   // Invalidate in-flight reads and unmount all old page state before reloading.
   request.current++;setLoading(true);setBrands([]);setSelected('');setAdding(false);setName('');setSection('dashboard');setRecoveryPath(result.recoveryPath);setReset(v=>v+1);
  },result=>result?'工作区已恢复，请重新配置模型连接':'已取消恢复，工作区未替换');
 }
 const navigationLocked=restoreTask.busy||channelBusy;
 function navigate(target:Section){if(!restoreTask.busy&&!channelLock.current)setSection(target)}
 const restoreProps={restore:restoreWorkspace,restoring:restoreTask.busy||create.busy};
 const brand=brands.find(b=>b.id===selected),activeSection=primarySection(section),primaryLabel=nav.find(n=>n[0]===activeSection)?.[1]||'首页',currentLabel=sectionLabel(section);
 async function createBrand(){if(restoreTask.busy||channelLock.current)return;await create.run(()=>call<Brand>('brand.create',{name}),b=>{setBrands(current=>[...current,b]);if(!channelLock.current){setSelected(b.id);setSection('dashboard')}setName('');setAdding(false)},'品牌已创建')}
 const brandForm=<form onSubmit={submit(()=>void createBrand())}>
<Field label="品牌名称">
<input required maxLength={100} value={name} placeholder="例如：你的品牌" onChange={e=>setName(e.target.value)}/>
</Field>
<button className="primary" disabled={create.busy||!name.trim()}>创建品牌</button>{adding&&<button type="button" onClick={()=>setAdding(false)}>取消</button>}{create.feedback}</form>;
 return <div className="app-shell">
<aside className="sidebar">
<div className="wordmark">
<img src="/brand/m-logo.png" alt="M- GEO 原始 Logo"/>
<span>M- GEO<small>商品内容与 AI 表现</small>
</span>
</div>
<div className="brand-switch">
<label htmlFor="brand">品牌工作区</label>
<select id="brand" aria-label="当前品牌" value={selected} disabled={!brands.length||navigationLocked} onChange={e=>{if(!restoreTask.busy&&!channelLock.current){setSelected(e.target.value);setAdding(false)}}}>{!brands.length&&<option value="">尚未创建品牌</option>}{brands.map(b=>
<option key={b.id} value={b.id}>{b.name}</option>)}</select>
<button className="text-button" disabled={navigationLocked} onClick={()=>{if(!restoreTask.busy&&!channelLock.current)setAdding(true)}}>
<Plus size={14}/>添加品牌</button>
</div>
<nav className="primary-nav" aria-label="主要导航">{nav.map(([key,label,Icon])=>{
 const expanded=activeSection===key,items=key==='dashboard'?[]:taskNav[key],subnavId=`nav-${key}`;
 return <div className="nav-group" key={key}>
 <button disabled={navigationLocked} aria-current={!items.length&&section===key?'page':undefined} aria-expanded={items.length?expanded:undefined} aria-controls={items.length?subnavId:undefined} className={expanded?'active':''} onClick={()=>navigate(items.length?items[0][0]:key)}>
 <Icon size={19}/><span>{label}</span>{items.length?<ChevronDown className="nav-chevron" size={15}/>:expanded&&<span className="nav-dot"/>}</button>
 {expanded&&items.length>0&&<div className="subnav" id={subnavId} role="group" aria-label={`${label}子导航`}>{items.map(([target,taskLabel])=>
 <button key={target} disabled={navigationLocked} aria-current={section===target?'page':undefined} className={section===target?'active':''} onClick={()=>navigate(target)}>{taskLabel}</button>)}</div>}
 </div>})}</nav>
<div className="sidebar-foot">
<ShieldCheck size={18}/>
<div>资料保存在本机<small>每个品牌独立整理</small>
</div>
</div>
</aside>
<main>
<header className="topbar">
<div className="topbar-location" aria-label="当前位置">
<span className="breadcrumb">{brand?.name||'品牌工作区'}</span>
<span className="slash">/</span>
<strong>{primaryLabel}</strong>
{currentLabel!==primaryLabel&&<><span className="slash">/</span><strong>{currentLabel}</strong></>}
</div>
<Badge tone="blue">测试版 · {version}</Badge>
</header>{channelBusy&&<div className="page" style={{paddingBottom:0}}>
<p className="notice" role="status">请等待本地资料读取或保存完成后再切换品牌或页面；可在文件对话框中取消。</p>
</div>}{(restoreTask.busy||restoreTask.error||restoreTask.message||recoveryPath)&&<div className="page" style={{paddingBottom:0}} ref={restoreNotice} tabIndex={-1} aria-label="工作区恢复状态">
 {restoreTask.feedback}
 {restoreTask.busy&&<p className="notice">正在选择备份或恢复工作区，完成前暂时无法切换品牌或编辑资料。可在文件选择器中取消。</p>}
 {recoveryPath&&<div className="notice">
<p>最近一次恢复前快照（使用该次恢复口令加密）：</p>
<p style={{overflowWrap:'anywhere'}}>{recoveryPath}</p>
<button disabled={restoreTask.busy||recoveryTask.busy} onClick={()=>void recoveryTask.run(()=>call('app.openRecoveryFolder'),undefined,'已请求打开恢复前快照目录')}>打开快照目录</button>{recoveryTask.feedback}</div>}
 </div>}<fieldset disabled={restoreTask.busy} aria-label="工作区内容" style={{border:0,padding:0,margin:0,minWidth:0}}>{loading?<div className="page">
<p role="status">正在打开本地工作区…</p>
</div>:error?<div className="page">
<Panel title="暂时无法打开工作区">
<p role="alert" className="feedback error">{error}</p>
<button onClick={()=>void load()}>
<RefreshCw size={16}/>重试</button>
</Panel>
<BackupPanel {...restoreProps} restoreOnly/>
</div>:adding||!brand?<div className="onboarding">
<div className="eyebrow">WELCOME TO M- GEO</div>
<img src="/brand/m-logo.png" alt=""/>
<h1>{adding?'添加一个品牌工作区':'让 AI 更准确地认识你的商品'}</h1>
<p>创建品牌整理商品事实，或从备份恢复工作区。<br/>无需云端账号，也可以先用本地资料制作内容。</p>
<Panel title={adding?'新品牌':'从你的第一个品牌开始'}>{brandForm}</Panel>{!brand&&<BackupPanel {...restoreProps} restoreOnly/>}<small>本地保存 · 多品牌管理 · 模型按需配置</small>
</div>:<Workspace key={`${brand.id}-${reset}`} brand={brand} onBrandSaved={saved=>setBrands(current=>current.map(item=>item.id===saved.id?saved:item))} section={section} setSection={navigate} onChannelBusyChange={onChannelBusyChange} {...restoreProps}/> }</fieldset>
</main>
</div>
}
function Workspace({brand,section,setSection,restore,restoring,onChannelBusyChange,onBrandSaved}:{brand:Brand;onBrandSaved:(brand:Brand)=>void;section:Section;setSection:(s:Section)=>void;onChannelBusyChange?:(busy:boolean)=>void}&RestoreProps){
 const [data,setData]=useState<WorkspaceSnapshot|null>(null),[error,setError]=useState(''),[loading,setLoading]=useState(false);const seq=useRef(0);
 async function refresh(){const n=++seq.current;setLoading(true);setError('');try{const result=await call<WorkspaceSnapshot>('workspace.snapshot',{brandId:brand.id});if(n!==seq.current)throw new Error('工作区刷新已被更新的请求替代，请重试');setData(result)}catch(e){if(n===seq.current)setError((e as Error).message);throw e}finally{if(n===seq.current)setLoading(false)}}
 useEffect(()=>{void refresh().catch(()=>{});return()=>{seq.current++}},[]);
 if(!data)return <div className="page">{error?<>
<p role="alert" className="feedback error">{error}</p>
<button onClick={()=>void refresh().catch(()=>{})}>重试</button>
<BackupPanel restore={restore} restoring={restoring} restoreOnly/>
</>:<p role="status">正在读取 {brand.name} 的资料…</p>}</div>;
 const props={brand,data,refresh};
 return <div className="page">{error&&<p role="alert" className="feedback error">{error}<button onClick={()=>void refresh().catch(()=>{})}>重试</button>
</p>}{loading&&<span className="refresh-state" role="status">正在更新资料…</span>}{data.truncated&&<p role="alert" className="notice">资料量超过单次显示上限，当前列表并非全部记录。请先备份工作区。</p>}{section==='dashboard'?<Dashboard {...props} go={setSection}/>:section==='pilot'?<TrialPreparation {...props}/>:section==='products'?<Products {...props}/>:section==='research'?<BrandResearch {...props} onBrandSaved={onBrandSaved} onBusyChange={onChannelBusyChange}/>:section==='knowledge'?<KnowledgeWorkbench {...props}/>:section==='assets'?<AssetLibrary {...props}/>:section==='media'?<MediaLibrary {...props}/>:section==='feeds'?<FeedWorkbench brandId={brand.id} data={data} onBusyChange={onChannelBusyChange}/>:section==='content'?<Content {...props}/>:section==='publications'?<Publications {...props}/>:section==='visibility'?<Visibility {...props} onBusyChange={onChannelBusyChange}/>:section==='attribution'?<AttributionWorkbench brandId={brand.id} data={data}/>:section==='p0Workflow'?<P0WorkflowWorkbench {...props} onNavigate={setSection} onBusyChange={onChannelBusyChange}/>:section==='offlineWorkflow'?<OfflineWorkflowWorkbench {...props} onNavigate={setSection} onBusyChange={onChannelBusyChange}/>:section==='automation'?<AutomationWorkbench brandId={brand.id} data={data}/>:section==='optimization'?<OptimizationWorkbench brandId={brand.id} data={data} onNavigate={setSection}/>:section==='operations'?<OperationsWorkbench brandId={brand.id} data={data}/>:<SettingsPage {...props} restore={restore} restoring={restoring}/>}</div>
}
function Dashboard({brand,data,go}:PageProps&{go:(s:Section)=>void}){
 const counts=evidenceCounts(data.observations,data.campaigns),confirmed=data.products.filter(p=>productHasCurrentFacts(p)).length,attention=data.drafts.filter(d=>!draftCanExport(data,d,brand.id)).length;
 const {steps,next}=geoWorkflow(data,brand.id);
 return <>
 <div className="page-heading"><div><div className="eyebrow">YOUR BRAND, CLEARLY UNDERSTOOD</div><h1>让 AI 准确理解你的商品</h1><p>{brand.name} · 从可信事实到可核对内容，再到可复核的 AI 回答。</p></div></div>
 <section className="next-action" aria-labelledby="next-action-title"><div><Badge tone="blue">当前下一步</Badge><h2 id="next-action-title">{next.title}</h2><p>{next.description}</p><button className="primary" onClick={()=>go(next.target)}>{next.action}<ArrowUpRight size={17}/></button></div><button className="text-button" onClick={()=>go(next.target==='visibility'?'products':'visibility')}>{next.target==='visibility'?'检查商品事实':'查看 AI 回答记录'}<ArrowUpRight size={14}/></button></section>
 <section className="workflow-panel" aria-labelledby="workflow-title"><div className="workflow-heading"><div><h2 id="workflow-title">GEO 工作流</h2><p>先保证事实准确，再观察特定测试条件下的提及与引用；每个阶段都可独立进入。</p></div><Badge>本地资料状态</Badge></div><ol className="geo-flow" aria-label="GEO 工作流阶段">{steps.map((step,index)=><li key={step.id} className={step.complete?'complete':next.step===step.id?'current':''}><button aria-current={next.step===step.id?'step':undefined} onClick={()=>go(step.target)}><span className="flow-index">{index+1}</span><span className="flow-copy"><strong>{step.title}</strong><small>{step.detail}</small></span><span className="flow-state">{step.complete?'已完成':next.step===step.id?'当前':'待继续'}</span></button></li>)}</ol></section>
 <div className="stats"><Stat label="已确认商品" value={confirmed} detail={`共 ${data.products.length} 件商品`} icon={<Package/>}/><Stat label="待处理内容" value={attention} detail="未审核或需要重新核对" icon={<FilePenLine/>}/><Stat label="AI 回答记录" value={data.observations.length} detail={`人工 ${counts.recorded} · 模拟 ${counts.simulated} · 未分类 ${counts.unspecified}`} icon={<ChartNoAxesCombined/>}/></div>
 <Panel title="商品概况" action={<button className="text-button" onClick={()=>go('products')}>查看全部<ArrowUpRight size={14}/></button>}>{data.products.length?<div className="compact-list">{data.products.slice(0,4).map(p=><button key={p.id} onClick={()=>go('products')}><span className="product-icon"><Package size={20}/></span><span><strong>{p.name}</strong><small>{p.sku}</small></span><Badge tone={productHasCurrentFacts(p)?'green':'amber'}>{productHasCurrentFacts(p)?'已确认':'待确认'}</Badge></button>)}</div>:<Empty title="还没有商品资料" action={<button onClick={()=>go('products')}>添加第一件商品</button>}>从名称、SKU 和一条有依据的事实开始。</Empty>}</Panel>
 <p className="footnote">发布、提及、引用与销售分别记录。本地测试不代表外部平台收录、推荐或经营收益。</p>
 </>
}
function Stat({label,value,detail,icon}:{label:string;value:string|number;detail:string;icon:React.ReactNode}){return <div className="stat">
<div>
<span>{label}</span>{icon}</div>
<strong>{value}</strong>
<small>{detail}</small>
</div>}
