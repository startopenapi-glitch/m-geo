import {csvMappingKeys,type CsvDiagnostic,type CsvPrepared} from '../operationsTypes';

const fieldLabels:Record<string,string>={kind:'记录类型',id:'记录编号',sessionId:'会话编号',sku:'商品 SKU',occurredAt:'发生时间',source:'来源',sourceEvidence:'来源证据',status:'订单状态',amount:'金额',amountMinor:'金额',orderId:'原订单编号',name:'数据集名称',evidenceMode:'数据性质',sourceLabel:'资料来源',coverageStart:'覆盖开始时间',coverageEnd:'覆盖结束时间',currency:'币种',amountUnit:'金额单位',mapping:'字段映射'};
const scopeLabels={csv:'CSV 结构',mapping:'字段映射',metadata:'资料说明',row:'记录字段',reference:'记录关联',catalog:'当前品牌商品',dataset:'整份资料'};
const object=(value:unknown):value is Record<string,unknown>=>typeof value==='object'&&value!==null&&!Array.isArray(value);
const natural=(value:unknown):value is number=>Number.isSafeInteger(value)&&Number(value)>=0;
const recordNumber=(value:unknown):value is number=>natural(value)&&value>0;
const textKeys=(value:Record<string,unknown>,keys:string[])=>keys.every(key=>typeof value[key]==='string');
const oneOf=(value:unknown,values:string[])=>typeof value==='string'&&values.includes(value);
function validPrepared(value:unknown):value is CsvPrepared{
 if(!object(value)||typeof value.content!=='string'||!object(value.mapping)||!object(value.metadata)||!object(value.preview))return false;
 const mapping=value.mapping;
 if(!csvMappingKeys.every(key=>mapping[key]===null||typeof mapping[key]==='string'))return false;
 const metadata=value.metadata,preview=value.preview;
 if(!textKeys(metadata,['sourceLabel','coverageStart','coverageEnd'])||!oneOf(metadata.evidenceMode,['simulated','recorded'])||!oneOf(metadata.currency,['CNY','USD','EUR'])||!oneOf(metadata.amountUnit,['minor','major']))return false;
 const counts=preview.counts;
 return textKeys(preview,['contentHash','sourceLabel','coverageStart','coverageEnd'])&&oneOf(preview.evidenceMode,['simulated','recorded'])&&oneOf(preview.currency,['CNY','USD','EUR'])&&typeof preview.alreadyImported==='boolean'&&object(counts)&&['visits','orders','refunds','duplicates'].every(key=>natural(counts[key]))&&Array.isArray(preview.products)&&preview.products.every(p=>object(p)&&textKeys(p,['id','sku','name'])&&natural(p.revision));
}

// Reject malformed native responses before they can render or become staged data.
export function checkedDiagnostic(value:unknown,stage:CsvDiagnostic['stage']):CsvDiagnostic{
 const invalid=()=>{throw new Error('CSV 检查响应无效，请重新检查')};
 if(!object(value)||value.format!=='m-geo-csv-diagnostics-v1'||value.stage!==stage||!oneOf(value.status,['ready','blocked']))return invalid();
 if(!Array.isArray(value.headers)||value.headers.length>40||!value.headers.every(h=>typeof h==='string')||!(value.rowCount===null||natural(value.rowCount))||!natural(value.totalIssues)||typeof value.issuesTruncated!=='boolean'||!Array.isArray(value.issues)||value.issues.length>200||value.totalIssues<value.issues.length||value.issuesTruncated!==(value.totalIssues>value.issues.length))return invalid();
 for(const issue of value.issues){
  if(!object(issue)||typeof issue.scope!=='string'||!Object.hasOwn(scopeLabels,issue.scope)||!(issue.record===null||recordNumber(issue.record))||!(issue.field===null||typeof issue.field==='string')||!textKeys(issue,['code','message'])||!Array.isArray(issue.relatedRecords)||issue.relatedRecords.length>20||!issue.relatedRecords.every((n,i,all)=>recordNumber(n)&&(i===0||n>all[i-1])))return invalid();
 }
 if(value.status==='ready'){
  if(value.rowCount===null||!value.headers.length||value.headers.some(h=>!h)||new Set(value.headers).size!==value.headers.length||value.totalIssues!==0||value.issuesTruncated||(stage==='mapped'?!validPrepared(value.prepared):value.prepared!==null))return invalid();
 }else if(value.prepared!==null||value.totalIssues===0||value.issues.length===0)return invalid();
 return value as unknown as CsvDiagnostic;
}

export default function CsvDiagnostics({value}:{value:CsvDiagnostic}){
 return <section aria-label="CSV 检查结果" aria-live="polite" className="cohort" style={{minWidth:0,overflowWrap:'anywhere'}}>
  <h3>{value.stage==='structure'?'CSV 结构检查':'CSV 完整检查'} · {value.status==='ready'?'检查通过':'需要修复'}</h3>
  <p className="inline-note">数据记录从表头后的第一条起按 1 编号；带引号的多行字段仍算一条记录，不是物理行号。</p>
  {value.status==='blocked'?<>
   <p role="status">当前检查阶段共 {value.totalIssues} 项问题{value.issuesTruncated?`，仅显示前 ${value.issues.length} 项，其余问题未展示`:`，显示全部 ${value.issues.length} 项`}。请修复这些问题后重新检查；后续阶段尚未检查，不会跳过问题记录。</p>
   <ol className="queue-list">{value.issues.map((issue,index)=><li key={index}>
    <strong>{issue.record===null?scopeLabels[issue.scope]:`数据记录 ${issue.record}`}{issue.field?` · ${Object.hasOwn(fieldLabels,issue.field)?fieldLabels[issue.field]:'相关字段'}`:''}</strong>
    <p>{issue.message}</p>
    {!!issue.relatedRecords.length&&<p className="inline-note">相关数据记录：{issue.relatedRecords.join('、')}</p>}
   </li>)}</ol>
  </>:<p>{value.stage==='structure'?`已检查 ${value.rowCount} 条数据记录，请明确映射与资料说明后准备完整预览。`:'当前完整检查通过。请核对预览后手动加入待处理列表，尚未保存数据集。'}</p>}
 </section>;
}
