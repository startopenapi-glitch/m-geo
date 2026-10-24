import {useLayoutEffect,useRef,useState,type CSSProperties} from 'react';
import {call} from '../api';
import type {AttributionMetrics,Currency} from '../attributionTypes';
import type {AttributionInspectionKind,AttributionInspectionPage,AttributionInspectionSelection} from '../attributionInspectionTypes';
import {evidenceLabel as evidenceLabels,metricCells,money,reasons as reasonLabels,sourceLabel} from './AttributionResults';
import {Field,Panel} from './ui';

const wrap:CSSProperties={minWidth:0,maxWidth:'100%',overflowWrap:'anywhere'};
const text:CSSProperties={...wrap,whiteSpace:'pre-wrap'};
const button:CSSProperties={maxWidth:'100%',whiteSpace:'normal'};
function MetricTable({title,rows,currency}:{title:string;rows:{label:string;metrics:AttributionMetrics}[];currency:Currency}) {
  return <section aria-label={`${title}统计`}>
    <h3>{title}</h3>
    {!rows.length?<p className="inline-note">当前区间无记录。</p>:<div className="attribution-table-wrap" style={wrap} tabIndex={0} role="region" aria-label={`${title}可横向滚动表格`}>
      <table className="attribution-table" aria-label={title}>
        <thead><tr><th scope="col">分组</th>{metricCells(rows[0].metrics,currency).map(([label])=><th scope="col" key={label}>{label}</th>)}</tr></thead>
        <tbody>{rows.map((row,index)=><tr key={index}><th scope="row" style={text}>{row.label}</th>{metricCells(row.metrics,currency).map(([label,value])=><td key={label}>{value}</td>)}</tr>)}</tbody>
      </table>
    </div>}
  </section>;
}
function Orders({page,currency}:{page:Extract<AttributionInspectionPage,{kind:'orders'}>;currency:Currency}) {
  return <div className="attribution-table-wrap" style={wrap} tabIndex={0} role="region" aria-label="历史订单行可横向滚动表格">
    <table className="attribution-table" aria-label="历史订单行">
      <thead><tr>{['订单行','SKU','支付／取消时间（UTC）','状态','原始金额','退款','净收入','来源','来源证据','匹配访问','匹配说明','访问在报告区间内'].map(label=><th scope="col" key={label}>{label}</th>)}</tr></thead>
      <tbody>{page.rows.slice(0,20).map((order,index)=><tr key={index}>
        <th scope="row" style={text}>{order.orderId}</th><td style={text}>{order.sku}</td><td>{order.occurredAt}</td>
        <td>{order.status==='paid'?'已支付':'已取消'}</td><td>{money(order.amountMinor,currency)}</td><td>{money(order.refundMinor,currency)}</td><td>{money(order.netMinor,currency)}</td>
        <td>{sourceLabel[order.source]}</td><td>{evidenceLabels[order.sourceEvidence]}</td><td style={text}>{order.visitId??'—'}</td><td>{reasonLabels[order.reason]??order.reason}</td><td>{order.visitInPeriod?'是':'否'}</td>
      </tr>)}</tbody>
    </table>
  </div>;
}

/** A brand change destroys the ephemeral history, including pending native responses. */
export default function AttributionPackageInspector({brandId}:{brandId:string}) {
  return <InspectionView key={brandId} brandId={brandId}/>;
}
function InspectionView({brandId}:{brandId:string}) {
  const [selection,setSelection]=useState<AttributionInspectionSelection|null>(null);
  const [page,setPage]=useState<AttributionInspectionPage|null>(null);
  const [kind,setKind]=useState<AttributionInspectionKind>('orders');
  const [cursors,setCursors]=useState({orders:0,products:0});
  const [busy,setBusy]=useState(false),[feedback,setFeedback]=useState(''),[error,setError]=useState('');
  const alive=useRef(false),locked=useRef(false),generation=useRef(0);
  useLayoutEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
  const current=(version:number)=>alive.current&&version===generation.current;
  function begin() {
    if(locked.current||!alive.current)return null;
    locked.current=true;setBusy(true);setFeedback('');setError('');
    return ++generation.current;
  }
  function finish(version:number) {if(current(version)){locked.current=false;setBusy(false)}}
  function fail(version:number,e:unknown) {if(current(version))setError(e instanceof Error?e.message:'操作未完成，请重试')}
  async function readPage(ticket:AttributionInspectionSelection,targetKind:AttributionInspectionKind,target:number,version:number) {
    const result=await call<AttributionInspectionPage>('file.readAttributionInspectionPage',{brandId,inspectionId:ticket.inspectionId,kind:targetKind,page:target});
    if(current(version)&&result.inspectionId===ticket.inspectionId&&result.kind===targetKind&&result.page===target)setPage(result);
  }
  async function select() {
    const version=begin();if(version===null)return;
    setSelection(null);setPage(null);setKind('orders');setCursors({orders:0,products:0});
    try {
      const result=await call<AttributionInspectionSelection|null>('file.inspectAttributionReport',{brandId});
      if(!current(version))return;
      if(result===null){setFeedback('已取消选择历史归因 ZIP');return}
      setSelection(result);
      await readPage(result,'orders',0,version);
    }catch(e){fail(version,e)}finally{finish(version)}
  }
  async function navigate(targetKind:AttributionInspectionKind,target:number) {
    if(!selection||target<0)return;
    const total=targetKind==='orders'?selection.report.counts.orders:selection.report.counts.products;
    if(target>=Math.max(1,Math.ceil(total/20)))return;
    const version=begin();if(version===null)return;
    setPage(null);setKind(targetKind);setCursors(previous=>({...previous,[targetKind]:target}));
    try {await readPage(selection,targetKind,target,version)}catch(e){fail(version,e)}finally{finish(version)}
  }
  async function exportReport() {
    if(!selection)return;
    const version=begin();if(version===null)return;
    try {
      const result=await call<{saved:true}|null>('file.exportAttributionInspection',{brandId,inspectionId:selection.inspectionId,reportFingerprint:selection.report.reportFingerprint});
      if(current(version))setFeedback(result===null?'已取消保存归因核验汇总':'归因核验汇总 ZIP 已保存');
    }catch(e){fail(version,e)}finally{finish(version)}
  }
  const report=selection?.report,cursor=cursors[kind];
  const total=page?.total??(kind==='orders'?report?.counts.orders:report?.counts.products)??0;
  const pages=Math.max(1,Math.ceil(total/20));
  return <Panel title="历史归因 ZIP 核验（只读）" description="重新打开已导出的冻结证据包；无需当前数据库保留原商品或数据集。">
    <div style={wrap} aria-busy={busy}>
      <p className="notice">仅检查历史文件与声明来源的一致性。记录数据表示导入者声明，未经平台核实；不证明 AI 引荐、因果销量增量或推荐排名。哈希不是签名。</p>
      <div className="actions" style={{margin:'14px 0'}}>
        <button style={button} disabled={busy} onClick={()=>void select()}>选择历史归因 ZIP</button>
        <button style={button} disabled={busy||!selection} onClick={()=>void exportReport()}>导出归因核验汇总 ZIP</button>
      </div>
      <p className="inline-note">核验汇总 ZIP 仅含 inspection.json 与 report.md，包含汇总金额、退款、净收入、转化及来源／周统计；不含商品名称、SKU、订单／访问／退款／会话标识或逐行记录。原始九文件证据包须单独保留；汇总经营数据仍应谨慎分享。</p>
      {busy&&<p role="status">正在读取或保存本地历史归因资料…</p>}
      {feedback&&<p role="status" style={text}>{feedback}</p>}
      {error&&<p role="alert" style={text}>{error}</p>}
      {!selection&&!busy&&!feedback&&!error&&<p>尚未选择历史归因 ZIP。</p>}
      {report&&<div style={wrap}>
        <h3>{report.status==='passed'?'一致性检查通过':'一致性检查未通过'}</h3>
        <p>{report.evidenceLabel}</p>
        <dl className="info-list" style={{margin:'16px 0'}}>
          <dt>历史文件</dt><dd style={text}>{report.source.fileName}</dd>
          <dt>ZIP SHA256</dt><dd>{report.source.sha256}</dd>
          <dt>格式</dt><dd>{report.profile}</dd>
          <dt>文件大小</dt><dd>压缩 {report.source.compressedBytes} 字节 · 展开 {report.source.uncompressedBytes} 字节 · {report.source.fileCount} 个文件</dd>
          <dt>品牌 ID</dt><dd>{report.brandId}</dd><dt>数据集 ID</dt><dd>{report.datasetId}</dd>
          <dt>内容指纹</dt><dd>{report.contentHash}</dd><dt>核验指纹</dt><dd>{report.reportFingerprint}</dd>
        </dl>
        <p>币种：{report.currency} · 覆盖区间（UTC）：[{report.coverage.from}, {report.coverage.to})</p>
        <p>报告区间（UTC）：[{report.period.from}, {report.period.to}) · 回溯 {report.period.lookbackDays} 天</p>
        <p>退款统计截至 {report.period.refundAsOf}（UTC，不含）；退款归入原支付订单队列，包含报告结束后的已知退款。</p>
        <p>访问 {report.counts.visits} · 订单行 {report.counts.orders} · 退款 {report.counts.refunds} · 商品 {report.counts.products}</p>
        <ul>{report.checks.map(check=><li key={check.code}>{check.code} · {check.status==='passed'?'通过':'未通过'} · {check.message}</li>)}</ul>
        <p style={text}>{report.caveat}</p>
        <p className="notice">支付订单行不是客户数或结账次数；全额退款仍保留历史支付转化。会话按（会话标识、SKU）去重，来源与周分组的会话数不可相加。周转化按所选区间内观察到的访问队列计算，收入按支付周计算。尚不支持现金流、利润或 ROI。</p>
        <section aria-label="历史汇总指标"><h3>历史汇总指标</h3>
          <dl className="attribution-metrics">{metricCells(report.summary,report.currency).map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
        </section>
        <MetricTable title="历史按来源" rows={report.bySource.map(row=>({label:sourceLabel[row.source],metrics:row}))} currency={report.currency}/>
        <MetricTable title="历史按 UTC 周一" rows={report.weekly.map(row=>({label:row.weekStart,metrics:row}))} currency={report.currency}/>
        <h3>本机历史明细（只读）</h3>
        <p className="inline-note">仅按需读取当前页，每页最多 20 行。冻结商品名称和 SKU、订单行及匹配访问标识仅显示于此，不包含在核验汇总导出内。取消行的原始金额不计入支付与净收入。</p>
        <Field label="历史明细类型"><select disabled={busy} value={kind} onChange={event=>{const value=event.target.value as AttributionInspectionKind;void navigate(value,cursors[value])}}><option value="orders">订单行</option><option value="products">商品快照统计</option></select></Field>
        <nav aria-label="历史归因明细分页" className="actions" style={{flexDirection:'row',margin:'14px 0'}}>
          <button style={button} disabled={busy||cursor===0} onClick={()=>void navigate(kind,cursor-1)}>上一页</button>
          <span>第 {cursor+1}/{pages} 页 · 共 {total} 条</span>
          <button style={{...button,marginTop:0}} disabled={busy||cursor+1>=pages} onClick={()=>void navigate(kind,cursor+1)}>下一页</button>
        </nav>
        {!page&&!busy&&<button style={button} onClick={()=>void navigate(kind,cursor)}>重试读取历史明细</button>}
        {page?.total===0&&<p>历史包没有此类明细。</p>}
        {page?.kind==='orders'&&<Orders page={page} currency={report.currency}/>}
        {page?.kind==='products'&&<MetricTable title="历史商品快照统计" rows={page.rows.slice(0,20).map(row=>({label:`${row.name} · ${row.sku}`,metrics:row}))} currency={report.currency}/>}
      </div>}
    </div>
  </Panel>;
}
