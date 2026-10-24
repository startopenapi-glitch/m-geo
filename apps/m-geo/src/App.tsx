import OptimizationWorkbench from './pages/OptimizationWorkbench';
import OperationsWorkbench from './pages/OperationsWorkbench';
import AttributionWorkbench from './pages/AttributionWorkbench';
import FeedWorkbench from './pages/FeedWorkbench';
import TrialPreparation from './pages/TrialPreparation';
import {evidenceCounts} from './evidence';
import CommerceDashboard from './components/CommerceDashboard';
import {useCallback,useEffect,useRef,useState} from 'react';
import {LayoutDashboard,Package,FilePenLine,Send,ChartNoAxesCombined,Settings,Plus,ArrowUpRight,ShieldCheck,RefreshCw} from 'lucide-react';
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
export type Section='dashboard'|'pilot'|'feeds'|'products'|'content'|'publications'|'visibility'|'attribution'|'operations'|'optimization'|'settings';
const nav=[['dashboard','工作台',LayoutDashboard],['pilot','试点准备',ShieldCheck],['products','商品资料',Package],['feeds','商品通道',Package],['content','内容工作室',FilePenLine],['publications','发布记录',Send],['visibility','AI 表现',ChartNoAxesCombined],['attribution','访问与订单',ChartNoAxesCombined],['operations','批量运营',RefreshCw],['optimization','优化闭环',ShieldCheck],['settings','设置',Settings]] as const;
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
 const brand=brands.find(b=>b.id===selected);
 async function createBrand(){if(restoreTask.busy||channelLock.current)return;await create.run(()=>call<Brand>('brand.create',{name}),b=>{setBrands(current=>[...current,b]);if(!channelLock.current){setSelected(b.id);setSection('dashboard')}setName('');setAdding(false)},'品牌已创建')}
 const brandForm=<form onSubmit={submit(()=>void createBrand())}><Field label="品牌名称"><input required maxLength={100} value={name} placeholder="例如：你的品牌" onChange={e=>setName(e.target.value)}/></Field><button className="primary" disabled={create.busy||!name.trim()}>创建品牌</button>{adding&&<button type="button" onClick={()=>setAdding(false)}>取消</button>}{create.feedback}</form>;
 return <div className="app-shell"><aside className="sidebar"><div className="wordmark"><img src="/brand/m-logo.png" alt="M- GEO 原始 Logo"/><span>M- GEO<small>商品内容与 AI 表现</small></span></div><div className="brand-switch"><label htmlFor="brand">品牌工作区</label><select id="brand" aria-label="当前品牌" value={selected} disabled={!brands.length||navigationLocked} onChange={e=>{if(!restoreTask.busy&&!channelLock.current){setSelected(e.target.value);setAdding(false)}}}>{!brands.length&&<option value="">尚未创建品牌</option>}{brands.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select><button className="text-button" disabled={navigationLocked} onClick={()=>{if(!restoreTask.busy&&!channelLock.current)setAdding(true)}}><Plus size={14}/>添加品牌</button></div><nav aria-label="主要导航">{nav.map(([key,label,Icon])=><button key={key} disabled={navigationLocked} aria-current={section===key?'page':undefined} className={section===key?'active':''} onClick={()=>navigate(key)}><Icon size={19}/>{label}{section===key&&<span className="nav-dot"/>}</button>)}</nav><div className="sidebar-foot"><ShieldCheck size={18}/><div>资料保存在本机<small>每个品牌独立整理</small></div></div></aside><main><header className="topbar"><div><span className="breadcrumb">品牌工作区</span><span className="slash">/</span><strong>{nav.find(n=>n[0]===section)?.[1]}</strong></div><Badge tone="blue">测试版 · {version}</Badge></header>{channelBusy&&<div className="page" style={{paddingBottom:0}}><p className="notice" role="status">请等待本地资料读取或保存完成后再切换品牌或页面；可在文件对话框中取消。</p></div>}{(restoreTask.busy||restoreTask.error||restoreTask.message||recoveryPath)&&<div className="page" style={{paddingBottom:0}} ref={restoreNotice} tabIndex={-1} aria-label="工作区恢复状态">
 {restoreTask.feedback}
 {restoreTask.busy&&<p className="notice">正在选择备份或恢复工作区，完成前暂时无法切换品牌或编辑资料。可在文件选择器中取消。</p>}
 {recoveryPath&&<div className="notice"><p>最近一次恢复前快照（使用该次恢复口令加密）：</p><p style={{overflowWrap:'anywhere'}}>{recoveryPath}</p><button disabled={restoreTask.busy||recoveryTask.busy} onClick={()=>void recoveryTask.run(()=>call('app.openRecoveryFolder'),undefined,'已请求打开恢复前快照目录')}>打开快照目录</button>{recoveryTask.feedback}</div>}
 </div>}<fieldset disabled={restoreTask.busy} aria-label="工作区内容" style={{border:0,padding:0,margin:0,minWidth:0}}>{loading?<div className="page"><p role="status">正在打开本地工作区…</p></div>:error?<div className="page"><Panel title="暂时无法打开工作区"><p role="alert" className="feedback error">{error}</p><button onClick={()=>void load()}><RefreshCw size={16}/>重试</button></Panel><BackupPanel {...restoreProps} restoreOnly/></div>:adding||!brand?<div className="onboarding"><div className="eyebrow">WELCOME TO M- GEO</div><img src="/brand/m-logo.png" alt=""/><h1>{adding?'添加一个品牌工作区':'让 AI 更准确地认识你的商品'}</h1><p>创建品牌整理商品事实，或从备份恢复工作区。<br/>无需云端账号，也可以先用本地资料制作内容。</p><Panel title={adding?'新品牌':'从你的第一个品牌开始'}>{brandForm}</Panel>{!brand&&<BackupPanel {...restoreProps} restoreOnly/>}<small>本地保存 · 多品牌管理 · 模型按需配置</small></div>:<Workspace key={`${brand.id}-${reset}`} brand={brand} section={section} setSection={navigate} onChannelBusyChange={onChannelBusyChange} {...restoreProps}/> }</fieldset></main></div>
}
function Workspace({brand,section,setSection,restore,restoring,onChannelBusyChange}:{brand:Brand;section:Section;setSection:(s:Section)=>void;onChannelBusyChange?:(busy:boolean)=>void}&RestoreProps){
 const [data,setData]=useState<WorkspaceSnapshot|null>(null),[error,setError]=useState(''),[loading,setLoading]=useState(false);const seq=useRef(0);
 async function refresh(){const n=++seq.current;setLoading(true);setError('');try{const result=await call<WorkspaceSnapshot>('workspace.snapshot',{brandId:brand.id});if(n!==seq.current)throw new Error('工作区刷新已被更新的请求替代，请重试');setData(result)}catch(e){if(n===seq.current)setError((e as Error).message);throw e}finally{if(n===seq.current)setLoading(false)}}
 useEffect(()=>{void refresh().catch(()=>{});return()=>{seq.current++}},[]);
 if(!data)return <div className="page">{error?<><p role="alert" className="feedback error">{error}</p><button onClick={()=>void refresh().catch(()=>{})}>重试</button><BackupPanel restore={restore} restoring={restoring} restoreOnly/></>:<p role="status">正在读取 {brand.name} 的资料…</p>}</div>;
 const props={brand,data,refresh};
 return <div className="page">{error&&<p role="alert" className="feedback error">{error}<button onClick={()=>void refresh().catch(()=>{})}>重试</button></p>}{loading&&<span className="refresh-state" role="status">正在更新资料…</span>}{data.truncated&&<p role="alert" className="notice">资料量超过单次显示上限，当前列表并非全部记录。请先备份工作区。</p>}{section==='dashboard'?<Dashboard {...props} go={setSection}/>:section==='pilot'?<TrialPreparation {...props}/>:section==='products'?<Products {...props}/>:section==='feeds'?<FeedWorkbench brandId={brand.id} data={data} onBusyChange={onChannelBusyChange}/>:section==='content'?<Content {...props}/>:section==='publications'?<Publications {...props}/>:section==='visibility'?<Visibility {...props} onBusyChange={onChannelBusyChange}/>:section==='attribution'?<AttributionWorkbench brandId={brand.id} data={data}/>:section==='optimization'?<OptimizationWorkbench brandId={brand.id} data={data} onNavigate={setSection}/>:section==='operations'?<OperationsWorkbench brandId={brand.id} data={data}/>:<SettingsPage {...props} restore={restore} restoring={restoring}/>}</div>
}
function Dashboard({brand,data,go}:PageProps&{go:(s:Section)=>void}){
 const counts=evidenceCounts(data.observations,data.campaigns);const confirmed=data.products.filter(p=>productHasCurrentFacts(p)).length,review=data.drafts.filter(d=>!draftCanExport(data,d,brand.id)).length;
 return <><div className="page-heading"><div><div className="eyebrow">YOUR BRAND, CLEARLY UNDERSTOOD</div><h1>今天，从准确的资料开始</h1><p>{brand.name} 的商品内容与实测进展，都在这里。</p></div><button className="primary" onClick={()=>go('products')}><Plus size={17}/>添加商品资料</button></div><div className="hero"><div><Badge tone="blue">一步一步，建立可信内容</Badge><h2>让每一次介绍<br/>都有事实可循。</h2><p>整理商品资料，制作可核对的内容，再用真实回答观察变化。</p><button onClick={()=>go(confirmed?'content':'products')}>{confirmed?'开始制作内容':'整理商品事实'}<ArrowUpRight size={17}/></button></div><div className="hero-art" aria-hidden="true"><div className="orbit orbit-one"/><div className="orbit orbit-two"/><img src="/brand/m-logo.png" alt=""/><span className="floating-note note-one"><ShieldCheck size={16}/>依据清晰</span><span className="floating-note note-two"><FilePenLine size={16}/>内容可核对</span></div></div><div className="stats"><Stat label="商品资料" value={data.products.length} detail={`${confirmed} 件已确认`} icon={<Package/>}/><Stat label="待审核草稿" value={review} detail="人工核对后可导出" icon={<FilePenLine/>}/><Stat label="实际发布记录" value={data.publications.length} detail="保存渠道与稿件快照" icon={<Send/>}/><Stat label="人工实测记录" value={counts.recorded||'未测'} detail={`模拟 ${counts.simulated} · 未分类 ${counts.unspecified}`} icon={<ChartNoAxesCombined/>}/></div><CommerceDashboard data={data}/><div className="dashboard-grid"><Panel title="下一步，继续推进" description="从资料到实测，保留每一步的证据">{[['01','确认商品事实',`${data.products.length-confirmed} 件资料待确认`,'products'],['02','制作并审核内容',`${review} 份草稿待审核`,'content'],['03','记录平台回答',data.observations.length?'保持同一问题与入口复测':'还没有平台实测样本','visibility']].map(([n,title,desc,target])=><button className="next-row" key={n} onClick={()=>go(target as Section)}><span className="step-number">{n}</span><span><strong>{title}</strong><small>{desc}</small></span><ArrowUpRight size={18}/></button>)}</Panel><Panel title="你的商品资料" action={<button className="text-button" onClick={()=>go('products')}>查看全部<ArrowUpRight size={14}/></button>}>{data.products.length?<div className="compact-list">{data.products.slice(0,4).map(p=><button key={p.id} onClick={()=>go('products')}><span className="product-icon"><Package size={20}/></span><span><strong>{p.name}</strong><small>{p.sku}</small></span><Badge tone={productHasCurrentFacts(p)?'green':'amber'}>{productHasCurrentFacts(p)?'已确认':'待确认'}</Badge></button>)}</div>:<Empty title="还没有商品资料" action={<button onClick={()=>go('products')}>添加第一件商品</button>}>从名称、SKU 和一条有依据的事实开始。</Empty>}</Panel></div><p className="footnote">发布、提及、引用与销售分别记录。内容产出不等于平台收录或经营收益。</p></>
}
function Stat({label,value,detail,icon}:{label:string;value:string|number;detail:string;icon:React.ReactNode}){return <div className="stat"><div><span>{label}</span>{icon}</div><strong>{value}</strong><small>{detail}</small></div>}
