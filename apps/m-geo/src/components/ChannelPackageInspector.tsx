import {useLayoutEffect,useRef,useState,type CSSProperties} from 'react';
import {call} from '../api';
import type {ChannelInspectionPage,ChannelInspectionReport,ChannelInspectionSelection} from '../channelInspectionTypes';
import {Panel} from './ui';

const wrap:CSSProperties={minWidth:0,maxWidth:'100%',overflowWrap:'anywhere'};
const text:CSSProperties={...wrap,whiteSpace:'pre-wrap'};
const button:CSSProperties={maxWidth:'100%',whiteSpace:'normal'};
const operationNames={comparison:'清单比较',simulation:'模拟回执','receipt-check':'回执核对'};
const changeNames={added:'本地选择新增',removed:'从本地选择移除',changed:'有变化',unchanged:'无变化'};
const statusNames={accepted:'模拟接受',rejected:'模拟拒绝',unknown:'未知',missing:'缺失'};
const checkNames={frozen_evidence:'冻结证据',report:'历史报告',notice:'本地操作声明'};
const fieldNames:Record<string,string>={sku:'商品编码（SKU）',productRevision:'商品版本',draftId:'说明草稿',draftRevision:'草稿版本',localStatus:'历史本地就绪情况',validity:'资料有效期',id:'渠道商品标识',title:'商品标题',description:'商品说明',link:'购买链接',imageLink:'商品图片',price:'价格',currency:'币种',availability:'库存状态'};
function Summary({report}:{report:ChannelInspectionReport}) {
  if(report.operation==='comparison') {
    const summary=report.summary;
    return <><p>本地新增 {summary.added} · 本地移除 {summary.removed} · 变化 {summary.changed} · 无变化 {summary.unchanged}</p><p className="inline-note">移除仅表示不在这次历史本地选择中，不代表平台删除。</p></>;
  }
  const summary=report.summary;
  return <p>模拟接受 {summary.accepted} · 模拟拒绝 {summary.rejected} · 未知 {summary.unknown} · 缺失 {summary.missing}</p>;
}
function Rows({page,operation}:{page:ChannelInspectionPage;operation:ChannelInspectionReport['operation']}) {
  const comparison=operation==='comparison';
  return <div className="attribution-table-wrap" style={wrap} tabIndex={0} role="region" aria-label="历史通道操作明细可横向滚动表格">
    <table className="attribution-table" aria-label="历史通道操作明细">
      <thead><tr>{(comparison?['SKU','历史变化','变化字段']:['SKU','行摘要（SHA256）','模拟状态','代码','说明']).map(label=><th scope="col" key={label}>{label}</th>)}</tr></thead>
      <tbody>{page.rows.slice(0,20).map((row,index)=><tr key={`${page.page}-${index}`}>
        <th scope="row" style={text}>{row.sku}</th>
        {comparison&&'change' in row?<><td style={text}>{changeNames[row.change]}</td><td style={text}>{row.fields.length?row.fields.map(field=>fieldNames[field]??field).join('、'):'—'}</td></>:!comparison&&'status' in row?<><td style={text}>{row.rowHash}</td><td>{statusNames[row.status]}</td><td style={text}>{row.code}</td><td style={text}>{row.message}</td></>:null}
      </tr>)}</tbody>
    </table>
  </div>;
}
type InspectorProps={brandId:string;onBusyChange?:(busy:boolean)=>void};
/** Snapshot refreshes retain history; the host locks navigation during native operations. */
export default function ChannelPackageInspector({brandId,onBusyChange}:InspectorProps) {
  return <InspectionView key={brandId} brandId={brandId} onBusyChange={onBusyChange}/>;
}
function InspectionView({brandId,onBusyChange}:InspectorProps) {
  const [selection,setSelection]=useState<ChannelInspectionSelection|null>(null);
  const [page,setPage]=useState<ChannelInspectionPage|null>(null);
  const [cursor,setCursor]=useState(0);
  const [busy,setBusy]=useState(false),[feedback,setFeedback]=useState(''),[error,setError]=useState('');
  const alive=useRef(false),locked=useRef(false),generation=useRef(0);
  const busyChange=useRef(onBusyChange);busyChange.current=onBusyChange;
  useLayoutEffect(()=>{alive.current=true;return()=>{
    alive.current=false;generation.current++;
    if(locked.current){locked.current=false;busyChange.current?.(false)}
  }},[]);
  const current=(version:number)=>alive.current&&version===generation.current;
  function begin() {
    if(locked.current||!alive.current)return null;
    locked.current=true;busyChange.current?.(true);setBusy(true);setFeedback('');setError('');
    return ++generation.current;
  }
  function finish(version:number) {if(current(version)){locked.current=false;busyChange.current?.(false);setBusy(false)}}
  function fail(version:number,cause:unknown) {if(current(version))setError(cause instanceof Error?cause.message:'操作未完成，请重试')}
  async function readPage(ticket:ChannelInspectionSelection,target:number,version:number) {
    const result=await call<ChannelInspectionPage>('file.readChannelInspectionPage',{brandId,inspectionId:ticket.inspectionId,page:target});
    if(current(version)&&result.inspectionId===ticket.inspectionId&&result.page===target)setPage(result);
  }
  async function select() {
    const version=begin();if(version===null)return;
    setSelection(null);setPage(null);setCursor(0);
    try {
      const result=await call<ChannelInspectionSelection|null>('file.inspectChannelReport',{brandId});
      if(!current(version))return;
      if(result===null){setFeedback('已取消选择历史通道 ZIP');return}
      setSelection(result);
      await readPage(result,0,version);
    }catch(cause){fail(version,cause)}finally{finish(version)}
  }
  async function navigate(target:number) {
    if(!selection||target<0||target>=Math.max(1,Math.ceil(selection.report.counts.rows/20)))return;
    const version=begin();if(version===null)return;
    setPage(null);setCursor(target);
    try {await readPage(selection,target,version)}catch(cause){fail(version,cause)}finally{finish(version)}
  }
  async function exportReport() {
    if(!selection)return;
    const version=begin();if(version===null)return;
    try {
      const result=await call<{saved:true}|null>('file.exportChannelInspection',{brandId,inspectionId:selection.inspectionId,reportFingerprint:selection.report.reportFingerprint});
      if(current(version))setFeedback(result===null?'已取消保存通道复核结论':'通道复核结论 ZIP 已保存');
    }catch(cause){fail(version,cause)}finally{finish(version)}
  }
  const report=selection?.report,total=report?.counts.rows??0,pages=Math.max(1,Math.ceil(total/20));
  return <Panel title="历史商品通道操作 ZIP 复核" description="从冻结的本地文件重新计算历史操作；无需当前工作区保留原商品或已审核草稿。">
    <div style={wrap} aria-busy={busy}>
      <p className="notice">支持比较、模拟回执、回执核对操作 ZIP；不支持原始 Feed/XML 草稿 ZIP。</p>
      <div className="actions" style={{margin:'14px 0'}}>
        <button style={button} disabled={busy} onClick={()=>void select()}>选择历史通道 ZIP</button>
        <button style={button} disabled={busy||!selection} onClick={()=>void exportReport()}>导出通道复核结论</button>
      </div>
      <p className="inline-note">导出为汇总操作报告 ZIP，仅含 inspection.json 与 report.md，不含原始文件、SKU 或逐行回执消息；包含所选文件名和经营汇总，仍应谨慎分享。</p>
      {busy&&<p role="status">正在读取或保存本地历史通道资料…</p>}
      {feedback&&<p role="status" style={text}>{feedback}</p>}
      {error&&<p role="alert" style={text}>{error}</p>}
      {!selection&&!busy&&!feedback&&!error&&<p>尚未选择历史通道 ZIP。</p>}
      {report&&<div style={wrap}>
        <h3>{report.status==='passed'?'一致性检查通过':'一致性检查未通过'}</h3>
        <dl className="info-list" style={{margin:'16px 0'}}>
          <dt>历史文件</dt><dd style={text}>{report.source.fileName}</dd>
          <dt>ZIP SHA256</dt><dd style={text}>{report.source.sha256}</dd>
          <dt>历史操作</dt><dd>{operationNames[report.operation]}（{report.operation}）</dd>
          <dt>格式</dt><dd style={text}>{report.profile}</dd>
          <dt>文件大小</dt><dd>压缩 {report.source.compressedBytes} 字节 · 展开 {report.source.uncompressedBytes} 字节 · {report.source.fileCount} 个文件</dd>
          <dt>品牌 ID</dt><dd style={text}>{report.brandId}</dd>
          <dt>历史清单标识</dt><dd style={text}>{report.packageId}</dd>
          {report.baselinePackageId!==null&&<><dt>基线清单标识</dt><dd style={text}>{report.baselinePackageId}</dd></>}
          <dt>复核指纹</dt><dd style={text}>{report.reportFingerprint}</dd>
        </dl>
        <p>历史选择 {report.counts.selected} 件 · 基线选择 {report.counts.baselineSelected} 件 · 明细 {report.counts.rows} 行</p>
        <p>历史本地就绪 {report.counts.ready} 件 · 历史阻断 {report.counts.blocked} 件</p>
        <p className="inline-note">就绪与阻断沿用冻结清单中的历史状态，不是对当前资料有效性的重新认证。</p>
        <Summary report={report}/>
        <ul>{report.checks.map(check=><li key={check.code} style={text}>{checkNames[check.code]} · {check.status==='passed'?'通过':'未通过'} · {check.message}</li>)}</ul>
        <p style={text}>{report.caveat}</p>
        <h3>本机历史明细（只读）</h3>
        <p className="inline-note">每页最多 20 行。SKU、变化字段与回执消息仅在本机明细展示，不包含在汇总操作报告中。</p>
        <nav aria-label="历史通道明细分页" className="actions" style={{flexDirection:'row',margin:'14px 0'}}>
          <button style={button} disabled={busy||cursor===0} onClick={()=>void navigate(cursor-1)}>上一页</button>
          <span>第 {cursor+1}/{pages} 页 · 共 {total} 条</span>
          <button style={{...button,marginTop:0}} disabled={busy||cursor+1>=pages} onClick={()=>void navigate(cursor+1)}>下一页</button>
        </nav>
        {!page&&!busy&&<button style={button} onClick={()=>void navigate(cursor)}>重试读取历史明细</button>}
        {page?.total===0&&<p>历史包没有操作明细。</p>}
        {page&&<Rows page={page} operation={report.operation}/>}
      </div>}
    </div>
  </Panel>;
}
