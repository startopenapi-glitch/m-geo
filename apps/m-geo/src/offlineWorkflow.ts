import type {Brand,Product} from './types';

export type OfflineWorkflowStatus='paused'|'running'|'awaiting_review'|'completed'|'failed'|'cancelled';
export type OfflineNodeKind='baseline'|'rewrite'|'audit'|'review'|'followup';
export interface OfflineAudit {scope:'offline_html';checks:{id:string;label:string;status:'pass'|'warning'|'fail';detail:string}[];passed:number;warning:number;blocking:number;networkRequests:0}
export interface OfflineMonitor {sampleCount:number;mentionCount:number;citationCount:number;samples:{question:string;answer:string;citations:string[];mentioned:boolean;evidenceMode:'simulated';surface:'fixture'}[];networkRequests:0;modelCalls:0;mentionDelta?:number;comparisonLabel?:string}
export interface OfflineWorkflowNode {id:string;kind:OfflineNodeKind;dependsOn:string[];status:'pending'|'completed'|'blocked'|'failed';result:Record<string,unknown>|null;error:string|null}
export interface OfflineWorkflow {id:string;brandId:string;productId:string;productRevision:number;requestId:string;name:string;revision:number;status:OfflineWorkflowStatus;questions:string[];repetitions:number;draftId:string|null;createdAt:string;updatedAt:string;error:string|null;budget:{maxSamples:number;usedSamples:number;networkRequests:0;modelCalls:0;publications:0};nodes:OfflineWorkflowNode[]}
export interface OfflineCreate {brandId:string;requestId:string;name:string;productId:string;questions:string[];repetitions:number;maxSamples:number;evidenceMode:'simulated'}
export const offlineStatusLabel:Record<OfflineWorkflowStatus,string>={paused:'已暂停',running:'运行中',awaiting_review:'等待人工审核',completed:'已完成',failed:'失败',cancelled:'已取消'};
export const offlineNodeLabel:Record<OfflineNodeKind,string>={baseline:'模拟基线',rewrite:'本地整理候选稿',audit:'离线 HTML 检查',review:'人工审核关卡',followup:'模拟复测'};
export const defaultOfflineQuestions='选购这类商品应关注哪些指标？\n比较同类商品时如何核对公开资料？\n使用前需要确认哪些注意事项？';
export function validateOfflineCreate(brand:Brand,input:{name:string;product:Product|undefined;questionsText:string;repetitions:number;maxSamples:number}):string{
 const {product,name,repetitions,maxSamples}=input,questions=input.questionsText.split(/\r?\n/).map(q=>q.trim()).filter(Boolean);
 if(!product||product.brandId!==brand.id||!product.confirmedAt)return '请选择当前品牌的已确认商品';
 if(!name.trim()||[...name.trim()].length>100)return '任务名称须为 1–100 字';
 if(!questions.length||questions.length>10||questions.some(q=>[...q].length>500))return '请输入 1–10 条问题，每条最多 500 字';
 if(new Set(questions.map(q=>q.toLocaleLowerCase())).size!==questions.length)return '问题不能重复';
 const identities=[brand.name,...brand.profile?.aliases??[],product.name].map(s=>s.trim().toLocaleLowerCase()).filter(Boolean);
 if(questions.some(q=>identities.some(identity=>q.toLocaleLowerCase().includes(identity))))return '问题不能包含品牌名称、别名或完整商品名';
 if(!Number.isInteger(repetitions)||repetitions<1||repetitions>3)return '每题重复次数须为 1–3';
 if(!Number.isInteger(maxSamples)||maxSamples<1||maxSamples>60||questions.length*repetitions*2>maxSamples)return '两轮所需样本不能超过预算，预算最多 60';
 return '';
}
export function validateOfflineHtml(html:string):string{
 if(!html.trim())return '请粘贴离线 HTML 源码';
 if(new TextEncoder().encode(html).byteLength>262144)return 'HTML 超过 256 KiB UTF-8 上限';
 if(/^https?:\/\/\S+$/i.test(html.trim()))return '请粘贴 HTML 源码；本地检查不会读取网址';
 return '';
}
