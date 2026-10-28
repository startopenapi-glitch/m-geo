import {useMemo,useState,type CSSProperties} from 'react';
import type {Kind} from '../types';
import {
 geoIntentLabels,geoRuleDisclaimer,geoSectionLabels,inspectGeoModelOutput,
 type GeoChannel,type GeoIntent,type GeoRuleConfig,type GeoRuleSeverity
} from '../geoRules';
import {isPublicSourceChannel,publicSourceChannels} from '../publicSourceChannels';
import {Badge,Panel} from './ui';

const wrap:CSSProperties={minWidth:0,maxWidth:'100%',overflowWrap:'anywhere',whiteSpace:'pre-wrap'};
const severityLabels:Record<GeoRuleSeverity,string>={block:'需补充',review:'人工复核','auto-clean':'安全清洗'};
const statusLabels={block:'结构待补充',review:'需人工复核','auto-clean':'已识别安全清洗',pass:'未命中结构缺口'} as const;
export interface GeoQualityPanelProps{
 draft:{body:string;kind:Kind;channel?:GeoChannel|null};
 intent?:GeoIntent;
 config?:GeoRuleConfig;
 onApplyCleanedBody?:(body:string)=>void;
}
/** Embeddable, local-only view. Applying cleanup is explicit and never reviews or saves a draft. */
export default function GeoQualityPanel({draft,intent,config,onApplyCleanedBody}:GeoQualityPanelProps){
 const [localIntent,setLocalIntent]=useState<GeoIntent>(intent??(draft.kind==='comparison'?'comparison':draft.kind==='guide'?'selection':'general'));
 const selectedIntent=intent??localIntent;
 const result=useMemo(()=>inspectGeoModelOutput(draft,selectedIntent,config),[draft,selectedIntent,config]);
 const publicProfile=isPublicSourceChannel(draft.channel)?publicSourceChannels[draft.channel]:null;
 const tone=result.status==='pass'?'green':result.status==='auto-clean'?'blue':'amber';
 return <Panel title="GEO 结构与公开信源检查" description="按渠道和问题意图检查必要结构；只清理可明确识别的外层模型话语。">
  <div style={wrap}>
   <div className="subheading"><Badge tone={tone}>{statusLabels[result.status]}</Badge><span className="inline-note">规则包 {result.rulePackVersion}</span></div>
   <p className="notice">{geoRuleDisclaimer}</p>
   <p className="inline-note">结构检查仅提供修改提示，不改变原生审核或导出资格。</p>
   {!intent&&<div className="field"><label htmlFor="geo-intent">问题意图</label><select id="geo-intent" value={selectedIntent} onChange={event=>setLocalIntent(event.target.value as GeoIntent)}>{Object.entries(geoIntentLabels).map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></div>}
   <h3>当前必填结构</h3>
   <p>{result.requiredSections.map(section=>geoSectionLabels[section]).join('、')||'当前配置没有额外必填结构；原生事实审核仍然适用。'}</p>
   <p className="inline-note">其余结构均为可选；对比意图本身不要求出现竞品，单品牌稿可按同一规则检查。</p>
   {result.issues.length?<ul aria-label="GEO 检查问题">{result.issues.map((issue,index)=><li key={`${issue.code}-${issue.section??'body'}-${index}`} style={{marginBottom:10}}><strong>{severityLabels[issue.severity]}{issue.section?` · ${geoSectionLabels[issue.section]}`:''}</strong><div style={wrap}>{issue.message}</div></li>)}</ul>:<p>当前配置未发现结构缺口。这不代表事实已核实或稿件已获准发布。</p>}
   {result.changed&&<section aria-label="安全清洗预览"><h3>清洗后正文预览</h3><p className="inline-note">这里只展示本地转换结果；写回后仍需重新执行原生审核与人工确认。</p><textarea className="draft-editor" rows={9} readOnly value={result.body} aria-label="清洗后正文"/>{onApplyCleanedBody&&<button type="button" onClick={()=>onApplyCleanedBody(result.body)}>将清洗结果写回编辑框</button>}</section>}
   {draft.channel&&<section aria-label="渠道属性"><h3>渠道属性</h3>{publicProfile?<div className="fact-card"><strong>{publicProfile.label} · 公开信源</strong><p>{publicProfile.purpose}</p><p><strong>所需证据：</strong>{publicProfile.requiredEvidence.join('；')}</p><p><strong>发布方式：</strong>仅人工发布，不连接账号，不自动发布。</p><p><strong>回读：</strong>{publicProfile.readBackSupport.instructions}</p><p className="inline-note"><strong>合规提醒：</strong>{publicProfile.complianceReminder}</p></div>:<p className="inline-note">当前为电商或电商内容渠道，不应登记为公开信源；平台发布与商品规范仍需单独核对。</p>}</section>}
   <details><summary>查看规则轨迹</summary><ol>{result.trace.map((entry,index)=><li key={`${entry.rule}-${index}`} style={wrap}><code>{entry.rule}</code> · {entry.outcome} · {entry.detail}</li>)}</ol><p className="inline-note">changed：{String(result.changed)}。轨迹用于解释本地规则，不是外部收录或效果证明。</p></details>
  </div>
 </Panel>;
}
