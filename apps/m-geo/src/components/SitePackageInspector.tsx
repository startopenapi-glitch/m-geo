import {useEffect,useId,useRef,useState} from 'react';
import {call} from '../api';
import type {WorkspaceSnapshot} from '../types';
import type {InspectionVersionStatus,SiteInspectionEnvelope} from '../siteInspectionTypes';

interface Props {brandId:string;data:WorkspaceSnapshot}
interface DisplayedInspection {value:SiteInspectionEnvelope;epoch:number;outdated:boolean}
const versionLabels:Record<InspectionVersionStatus,string>={match:'一致',changed:'已变化',unavailable:'不可用'};
const checkLabels={pass:'通过',warning:'警告',blocked:'阻断'};
const caveat='仅核对本地文件与当前品牌，不证明文件来源可信、官网可达、收录或推荐；哈希不等于数字签名。';

/** A brand switch discards the whole local session, including pending callbacks. */
export default function SitePackageInspector(props:Props){
 return <InspectionSession key={props.brandId} {...props}/>;
}

function InspectionSession({brandId,data}:Props){
 const headingId=useId();
 const [inspection,setInspection]=useState<DisplayedInspection|null>(null);
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
 const alive=useRef(true),locked=useRef(false);
 const snapshot=useRef({data,epoch:0});
 // Track identity, not serialized/truncated UI contents. Returning to an older object
 // must not revive evidence checked before an intervening workspace replacement.
 if(snapshot.current.data!==data){snapshot.current={data,epoch:snapshot.current.epoch+1}}
 useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
 const report=inspection?.value.report;
 const outdated=!!inspection&&(inspection.outdated||inspection.epoch!==snapshot.current.epoch);

 async function run(action:'select'|'recheck'|'export'){
  if(locked.current||!alive.current)return;
  if(action!=='select'&&!inspection)return;
  if(action==='export'&&outdated)return;
  locked.current=true;
  const epoch=snapshot.current.epoch;
  const selected=inspection?.value;
  setBusy(true);setError('');setMessage('');
  if(action==='select')setInspection(null);
  if(action==='recheck')setInspection(old=>old?{...old,outdated:true}:old);
  try{
   if(action==='export'&&selected){
    const result=await call<{saved:true}|null>('file.exportSiteInspection',{
     brandId,inspectionId:selected.inspectionId,reportFingerprint:selected.report.reportFingerprint
    });
    if(!alive.current)return;
    setMessage(result?.saved?'验收报告 ZIP 已保存到本机。':'已取消导出验收报告，原报告仍可查看。');
   }else{
    const value=action==='select'
     ?await call<SiteInspectionEnvelope|null>('file.inspectSitePackage',{brandId})
     :await call<SiteInspectionEnvelope>('file.recheckSitePackage',{brandId,inspectionId:selected!.inspectionId});
    if(!alive.current)return;
    if(value){
     if(value.report.brandId!==brandId)throw new Error('验收报告与当前品牌不一致，请重新选择网站 ZIP。');
     setInspection({value,epoch,outdated:false});
     setMessage(action==='select'?'网站 ZIP 只读验收完成。':'已重新核对当前品牌。');
    }else if(action==='select'){
     setMessage('已取消选择网站 ZIP，请手动重新选择。');
    }else{
     throw new Error('验收会话已失效，请重新选择网站 ZIP。');
    }
   }
  }catch(reason){
   if(!alive.current)return;
   if(action!=='select')setInspection(old=>old?{...old,outdated:true}:old);
   setError(reason instanceof Error?reason.message:'验收操作未完成，请手动重试。');
  }finally{
   locked.current=false;
   if(alive.current)setBusy(false);
  }
 }

 return <section className="panel" aria-labelledby={headingId} style={{minWidth:0,overflowWrap:'anywhere'}}>
  <div className="panel-head"><div style={{minWidth:0}}>
   <h2 id={headingId}>已导出网站包验收</h2>
   <p>手动选择网站 ZIP，只读核对包内文件与当前品牌；不会修改商品、稿件或原网站包。</p>
  </div></div>
  <p>{caveat}</p>
  <div className="actions" style={{minWidth:0,flexWrap:'wrap'}}>
   <button type="button" disabled={busy} onClick={()=>void run('select')}>选择网站 ZIP 并只读验收</button>
   <button type="button" disabled={busy||!inspection} onClick={()=>void run('recheck')}>重新核对当前品牌</button>
   <button type="button" disabled={busy||!inspection||outdated} onClick={()=>void run('export')}>导出验收报告 ZIP</button>
  </div>
  {busy&&<p role="status">正在处理，请稍候…</p>}
  {error&&<p className="feedback error" role="alert">{error}</p>}
  {message&&<p role="status">{message}</p>}
  {report&&<div style={{minWidth:0}}>
   {outdated&&<p className="notice" role="status">报告已过时：当前资料已变化或核对未完成，请重新核对当前品牌后再导出。下方保留上次验收证据。</p>}
   <h3>本地文件验收报告</h3>
   <dl>
    <dt>网站包文件名</dt><dd>{report.source.fileName}</dd>
    <dt>原 ZIP SHA-256</dt><dd style={{marginInlineStart:0,overflowWrap:'anywhere'}}>{report.source.sha256}</dd>
    <dt>文件规模</dt><dd>压缩 {report.source.compressedBytes} 字节 · 解压 {report.source.uncompressedBytes} 字节 · {report.source.fileCount} 个文件</dd>
    <dt>验收时间</dt><dd>{report.checkedAt}</dd>
   </dl>
   <p>本地文件一致性：{checkLabels[report.packageStatus]}</p>
   <p>当前品牌版本：{versionLabels[report.currentStatus]}</p>
   <p>{report.previewOnly===null?'预览配置：不可判定':report.previewOnly?'仅预览（noindex）':'已配置域名（未验证）'}</p>
   {report.baseUrl&&<p>{report.baseUrl}</p>}
   <p>远程状态：未验证</p><p>发布状态：尚未发布</p>
   <p>商品 {report.products.length} · 检查 {report.checks.length} · 通过 {report.checks.filter(check=>check.status==='pass').length} · 警告 {report.checks.filter(check=>check.status==='warning').length} · 阻断 {report.checks.filter(check=>check.status==='blocked').length}</p>
   <h3>包内与当前版本</h3>
   {report.products.length===0&&<p>没有可核对的有效商品引用。</p>}
   {report.products.map(product=><article key={product.productId} style={{minWidth:0}}>
    <h4>{product.name}</h4><p>SKU：{product.sku} · 商品 ID：{product.productId}</p>
    <p>商品版本：包内 {product.packageRevision} · 当前 {product.currentRevision??'不可用'} · {versionLabels[product.status]}</p>
    <p>{product.detail}</p>
    <ul>{product.drafts.map(draft=><li key={draft.id}>稿件 {draft.id}：包内 {draft.packageRevision} · 当前 {draft.currentRevision??'不可用'} · {versionLabels[draft.status]}</li>)}</ul>
   </article>)}
   <h3>逐项检查</h3>
   <ul>{report.checks.map((check,index)=><li key={index}>
    {checkLabels[check.status]} · {check.code} · {check.message}{check.path&&<> · 文件：{check.path}</>}
   </li>)}</ul>
  </div>}
 </section>;
}
