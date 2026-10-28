import type {Section} from './App';
import type {WorkspaceSnapshot} from './types';
import {draftCanExport,productHasCurrentFacts} from './components/draftEligibility';

export type WorkflowStepId='facts'|'questions'|'content'|'test'|'compare'|'optimize';
export interface WorkflowStep{
 id:WorkflowStepId;
 title:string;
 detail:string;
 target:Section;
 complete:boolean;
}
export interface WorkflowNext{
 step:WorkflowStepId;
 title:string;
 description:string;
 action:string;
 target:Section;
}

/** A presentation-only map of the local GEO loop; it never infers external AI performance. */
export function geoWorkflow(data:WorkspaceSnapshot,brandId:string):{steps:WorkflowStep[];next:WorkflowNext}{
 const confirmed=data.products.filter(p=>productHasCurrentFacts(p)).length;
 const pending=data.products.length-confirmed;
 const confirmedQuestions=data.questions.filter(q=>q.confirmed).length;
 const pendingQuestions=data.questions.length-confirmedQuestions;
 const reviewed=data.drafts.filter(d=>draftCanExport(data,d,brandId)).length;
 const attention=data.drafts.length-reviewed;
 const hasFollowup=data.observations.some(o=>o.phase==='followup');
 const steps:WorkflowStep[]=[
  {id:'facts',title:'事实与证据',detail:data.products.length?`${confirmed}/${data.products.length} 件商品已确认`:'尚未录入商品',target:'products',complete:data.products.length>0&&pending===0},
  {id:'questions',title:'问题与诊断',detail:`${confirmedQuestions} 条已确认 · ${pendingQuestions} 条待确认`,target:'pilot',complete:confirmedQuestions>0},
  {id:'content',title:'内容与审核',detail:`${reviewed} 篇可导出 · ${attention} 篇待核对`,target:'content',complete:data.drafts.length>0&&attention===0},
  {id:'test',title:'AI 可见度测试',detail:`${data.observations.length} 条回答记录`,target:'visibility',complete:data.observations.length>0},
  {id:'compare',title:'基线与复测比较',detail:hasFollowup?'已有复测记录，可核对前后变化':'尚需保持条件一致并补充复测',target:'visibility',complete:hasFollowup},
  {id:'optimize',title:'优化与再验证',detail:'从冻结测试证据建立人工行动项',target:'optimization',complete:false},
 ];
 let next:WorkflowNext;
 if(!data.products.length)next={step:'facts',title:'从第一件商品开始',description:'先录入名称、SKU 和一条有来源的事实，再制作内容。',action:'录入第一件商品',target:'products'};
 else if(pending)next={step:'facts',title:`还有 ${pending} 件商品待确认`,description:'补齐来源并确认公开事实，稿件主张才能通过审核。',action:'继续完善商品事实',target:'products'};
 else if(!confirmedQuestions)next={step:'questions',title:'建立第一版固定问题集',description:pendingQuestions?`已有 ${pendingQuestions} 条候选问题，请逐条核对来源并确认，再用于固定测试。`:'从真实购买意图整理问题；模板建议只是线索，不代表真实搜索需求。',action:'整理并确认问题',target:'pilot'};
 else if(!data.drafts.length)next={step:'content',title:'把事实转成第一份可审核草稿',description:'选择已确认商品生成草稿，逐条核对关键主张与事实依据。',action:'开始制作内容',target:'content'};
 else if(attention)next={step:'content',title:`有 ${attention} 篇内容需要核对`,description:'检查稿件与当前事实是否一致，审核通过后才能导出。',action:'继续核对内容',target:'content'};
 else if(!data.observations.length)next={step:'test',title:'用固定问题开始一次 AI 测试',description:'记录模型、日期和回答性质；模拟结果不会被视为真实平台表现。',action:'开始 AI 测试',target:'visibility'};
 else if(!hasFollowup)next={step:'compare',title:'用同一问题集补一次复测',description:'保持模型与测试条件尽量一致，再比较提及、引用和事实准确性。',action:'记录复测回答',target:'visibility'};
 else next={step:'optimize',title:'把复测结果转成优化行动',description:'用冻结证据诊断事实、内容和测量缺口；人工处理后再验证。',action:'打开优化闭环',target:'optimization'};
 return {steps,next};
}
