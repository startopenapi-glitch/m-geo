import type {ChannelId} from './channels';
import type {PublicSourceChannelId} from './publicSourceChannels';
import type {Kind} from './types';

export const GEO_RULE_PACK_VERSION='geo-p1-structure-1.0.0';
export const geoRuleDisclaimer='本地结构与清洗结果仍需经过原生事实证据审核和人工审核；不核实外部事实，不评估搜索或生成式结果表现。';

export type GeoIntent='general'|'selection'|'comparison'|'usage'|'after_sales';
export type GeoChannel=ChannelId|PublicSourceChannelId|'generic';
export type GeoSection='answer_summary'|'key_facts'|'applicability'|'usage_guidance'|'faq'|'sources';
export type GeoRuleSeverity='block'|'review'|'auto-clean';
export type GeoRuleStatus='block'|'review'|'auto-clean'|'pass';
export type GeoIssueCode='empty_body'|'missing_required_section'|'empty_required_section'|'model_preface_removed'|'code_fence_removed'|'publish_meta_removed'|'unsafe_meta_retained';
export interface GeoRuleIssue{
 code:GeoIssueCode;
 severity:GeoRuleSeverity;
 message:string;
 section?:GeoSection;
}
export interface GeoRuleTrace{
 rule:string;
 outcome:'passed'|'failed'|'changed'|'skipped'|'review';
 detail:string;
}
export interface GeoRuleResult{
 rulePackVersion:string;
 status:GeoRuleStatus;
 changed:boolean;
 issues:GeoRuleIssue[];
 trace:GeoRuleTrace[];
}
export interface GeoDraftInput{body:string;kind:Kind;channel?:GeoChannel|null}
export interface GeoCleanResult extends GeoRuleResult{body:string}
export interface GeoInspectionResult extends GeoRuleResult{body:string;intent:GeoIntent;requiredSections:GeoSection[]}
export interface GeoStructureRequirement{
 requiredSections?:GeoSection[];
 optionalSections?:GeoSection[];
}
export interface GeoRuleConfig{
 requirements?:{
  defaults?:GeoStructureRequirement;
  byKind?:Partial<Record<Kind,GeoStructureRequirement>>;
  byIntent?:Partial<Record<GeoIntent,GeoStructureRequirement>>;
  byChannel?:Partial<Record<GeoChannel,GeoStructureRequirement>>;
 };
}

export const geoSectionLabels:Record<GeoSection,string>={
 answer_summary:'答案摘要',
 key_facts:'事实要点',
 applicability:'适用条件',
 usage_guidance:'使用／选购说明',
 faq:'常见问题',
 sources:'来源'
};
export const geoIntentLabels:Record<GeoIntent,string>={
 general:'通用介绍',selection:'选购决策',comparison:'对比问题',usage:'使用方法',after_sales:'售后问题'
};

const allSections=Object.keys(geoSectionLabels) as GeoSection[];
const kindRequirements:Record<Kind,GeoSection[]>={
 description:['answer_summary','key_facts','sources'],
 faq:['faq','sources'],
 guide:['answer_summary','key_facts','applicability','usage_guidance','sources'],
 // Comparison intent describes the reader's task, not a mandatory competitor section.
 comparison:['answer_summary','key_facts','applicability','sources']
};
const intentRequirements:Partial<Record<GeoIntent,GeoSection[]>>={
 selection:['answer_summary','key_facts','applicability','usage_guidance','sources'],
 comparison:['answer_summary','key_facts','applicability','sources'],
 usage:['answer_summary','applicability','usage_guidance','sources'],
 after_sales:['answer_summary','applicability','usage_guidance','faq','sources']
};
const sectionAliases:Record<GeoSection,string[]>={
 answer_summary:['答案摘要','直接答案','核心答案','摘要','answer summary','summary'],
 key_facts:['事实要点','关键事实','核心参数','商品参数','产品参数','关键要点','key facts','facts'],
 applicability:['适用条件','适用范围','使用条件','限制条件','applicability','conditions'],
 usage_guidance:['使用说明','选购说明','使用建议','选购建议','操作步骤','usage','buying guide','instructions'],
 faq:['常见问题','常见问答','faq','q&a'],
 sources:['信息来源','资料来源','参考来源','参考资料','来源','sources','references']
};
const prefacePatterns=[
 /^(?:好的|当然可以|可以)[，,。.!！\s]*$/i,
 /^(?:以下|下面)(?:是|为)(?:根据[^\n]{0,50})?(?:生成|整理|撰写|提供)?的?(?:内容|草稿|答案|介绍|说明)[：:]\s*$/i,
 /^(?:sure|certainly)[,.!\s]*(?:here(?:'s| is)(?: the)?(?: draft| answer| content)?)[.:!\s]*$/i,
 /^as an ai(?: language model)?[,.!\s]*$/i
];
const standalonePublishMetaPatterns=[
 /^(?:本文|内容|草稿|稿件)?(?:已经|已)发布(?:到(?:官网|网站|平台|媒体|社区))?[。.!！]*$/i,
 /^(?:我们)?(?:保证|确保|承诺)(?:首页|排名第一|排名靠前|获得排名)[。.!！]*$/i,
 /^(?:guaranteed\s+(?:ranking|rankings|first\s+page|top\s+ranking)|first\s+page\s+guaranteed)[.!\s]*$/i
];
const retainedMetaPattern=/(?:已经?发布|保证首页|确保首页|承诺首页|guaranteed\s+(?:ranking|rankings|first\s+page|top\s+ranking)|first\s+page\s+guaranteed)/i;

function statusFor(issues:GeoRuleIssue[],changed:boolean):GeoRuleStatus{
 if(issues.some(issue=>issue.severity==='block'))return 'block';
 if(issues.some(issue=>issue.severity==='review'))return 'review';
 if(changed||issues.some(issue=>issue.severity==='auto-clean'))return 'auto-clean';
 return 'pass';
}
function uniqueSections(sections:GeoSection[]):GeoSection[]{
 return allSections.filter(section=>sections.includes(section));
}
function applyRequirement(target:GeoSection[],requirement?:GeoStructureRequirement):GeoSection[]{
 return requirement?.requiredSections?uniqueSections(requirement.requiredSections):target;
}
/** Resolve defaults first, then explicit kind, intent and channel overrides. */
export function resolveGeoSections(kind:Kind,intent:GeoIntent,channel:GeoChannel|null|undefined,config:GeoRuleConfig={}):GeoSection[]{
 let required=[...(intentRequirements[intent]??kindRequirements[kind])];
 // Short-form commerce surfaces keep evidence in the native claim audit without forcing a visible source appendix.
 if(channel&&['douyin','wechat_channels','xiaohongshu','kuaishou'].includes(channel))required=required.filter(section=>section!=='sources');
 if(channel==='public_qa_manual'&&!required.includes('faq'))required.push('faq');
 if(channel==='documentation_center'&&intent==='usage'&&!required.includes('usage_guidance'))required.push('usage_guidance');
 const overrides=config.requirements;
 required=applyRequirement(required,overrides?.defaults);
 required=applyRequirement(required,overrides?.byKind?.[kind]);
 required=applyRequirement(required,overrides?.byIntent?.[intent]);
 required=applyRequirement(required,channel?overrides?.byChannel?.[channel]:undefined);
 return uniqueSections(required);
}
function inferIntent(kind:Kind):GeoIntent{
 if(kind==='comparison')return 'comparison';
 if(kind==='guide')return 'selection';
 return 'general';
}
function sectionPattern(section:GeoSection):RegExp{
 const aliases=sectionAliases[section].map(alias=>alias.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')).join('|');
 return new RegExp(`^\\s{0,3}(?:#{1,6}\\s*)?(?:${aliases})\\s*(?::|：)?\\s*(.*?)\\s*$`,'i');
}
function hasSectionContent(body:string,section:GeoSection):boolean{
 if(section==='faq'&&/(?:^|\n)\s*(?:Q[.:：]|问[：:])[^\n]+\n\s*(?:A[.:：]|答[：:])\s*\S+/i.test(body))return true;
 const lines=body.split(/\r?\n/);
 const pattern=sectionPattern(section);
 for(let index=0;index<lines.length;index++){
  const match=lines[index].match(pattern);
  if(!match)continue;
  if(match[1]?.trim())return true;
  for(let next=index+1;next<lines.length;next++){
   if(allSections.some(candidate=>sectionPattern(candidate).test(lines[next])))break;
   const value=lines[next].trim();
   if(value&&!/^\s{0,3}#{1,6}\s+/.test(value))return true;
  }
 }
 return false;
}
export function inspectGeoStructure(
 draft:GeoDraftInput,
 intent?:GeoIntent,
 config:GeoRuleConfig={}
):GeoInspectionResult{
 const resolvedIntent=intent??inferIntent(draft.kind);
 const requiredSections=resolveGeoSections(draft.kind,resolvedIntent,draft.channel??'generic',config);
 const issues:GeoRuleIssue[]=[];
 const trace:GeoRuleTrace[]=[{rule:'profile.resolve',outcome:'passed',detail:`kind=${draft.kind}; intent=${resolvedIntent}; channel=${draft.channel??'generic'}`}];
 if(!draft.body.trim())issues.push({code:'empty_body',severity:'block',message:'正文为空，无法进行结构检查。'});
 for(const section of requiredSections){
  const present=hasSectionContent(draft.body,section);
  trace.push({rule:`section.${section}`,outcome:present?'passed':'failed',detail:present?`${geoSectionLabels[section]}已有内容。`:`缺少${geoSectionLabels[section]}。`});
  if(!present)issues.push({code:draft.body.trim()?'missing_required_section':'empty_required_section',severity:'block',section,message:`当前渠道与意图要求“${geoSectionLabels[section]}”，请补充有实际内容的对应段落。`});
 }
 for(const section of allSections.filter(section=>!requiredSections.includes(section)))trace.push({rule:`section.${section}`,outcome:'skipped',detail:`${geoSectionLabels[section]}在当前配置中为可选。`});
 trace.push({rule:'native_evidence_review',outcome:'skipped',detail:'事实来源、公开性、审核状态与主张对应关系仍由原生审核处理。'});
 return {rulePackVersion:GEO_RULE_PACK_VERSION,status:statusFor(issues,false),changed:false,issues,trace,body:draft.body,intent:resolvedIntent,requiredSections};
}
function removeLeadingPrefaces(input:string):{body:string;count:number}{
 let body=input,count=0;
 for(;;){
  const firstLine=body.match(/^([^\r\n]*)(?:\r?\n|$)/);
  if(!firstLine||!prefacePatterns.some(pattern=>pattern.test(firstLine[1].trim())))break;
  body=body.slice(firstLine[0].length);
  count++;
 }
 return {body,count};
}
function removeStandaloneMeta(input:string):{body:string;count:number}{
 const lines=input.split(/(\r?\n)/);
 let count=0;
 for(let index=0;index<lines.length;index+=2){
  if(standalonePublishMetaPatterns.some(pattern=>pattern.test(lines[index].trim()))){lines[index]='';count++;}
 }
 const body=lines.join('');
 return {body:count?body.replace(/^(?:\r?\n)+/,'').replace(/(?:\r?\n){3,}/g,'\n\n'):body,count};
}
/**
 * Conservative presentation cleanup only. Embedded claims and promises are retained;
 * cleanup never marks evidence as verified and never bypasses native draft review.
 */
export function cleanModelOutput(input:string):GeoCleanResult{
 let body=input;
 const issues:GeoRuleIssue[]=[];
 const trace:GeoRuleTrace[]=[];
 let prefaces=removeLeadingPrefaces(body);body=prefaces.body;
 const fenced=body.match(/^\s*```(?:markdown|md|text|txt)?[\t ]*\r?\n([\s\S]*?)\r?\n```[\t ]*$/i);
 if(fenced){body=fenced[1];issues.push({code:'code_fence_removed',severity:'auto-clean',message:'已移除包裹整篇正文的 Markdown 代码围栏。'});trace.push({rule:'clean.code_fence',outcome:'changed',detail:'仅移除了完整包裹正文的外层围栏。'});}
 const afterFencePrefaces=removeLeadingPrefaces(body);body=afterFencePrefaces.body;prefaces.count+=afterFencePrefaces.count;
 if(prefaces.count){issues.push({code:'model_preface_removed',severity:'auto-clean',message:'已移除正文开头可明确识别的模型前言。'});trace.push({rule:'clean.model_preface',outcome:'changed',detail:`移除了 ${prefaces.count} 行独立模型前言。`});}
 const meta=removeStandaloneMeta(body);body=meta.body;
 if(meta.count){issues.push({code:'publish_meta_removed',severity:'auto-clean',message:'已移除独立成行的发布状态或排名保证元话语。'});trace.push({rule:'clean.publish_meta',outcome:'changed',detail:`移除了 ${meta.count} 行独立外层元话语。`});}
 if(retainedMetaPattern.test(body)){issues.push({code:'unsafe_meta_retained',severity:'review',message:'正文仍含发布状态或排名保证表述；因其可能与实质内容混合，未自动删除，请人工核对。'});trace.push({rule:'clean.unsafe_meta',outcome:'review',detail:'保留了无法证明可安全移除的表述。'});}
 trace.push({rule:'native_evidence_review',outcome:'skipped',detail:'清洗后仍须执行原生事实证据审核与人工审核。'});
 const changed=body!==input;
 return {rulePackVersion:GEO_RULE_PACK_VERSION,status:statusFor(issues,changed),changed,issues,trace,body};
}
/** Clean first, then inspect the actual cleaned body; neither step replaces native review. */
export function inspectGeoModelOutput(
 draft:GeoDraftInput,
 intent?:GeoIntent,
 config:GeoRuleConfig={}
):GeoInspectionResult{
 const cleaned=cleanModelOutput(draft.body);
 const structure=inspectGeoStructure({...draft,body:cleaned.body},intent,config);
 const issues=[...cleaned.issues,...structure.issues];
 return {...structure,status:statusFor(issues,cleaned.changed),changed:cleaned.changed,issues,trace:[...cleaned.trace,...structure.trace]};
}
export const inspectGeoRules=inspectGeoModelOutput;
