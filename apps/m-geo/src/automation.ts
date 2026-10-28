import type {EvidenceMode,Observation,Platform,WorkspaceSnapshot} from './types';

export const AUTOMATION_NODE_KINDS=['fact_expiry_scan','publication_readback_due','visibility_retest_reminder'] as const;
export type AutomationNodeKind=typeof AUTOMATION_NODE_KINDS[number];
export type AutomationStatus='active'|'paused'|'cancelled';
export interface AutomationNode{id:string;kind:AutomationNodeKind;dependsOn:string[]}
export type AutomationReminderKind='fact_expired'|'fact_expiring'|'publication_readback_due'|'visibility_retest_due';
export interface AutomationReminder{kind:AutomationReminderKind;title:string;detail:string;due:boolean;refs:Record<string,string|number|boolean|null>}
export interface AutomationNodeReport{nodeId:string;kind:AutomationNodeKind;reminders:AutomationReminder[];omitted:number}
export interface AutomationReport{id:string;ranAt:string;nodes:AutomationNodeReport[];modelCalls:0;modelFeeMinor:0;networkRequests:0;publicationWrites:0}
export interface AutomationTask{id:string;brandId:string;requestId:string;createFingerprint:string;format:'m-geo-automation-task-v1';name:string;intervalMinutes:number;nodes:AutomationNode[];status:AutomationStatus;revision:number;createdAt:string;updatedAt:string;nextRunAt:string;lastRunAt:string|null;totalRuns:number;missedRuns:number;reports:AutomationReport[]}
export type AutomationCreated=AutomationTask&{alreadyExists:boolean};
export interface AutomationRunDue{tasks:AutomationTask[];reports:AutomationReport[];skipped:number;modelCalls:0;modelFeeMinor:0;networkRequests:0;publicationWrites:0}
export interface AutomationCreatePayload{brandId:string;requestId:string;name:string;intervalMinutes:number;nodes:AutomationNode[]}

export const MIN_TREND_SAMPLES=30;
export function buildAutomationNodes(kinds:readonly AutomationNodeKind[]):AutomationNode[]{
 const selected=[...new Set(kinds)];
 if(!selected.length||selected.length>8||selected.some(kind=>!AUTOMATION_NODE_KINDS.includes(kind)))throw new Error('请选择至少一个受支持的只读巡检节点');
 return selected.map((kind,index)=>({id:`node-${index+1}`,kind,dependsOn:index?[`node-${index}`]:[]}));
}
export function makeAutomationCreatePayload(brandId:string,name:string,intervalMinutes:number,kinds:readonly AutomationNodeKind[],requestId:string):AutomationCreatePayload{
 const trimmed=name.trim();
 if(!brandId||!requestId)throw new Error('缺少品牌或请求标识');
 if(!trimmed||trimmed.length>100)throw new Error('巡检名称须为1至100字');
 if(!Number.isSafeInteger(intervalMinutes)||intervalMinutes<15||intervalMinutes>525600)throw new Error('巡检间隔须为15分钟至365天');
 return {brandId,name:trimmed,intervalMinutes,nodes:buildAutomationNodes(kinds),requestId};
}
export function taskIsDue(task:AutomationTask,at=new Date()):boolean{
 const next=Date.parse(task.nextRunAt);
 return task.status==='active'&&Number.isFinite(next)&&next<=at.getTime();
}

export interface TrendPoint{
 key:string;date:string;evidenceMode:EvidenceMode;modelLabel:string;platform:Platform;campaignId:string|null;campaignName:string;
 sampleCount:number;mentionedCount:number;citedCount:number;accuracyReviewedCount:number;accuracyCorrectCount:number;
 mentionRate:number;citationRate:number;accuracyReviewed:number|null;sampleInsufficient:boolean;
}
export interface TrendFilters{evidenceMode?:EvidenceMode|'all';modelLabel?:string|'all';platform?:Platform|'all';from?:string;to?:string;campaignId?:string|null|'all'}
export interface TrendSummary{sampleCount:number;mentionedCount:number;citedCount:number;accuracyReviewedCount:number;accuracyCorrectCount:number;mentionRate:number|null;citationRate:number|null;accuracyReviewed:number|null;sampleInsufficient:boolean}
export interface TrendVersionComparison{baseline:TrendSummary;comparison:TrendSummary;mentionRateDelta:number|null;citationRateDelta:number|null;accuracyReviewedDelta:number|null;comparable:boolean;note:string}

function validDate(value:string):string|null{
 if(!/^\d{4}-\d{2}-\d{2}T/.test(value))return null;
 const time=Date.parse(value);if(!Number.isFinite(time))return null;
 return new Date(time).toISOString().slice(0,10);
}
function evidenceMode(observation:Observation,campaigns:WorkspaceSnapshot['campaigns']):EvidenceMode{
 const id=observation.assessment?.campaignId;
 return (id?campaigns?.find(campaign=>campaign.id===id):undefined)?.evidenceMode??'unspecified';
}
function summarizeCounts(sampleCount:number,mentionedCount:number,citedCount:number,accuracyReviewedCount:number,accuracyCorrectCount:number):TrendSummary{
 return {sampleCount,mentionedCount,citedCount,accuracyReviewedCount,accuracyCorrectCount,
  mentionRate:sampleCount?mentionedCount/sampleCount:null,citationRate:sampleCount?citedCount/sampleCount:null,
  accuracyReviewed:accuracyReviewedCount?accuracyCorrectCount/accuracyReviewedCount:null,sampleInsufficient:sampleCount<MIN_TREND_SAMPLES};
}
export function summarizeTrend(points:readonly TrendPoint[]):TrendSummary{
 return summarizeCounts(points.reduce((sum,point)=>sum+point.sampleCount,0),points.reduce((sum,point)=>sum+point.mentionedCount,0),points.reduce((sum,point)=>sum+point.citedCount,0),points.reduce((sum,point)=>sum+point.accuracyReviewedCount,0),points.reduce((sum,point)=>sum+point.accuracyCorrectCount,0));
}
/** Aggregates only identical evidence mode/model/platform/campaign/date cells; simulated and recorded evidence never share a cell. */
export function deriveTrendPoints(data:WorkspaceSnapshot,filters:TrendFilters={}):TrendPoint[]{
 const campaigns=data.campaigns??[];
 const cells=new Map<string,{observations:Observation[];mode:EvidenceMode;campaignId:string|null;campaignName:string;date:string}>();
 for(const observation of data.observations){
  const date=validDate(observation.testedAt);if(!date)continue;
  const campaignId=observation.assessment?.campaignId??null;
  const campaign=campaignId?campaigns.find(item=>item.id===campaignId):undefined;
  const mode=evidenceMode(observation,campaigns),modelLabel=observation.modelLabel,platform=observation.platform;
  if(filters.evidenceMode&&filters.evidenceMode!=='all'&&filters.evidenceMode!==mode)continue;
  if(filters.modelLabel&&filters.modelLabel!=='all'&&filters.modelLabel!==modelLabel)continue;
  if(filters.platform&&filters.platform!=='all'&&filters.platform!==platform)continue;
  if(filters.campaignId!==undefined&&filters.campaignId!=='all'&&filters.campaignId!==campaignId)continue;
  if(filters.from&&date<filters.from||filters.to&&date>filters.to)continue;
  const key=JSON.stringify([date,mode,modelLabel,platform,campaignId]);
  const cell=cells.get(key)??{observations:[],mode,campaignId,campaignName:campaign?.name??'未关联固定计划',date};
  cell.observations.push(observation);cells.set(key,cell);
 }
 return [...cells].map(([key,cell])=>{
  const sampleCount=cell.observations.length;
  const mentionedCount=cell.observations.filter(item=>item.mentioned).length;
  const citedCount=cell.observations.filter(item=>!!item.citedUrl.trim()).length;
  const reviewed=cell.observations.filter(item=>item.accuracy!=='unknown');
  const point=summarizeCounts(sampleCount,mentionedCount,citedCount,reviewed.length,reviewed.filter(item=>item.accuracy==='correct').length);
  const first=cell.observations[0];
  return {key,date:cell.date,evidenceMode:cell.mode,modelLabel:first.modelLabel,platform:first.platform,campaignId:cell.campaignId,campaignName:cell.campaignName,...point} as TrendPoint;
 }).sort((left,right)=>left.date.localeCompare(right.date)||left.key.localeCompare(right.key));
}
export function compareTrendVersions(baselinePoints:readonly TrendPoint[],comparisonPoints:readonly TrendPoint[]):TrendVersionComparison{
 const baseline=summarizeTrend(baselinePoints),comparison=summarizeTrend(comparisonPoints);
 const comparable=!baseline.sampleInsufficient&&!comparison.sampleInsufficient;
 const delta=(left:number|null,right:number|null)=>left===null||right===null?null:right-left;
 return {baseline,comparison,mentionRateDelta:delta(baseline.mentionRate,comparison.mentionRate),citationRateDelta:delta(baseline.citationRate,comparison.citationRate),accuracyReviewedDelta:delta(baseline.accuracyReviewed,comparison.accuracyReviewed),comparable,
  note:comparable?'仅描述现有样本差异；不证明因果、真实平台提升或持续改善。':`至少一组样本少于${MIN_TREND_SAMPLES}，或缺少可计算指标；只列描述值，不作提升判断。`};
}
export function formatRate(value:number|null):string{return value===null?'—':`${(value*100).toFixed(1)}%`}
export function evidenceModeLabel(mode:EvidenceMode):string{return mode==='recorded'?'人工实测记录':mode==='simulated'?'模拟数据（非真实平台表现）':'历史未分类（不得视为真实实测）'}
