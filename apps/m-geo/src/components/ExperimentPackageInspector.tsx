import {useLayoutEffect, useRef, useState, type CSSProperties} from 'react';
import {call} from '../api';
import type {ExperimentInspectionPage, ExperimentInspectionSelection, InspectedObservation, InspectionAnnotation} from '../experimentInspectionTypes';
import {Panel} from './ui';

const platformNames={doubao:'豆包',yuanbao:'元宝',qianwen:'千问',baidu:'百度'};
const wrap:CSSProperties={minWidth:0,maxWidth:'100%',overflowWrap:'anywhere'};
const rawText:CSSProperties={...wrap,whiteSpace:'pre-wrap'};
const buttonStyle:CSSProperties={maxWidth:'100%',whiteSpace:'normal'};

function Annotation({value}:{value:InspectionAnnotation}) {
  const assessment=value.assessment;
  return <div style={rawText}>
    <p>提及：{value.mentioned?'是':'否'} · 事实：{value.accuracy==='unknown'?'尚未核对':value.accuracy==='correct'?'正确':'有误'}</p>
    <p>购买推荐：{assessment.recommendation==='yes'?'是':assessment.recommendation==='no'?'否':'未核对'} · 推荐顺位：{assessment.rank??'未记录'}</p>
    <p>引用网址：<span>{value.citedUrl||'未记录'}</span></p>
    <p>竞品说明：<span>{assessment.competitors||'未记录'}</span></p>
    <p>核对说明：<span>{value.evidenceNote||'未记录'}</span></p>
  </div>;
}
function Sample({observation:o}:{observation:InspectedObservation}) {
  const history=o.assessmentHistory??[];
  return <details className="observation" style={wrap}>
    <summary>{platformNames[o.platform]} · {o.phase==='baseline'?'基线':'复测'} · {o.testedAt} · 第 {o.assessment.repetition} 轮</summary>
    <p style={rawText}>{o.querySnapshot}</p>
    <p style={rawText}>模型：{o.modelLabel} · 搜索模式：{o.searchMode}</p>
    <p style={rawText}>冻结测试条件：{o.assessment.context}</p>
    <p style={rawText}>观察 ID：{o.id} · 问题 ID：{o.questionId} · 商品 ID：{o.productId}</p>
    <pre className="preview" style={rawText} tabIndex={0} aria-label="历史原始回答">{o.answer}</pre>
    <h3>当前评估 v{o.assessmentRevision??1} · 历史版本 {history.length} 次</h3>
    <Annotation value={o}/>
    {history.length===0?<p>未保存更正历史（初始评估 v1）。</p>:<details>
      <summary>查看 {history.length} 次历史评估（只读）</summary>
      {history.map(h=><section key={h.revision} aria-label={`历史评估 v${h.revision}`} style={wrap}>
        <h3>历史评估 v{h.revision}</h3><p style={rawText}>被更正时间：{h.supersededAt}</p><Annotation value={h}/>
      </section>)}
    </details>}
  </details>;
}

/** A brand change destroys the prior view immediately, including pending-request feedback. */
export default function ExperimentPackageInspector({brandId}:{brandId:string}) {
  return <InspectionView key={brandId} brandId={brandId}/>;
}
function InspectionView({brandId}:{brandId:string}) {
  const [selection,setSelection]=useState<ExperimentInspectionSelection|null>(null);
  const [page,setPage]=useState<ExperimentInspectionPage|null>(null);
  const [cursor,setCursor]=useState(0), [busy,setBusy]=useState(false);
  const [feedback,setFeedback]=useState(''), [error,setError]=useState('');
  const alive=useRef(false), locked=useRef(false), generation=useRef(0);
  useLayoutEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
  const current=(version:number)=>alive.current&&version===generation.current;
  function begin() {
    if(locked.current||!alive.current)return null;
    locked.current=true;setBusy(true);setFeedback('');setError('');
    return ++generation.current;
  }
  function finish(version:number) {if(current(version)){locked.current=false;setBusy(false)}}
  function fail(version:number,e:unknown) {if(current(version))setError(e instanceof Error?e.message:'操作未完成，请重试')}
  async function readPage(ticket:ExperimentInspectionSelection,target:number,version:number) {
    const result=await call<ExperimentInspectionPage>('file.readExperimentInspectionPage',{brandId,inspectionId:ticket.inspectionId,page:target});
    if(current(version))setPage(result);
  }
  async function select() {
    const version=begin();if(version===null)return;
    setSelection(null);setPage(null);setCursor(0);
    try {
      const result=await call<ExperimentInspectionSelection|null>('file.inspectExperimentReport',{brandId});
      if(!current(version))return;
      if(result===null){setFeedback('已取消选择历史实验 ZIP');return}
      setSelection(result);
      await readPage(result,0,version);
    }catch(e){fail(version,e)}finally{finish(version)}
  }
  async function navigate(target:number) {
    if(!selection||target<0||target>=pages)return;
    const version=begin();if(version===null)return;
    setPage(null);setCursor(target);
    try {await readPage(selection,target,version)}catch(e){fail(version,e)}finally{finish(version)}
  }
  async function exportReport() {
    if(!selection)return;
    const version=begin();if(version===null)return;
    try {
      const result=await call<{saved:true}|null>('file.exportExperimentInspection',{brandId,inspectionId:selection.inspectionId,reportFingerprint:selection.report.reportFingerprint});
      if(current(version))setFeedback(result===null?'已取消保存本地核验报告':'本地核验报告 ZIP 已保存');
    }catch(e){fail(version,e)}finally{finish(version)}
  }
  const report=selection?.report, pages=Math.max(1,Math.ceil((report?.counts.observations??0)/20));
  return <Panel title="历史实验 ZIP 核验" description="只读核验已导出的本地历史包；不需要当前数据库中仍有原计划或问题。">
    <div style={wrap} aria-busy={busy}>
      <p>历史包的一致性不证明真实平台来源、平台认证或因果改善。人工实测表示用户录入，仍需核对原始证据。</p>
      <div className="actions" style={{margin:'14px 0'}}>
        <button style={buttonStyle} disabled={busy} onClick={()=>void select()}>选择历史实验 ZIP</button>
        <button style={buttonStyle} disabled={busy||!selection} onClick={()=>void exportReport()}>导出本地核验报告 ZIP</button>
      </div>
      {busy&&<p role="status">正在读取或保存本地历史核验资料…</p>}
      {feedback&&<p role="status" style={wrap}>{feedback}</p>}
      {error&&<p role="alert" style={wrap}>{error}</p>}
      {!selection&&!busy&&!feedback&&!error&&<p>尚未选择历史实验 ZIP。</p>}
      {report&&<div style={wrap}>
        <h3>{report.status==='passed'?'一致性检查通过':'一致性检查未通过'}</h3>
        <p>{report.evidenceLabel}</p>
        <dl className="info-list" style={{margin:'16px 0'}}>
          <dt>历史文件</dt><dd>{report.source.fileName}</dd>
          <dt>ZIP SHA256</dt><dd>{report.source.sha256}</dd>
          <dt>格式</dt><dd>{report.profile}</dd>
          <dt>文件大小</dt><dd>压缩 {report.source.compressedBytes} 字节 · 展开 {report.source.uncompressedBytes} 字节 · {report.source.fileCount} 个文件</dd>
          <dt>品牌 ID</dt><dd>{report.brandId}</dd><dt>计划 ID</dt><dd>{report.campaignId}</dd>
          <dt>核验指纹</dt><dd>{report.reportFingerprint}</dd>
        </dl>
        <p>基线 {report.counts.baseline}/{report.counts.expectedPerPhase} · 复测 {report.counts.followup}/{report.counts.expectedPerPhase} · 缺失 {report.counts.missing}/{report.counts.expectedPerPhase*2} 个计划单元</p>
        <p>原始观察 {report.counts.observations} 条 · 已更正观察 {report.counts.reviewedObservations} 条 · 历史评估 {report.counts.historyEntries} 个版本</p>
        <ul>{report.checks.map(c=><li key={c.code}>{c.code} · {c.status==='passed'?'通过':'未通过'} · {c.message}</li>)}</ul>
        <p>{report.caveat}</p>
        {report.platforms.map(p=><section className="cohort" key={p.platform} aria-label={`${platformNames[p.platform]}历史统计`}>
          <h3>{platformNames[p.platform]}</h3>
          <p>配对 {p.matchedPairs}/{p.expectedPerPhase} · 双阶段推荐已核对 {p.matchedAssessedPairs}/{p.matchedPairs} 对 · 未配对 {p.unmatchedSamples}/{p.baseline.samples+p.followup.samples} 条</p>
          {(['baseline','followup'] as const).map(phase=><p key={phase}>{phase==='baseline'?'基线':'复测'}：推荐 {p[phase].recommended}/{p[phase].assessed} 条已核对；未知 {p[phase].unknown}/{p[phase].samples} 条；前三 {p[phase].top3}/{p[phase].ranked} 条有顺位</p>)}
          <p>配对推荐率变化：{p.deltaPercentagePoints===null?'无法计算':`${p.deltaPercentagePoints.toFixed(1)} 个百分点`}（仅 {p.matchedAssessedPairs} 对两阶段已核对样本）</p>
        </section>)}
        <h3>历史原始记录与评估版本（只读）</h3>
        <p>“当前评估”指历史 ZIP 内的最新版本；与本机后来更正的数据无关。每页最多读取 20 条观察。</p>
        <nav aria-label="历史原始记录分页" className="actions" style={{flexDirection:'row',margin:'14px 0'}}>
          <button style={buttonStyle} disabled={busy||cursor===0} onClick={()=>void navigate(cursor-1)}>上一页</button>
          <span>第 {cursor+1}/{pages} 页 · 共 {report.counts.observations} 条</span>
          <button style={{...buttonStyle,marginTop:0}} disabled={busy||cursor+1>=pages} onClick={()=>void navigate(cursor+1)}>下一页</button>
        </nav>
        {!page&&!busy&&<button style={buttonStyle} onClick={()=>void navigate(cursor)}>重试读取历史记录</button>}
        {page?.total===0&&<p>历史包没有原始观察记录。</p>}
        {page?.observations.slice(0,20).map(o=><Sample key={`${selection.inspectionId}-${page.page}-${o.id}`} observation={o}/>)}
      </div>}
    </div>
  </Panel>;
}
