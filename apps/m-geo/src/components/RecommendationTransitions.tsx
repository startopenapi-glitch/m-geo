import {useLayoutEffect,useRef,useState,type CSSProperties} from 'react';
import {call} from '../api';
import {evidenceLabels} from '../evidence';
import type {Brand,Observation,WorkspaceSnapshot} from '../types';
import type {Transition,TransitionEvidence,TransitionMetrics,TransitionPair,TransitionReport,TransitionSide} from '../transitionTypes';
import {Field,Panel} from './ui';

const text:CSSProperties={whiteSpace:'pre-wrap',overflowWrap:'anywhere'};
const wrap:CSSProperties={minWidth:0,maxWidth:'100%',overflowWrap:'anywhere'};
const platformNames:Record<string,string>={doubao:'豆包',yuanbao:'元宝',qianwen:'千问',baidu:'百度'};
const transitionNames:Record<Transition,string>={gained:'新增推荐',lost:'失去推荐',retained_yes:'保持推荐',retained_no:'保持未推荐',unknown_pair:'推荐尚未核对',baseline_only:'仅基线已采集',followup_only:'仅复测已采集',missing_both:'两期均未采集'};
const recommendationNames={yes:'明确推荐购买',no:'未推荐购买',unknown:'尚未核对'};
const accuracyNames={unknown:'尚未核对',correct:'核对正确',incorrect:'发现错误'};
const delta=(value:number|null)=>value===null?'无法计算':`${value>0?'+':''}${value.toFixed(1)}`;
type Props={brand:Brand;data:WorkspaceSnapshot;onBusyChange?:(busy:boolean)=>void};
type Dimension='product'|'question'|'platform';
function Pager({label,page,total,busy,onChange}:{label:string;page:number;total:number;busy:boolean;onChange:(page:number)=>void}) {
 const pages=Math.max(1,Math.ceil(total/20));
 return <nav aria-label={label} className="actions" style={{flexDirection:'row',margin:'14px 0'}}><button disabled={busy||page===0} onClick={()=>onChange(page-1)}>上一页</button><span>第 {page+1}/{pages} 页 · 共 {total} 条</span><button disabled={busy||page+1>=pages} onClick={()=>onChange(page+1)}>下一页</button></nav>;
}
function Metrics({metrics:m}:{metrics:TransitionMetrics}) {
 return <><p>全计划 · 计划单元 {m.total} · 基线样本 {m.baselineSamples}/{m.total} · 复测样本 {m.followupSamples}/{m.total}</p><p>两期均已采集 {m.paired}/{m.total} 对 · 两期推荐均已核对 {m.assessedPairs}/{m.paired} 对</p><p>新增推荐 {m.gained} · 失去推荐 {m.lost} · 保持推荐 {m.retainedYes} · 保持未推荐 {m.retainedNo} · 推荐尚未核对 {m.unknownPair}</p><p>仅基线已采集 {m.baselineOnly} · 仅复测已采集 {m.followupOnly} · 两期均未采集 {m.missingBoth}</p><p>已核对配对中的基线推荐 {m.baselineRecommended}/{m.assessedPairs} · 复测推荐 {m.followupRecommended}/{m.assessedPairs}</p><p>推荐率变化：{m.deltaPercentagePoints===null?'无法计算':`${delta(m.deltaPercentagePoints)} 个百分点`}（仅 {m.assessedPairs} 对两期推荐均已核对单元作为分母）</p></>;
}
function MetricCells({m}:{m:TransitionMetrics}) {
 return <><td>{m.total}</td><td>基线 {m.baselineSamples} · 复测 {m.followupSamples}</td><td>{m.paired}/{m.total}</td><td>{m.assessedPairs}/{m.paired}</td><td>新增推荐 {m.gained} · 失去推荐 {m.lost}</td><td>推荐 {m.retainedYes} · 未推荐 {m.retainedNo}</td><td>{m.unknownPair}</td><td>仅基线 {m.baselineOnly} · 仅复测 {m.followupOnly} · 均未采集 {m.missingBoth}</td><td>基线 {m.baselineRecommended}/{m.assessedPairs} · 复测 {m.followupRecommended}/{m.assessedPairs}</td><td>{delta(m.deltaPercentagePoints)}</td></>;
}
function Side({side}:{side:TransitionSide|null}) {
 if(!side)return <span>未采集</span>;
 return <div style={text}><p>{recommendationNames[side.recommendation]} · 推荐顺位 {side.rank??'未记录'}</p><p>评估版本 {side.assessmentRevision} · 事实 {accuracyNames[side.accuracy]}</p><p>提及 {side.mentioned?'是':'否'} · 引用 {side.cited?'有':'无'}</p><small>观察 ID：{side.observationId}</small></div>;
}
function Original({title,observation:o}:{title:string;observation:Observation|null}) {
 return <div style={wrap}><h4>{title}</h4>{o?<><p style={text}>观察 ID：{o.id}</p><p>当前评估版本：{o.assessmentRevision??1}</p><pre className="preview" style={text}>{o.answer}</pre><p>购买推荐：{recommendationNames[o.assessment?.recommendation??'unknown']} · 推荐顺位：{o.assessment?.rank??'未记录'}</p><p>事实准确性：{accuracyNames[o.accuracy]} · 明确提及：{o.mentioned?'是':'否'}</p><p style={text}>引用网址：{o.citedUrl||'无引用网址'}</p><h4>核对说明</h4><p style={text}>{o.evidenceNote||'未记录'}</p><h4>竞品与推荐理由</h4><p style={text}>{o.assessment?.competitors||'未记录'}</p>{!!o.assessmentHistory?.length&&<details><summary>历史评估（只读）</summary><pre className="preview" style={text}>{JSON.stringify(o.assessmentHistory,null,2)}</pre></details>}</>:<p>未采集</p>}</div>;
}
/** A brand change unmounts the old operation owner; snapshot changes only invalidate its response. */
export default function RecommendationTransitions(props:Props) {return <TransitionView key={props.brand.id} {...props}/>}
function TransitionView({brand,data,onBusyChange}:Props) {
 const [campaignId,setCampaign]=useState(''),[report,setReport]=useState<TransitionReport|null>(null),[evidence,setEvidence]=useState<TransitionEvidence|null>(null);
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[feedback,setFeedback]=useState('');
 const [dimension,setDimension]=useState<Dimension>('product'),[aggregatePage,setAggregatePage]=useState(0),[pairPage,setPairPage]=useState(0);
 const [productFilter,setProductFilter]=useState(''),[platformFilter,setPlatformFilter]=useState(''),[transitionFilter,setTransitionFilter]=useState('');
 const alive=useRef(false),generation=useRef(0),operation=useRef(0),locked=useRef(false);
 const busyChange=useRef(onBusyChange);busyChange.current=onBusyChange;
 useLayoutEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++;if(locked.current){locked.current=false;busyChange.current?.(false)}}},[]);
 function resetView(){setEvidence(null);setDimension('product');setAggregatePage(0);setPairPage(0);setProductFilter('');setPlatformFilter('');setTransitionFilter('')}
 useLayoutEffect(()=>{
  generation.current++;setReport(null);setError('');setFeedback('');resetView();
  if(campaignId&&!(data.campaigns??[]).some(c=>c.id===campaignId))setCampaign('');
  // A refreshed snapshot invalidates results, but the native operation still owns the lock.
 },[brand.id,data,campaignId]);
 useLayoutEffect(()=>{setEvidence(null)},[dimension,aggregatePage,pairPage,productFilter,platformFilter,transitionFilter]);
 const current=(version:number)=>alive.current&&version===generation.current;
 function begin(){if(locked.current||!alive.current)return null;locked.current=true;const token=++operation.current;const version=++generation.current;busyChange.current?.(true);setBusy(true);setError('');setFeedback('');setEvidence(null);return {token,version}}
 function finish(token:number){if(alive.current&&operation.current===token&&locked.current){locked.current=false;setBusy(false);busyChange.current?.(false)}}
 function fail(version:number,cause:unknown){if(current(version)){setReport(null);resetView();setError(cause instanceof Error?cause.message:'操作未完成，请重新计算')}}
 async function calculate(){if(!campaignId)return;const ticket=begin();if(!ticket)return;setReport(null);resetView();try{const result=await call<TransitionReport>('campaign.transitions',{brandId:brand.id,campaignId});if(current(ticket.version))setReport(result)}catch(cause){fail(ticket.version,cause)}finally{finish(ticket.token)}}
 async function lookup(row:TransitionPair){if(!report)return;const ticket=begin();if(!ticket)return;try{const result=await call<TransitionEvidence>('campaign.transitionEvidence',{brandId:brand.id,campaignId,reportFingerprint:report.reportFingerprint,questionId:row.questionId,platform:row.platform,repetition:row.repetition});if(current(ticket.version))setEvidence(result)}catch(cause){fail(ticket.version,cause)}finally{finish(ticket.token)}}
 async function exportReport(){if(!report)return;const ticket=begin();if(!ticket)return;try{const result=await call<{saved:true}|null>('file.exportTransitions',{brandId:brand.id,campaignId,reportFingerprint:report.reportFingerprint});if(current(ticket.version))setFeedback(result===null?'已取消保存推荐变化 ZIP':'推荐变化 ZIP 已保存到本机')}catch(cause){fail(ticket.version,cause)}finally{finish(ticket.token)}}
 function change(action:()=>void){if(!locked.current)action()}
 const aggregates=report?(dimension==='product'?report.products.map(p=>({key:p.productId,label:`${p.name} · SKU ${p.sku} · 商品 ID ${p.productId}`,metrics:p.metrics})):dimension==='question'?report.questions.map(q=>({key:`${q.productId}/${q.questionId}`,label:`${q.questionText} · 商品 ID ${q.productId} · 问题 ID ${q.questionId}`,metrics:q.metrics})):report.platforms.map(p=>({key:p.platform,label:platformNames[p.platform],metrics:p.metrics}))):[];
 const pairs=(report?.rows??[]).filter(row=>(!productFilter||row.productId===productFilter)&&(!platformFilter||row.platform===platformFilter)&&(!transitionFilter||row.transition===transitionFilter));
 const currentAggregate=Math.min(aggregatePage,Math.max(0,Math.ceil(aggregates.length/20)-1)),currentPair=Math.min(pairPage,Math.max(0,Math.ceil(pairs.length/20)-1));
 useLayoutEffect(()=>{setAggregatePage(p=>Math.min(p,Math.max(0,Math.ceil(aggregates.length/20)-1)));setPairPage(p=>Math.min(p,Math.max(0,Math.ceil(pairs.length/20)-1)))},[aggregates.length,pairs.length]);
 const product=(id:string)=>report?.products.find(p=>p.productId===id);
 const question=(row:TransitionPair)=>report?.questions.find(q=>q.productId===row.productId&&q.questionId===row.questionId)?.questionText??row.questionId;
 return <Panel title="逐商品与问题推荐变化" description="从本机完整固定计划配对计算；按商品、问题和平台拆开查看新增与失去推荐。"><div style={wrap} aria-busy={busy}>
 <p className="notice">只读本地计算；不会自动查询平台、核对评估或保存原始回答。</p>
 {!(data.campaigns??[]).length?<p>先创建固定测试计划。</p>:<Field label="推荐变化固定计划"><select disabled={busy} value={campaignId} onChange={e=>change(()=>setCampaign(e.target.value))}><option value="">选择固定计划</option>{(data.campaigns??[]).map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></Field>}
 <div className="actions"><button disabled={busy||!campaignId} onClick={()=>void calculate()}>计算推荐变化</button><button disabled={busy||!report} onClick={()=>void exportReport()}>导出全计划变化 ZIP</button></div>
 <p className="inline-note">导出包含全计划经营信息：品牌及商品标识、当前名称/SKU、冻结问题与测试条件，并非匿名；不包含原始回答、核对说明、竞品或引用网址。</p>
 {busy&&<p role="status">正在读取或保存本地推荐变化资料…</p>}{error&&<p role="alert" style={text}>{error}</p>}{feedback&&<p role="status">{feedback}</p>}
 {report&&<><p>{evidenceLabels[report.evidenceMode]}</p><p className="notice">商品名称与 SKU 是当前标签，不代表历史名称；问题采用计划冻结原文。商品与问题 ID 用于区分同文问题。</p><p style={text}>冻结条件：{report.campaign.modelLabel} · {report.campaign.searchMode} · {report.campaign.context} · 每题每平台 {report.campaign.repetitions} 轮</p><h3>全计划汇总（不随明细筛选变化）</h3><Metrics metrics={report.summary}/><p style={text}>{report.caveat}</p>
 <Field label="推荐变化汇总维度"><select disabled={busy} value={dimension} onChange={e=>change(()=>{setDimension(e.target.value as Dimension);setAggregatePage(0)})}><option value="product">商品</option><option value="question">问题</option><option value="platform">平台</option></select></Field>
 <p className="inline-note">汇总每页 20 条；正负变化分别显示。推荐率分母仅含两期推荐均为是/否的配对；未知与未采集分别保留。</p>
 <Pager label="推荐变化汇总分页" page={currentAggregate} total={aggregates.length} busy={busy} onChange={page=>change(()=>setAggregatePage(page))}/>
 <div className="attribution-table-wrap" style={wrap} tabIndex={0} role="region" aria-label="推荐变化汇总可横向滚动表格"><table className="attribution-table" aria-label="推荐变化汇总"><thead><tr>{['商品 / 冻结问题 / 平台','计划单元','已采集样本','已配对 / 计划','已核对 / 已配对','新增与失去推荐','保持状态','未知配对','未配对与缺失','推荐 / 已核对配对','变化（百分点）'].map(label=><th scope="col" key={label}>{label}</th>)}</tr></thead><tbody>{aggregates.slice(currentAggregate*20,currentAggregate*20+20).map(group=><tr key={group.key}><th scope="row" style={text}>{group.label}</th><MetricCells m={group.metrics}/></tr>)}</tbody></table></div>
 <h3>配对明细（筛选仅作用于本表）</h3><div className="form-grid"><Field label="明细商品"><select disabled={busy} value={productFilter} onChange={e=>change(()=>{setProductFilter(e.target.value);setPairPage(0)})}><option value="">全部商品</option>{report.products.map(p=><option key={p.productId} value={p.productId}>{p.name} · {p.sku}</option>)}</select></Field><Field label="明细平台"><select disabled={busy} value={platformFilter} onChange={e=>change(()=>{setPlatformFilter(e.target.value);setPairPage(0)})}><option value="">全部平台</option>{report.platforms.map(p=><option key={p.platform} value={p.platform}>{platformNames[p.platform]}</option>)}</select></Field><Field label="明细变化类型"><select disabled={busy} value={transitionFilter} onChange={e=>change(()=>{setTransitionFilter(e.target.value);setPairPage(0)})}><option value="">全部变化</option>{Object.entries(transitionNames).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Field></div>
 <Pager label="推荐变化明细分页" page={currentPair} total={pairs.length} busy={busy} onChange={page=>change(()=>setPairPage(page))}/>
 {!pairs.length?<p>当前筛选没有配对单元。</p>:<div className="attribution-table-wrap" style={wrap} tabIndex={0} role="region" aria-label="推荐变化明细可横向滚动表格"><table className="attribution-table" aria-label="推荐变化配对明细"><thead><tr>{['当前商品 / SKU','冻结问题原文','平台 / 轮次','变化类型','基线元数据','复测元数据','本机原始证据'].map(label=><th scope="col" key={label}>{label}</th>)}</tr></thead><tbody>{pairs.slice(currentPair*20,currentPair*20+20).map(row=><tr key={`${row.productId}/${row.questionId}/${row.platform}/${row.repetition}`}><th scope="row" style={text}>{product(row.productId)?.name} · {product(row.productId)?.sku}<p>商品 ID：{row.productId}</p></th><td style={text}>{question(row)}<p>问题 ID：{row.questionId}</p></td><td>{platformNames[row.platform]} · 第 {row.repetition} 轮</td><td>{transitionNames[row.transition]}</td><td><Side side={row.baseline}/></td><td><Side side={row.followup}/></td><td><button disabled={busy} onClick={()=>void lookup(row)}>查看两期原始证据</button></td></tr>)}</tbody></table></div>}
 {evidence&&<section role="region" aria-label="两期原始证据" style={wrap}><h3>两期原始证据（本机只读）</h3><p style={text}>{question(evidence.row)} · {platformNames[evidence.row.platform]} · 第 {evidence.row.repetition} 轮</p><p className="inline-note">原始回答与核对详情仅供本机查看，不包含在变化 ZIP 中。</p><div className="content-grid"><Original title="基线原始证据" observation={evidence.baseline}/><Original title="复测原始证据" observation={evidence.followup}/></div></section>}
 </>}
 </div></Panel>;
}
