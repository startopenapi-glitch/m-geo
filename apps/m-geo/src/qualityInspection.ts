import type {Draft,DraftClaim,Fact,Product} from './types';
import {factIsApproved} from './components/factReview';

export type QualityInspectionStatus='blocked'|'review'|'pass';
export type QualityIssueSeverity='block'|'review';
export type QualityIssueCode=
 |'draft_product_mismatch'
 |'no_key_claims'
 |'claim_not_in_body'
 |'fact_not_found'
 |'fact_not_public'
 |'fact_not_approved'
 |'fact_source_missing'
 |'absolute_claim'
 |'efficacy_promise'
 |'valid_until_missing'
 |'source_vague';

export interface QualityIssue {
 code:QualityIssueCode;
 severity:QualityIssueSeverity;
 message:string;
 claimId?:string;
}

export interface QualityInspectionResult {
 status:QualityInspectionStatus;
 issues:QualityIssue[];
 claimsTotal:number;
 claimsWithEvidence:number;
}

export const localQualityInspectionDisclaimer='本地规则检查仅用于发现明确缺口；pass 仅表示未命中这些规则，不代表事实已核实或内容已获准发布，不能替代人工审核。';

const absoluteClaimPattern=/(?:全网|全国|全球|行业|世界)第[一1]|100\s*[%％]\s*(?:有效|保证|安全|成功)|百分之百(?:有效|安全)|绝对安全|永久有效|最安全|零风险/g;
const efficacyPromisePattern=/(?:保证|确保|承诺)[^。！？!?；;，,：:\n]{0,12}(?:有效|见效|治愈|根治|康复|减肥|瘦身|降糖|降压)|(?:立即|立刻|[两三四五六七八九十\d]+天)(?:内)?(?:见效|治愈|根治)|药到病除|包治百病/g;
const negationPattern=/(?:并非|不是|不代表|不意味着|不等于|无法|不能|不得|不应|避免|请勿|不)[^。！？!?；;，,：:\n]{0,6}$/;
const timeSensitivePattern=/(?:价格|售价|定价|优惠价|活动价|到手价|促销|折扣|优惠活动|库存|现货|缺货|补货|price|availability|stock|inventory|discount|promotion)/i;
const temporalContextPattern=/(?:目前|当前|今日|今天|本周|本月|限时|截至)/;
const vagueSources=new Set([
 '网络','网上','互联网','来自网络','来源于网络','网上查询','网络资料','网上资料','互联网资料','网络资料整理',
 '官网','官方网站','见官网','官网资料','官网信息','官方','官方资料','官方信息','官方说明',
 '品牌','品牌方','品牌提供','品牌资料','品牌官方','商家','商家提供','厂家','厂家提供','厂家资料',
 '内部','内部资料','内部信息','相关资料','相关说明','公开资料','产品资料','商品资料','产品说明','商品说明',
 '搜索','搜索引擎','百度搜索','用户反馈','ai','ai生成','模型生成'
]);

function hasUnnegatedMatch(body:string,pattern:RegExp):boolean {
 for(const match of body.matchAll(new RegExp(pattern.source,pattern.flags))){
  const index=match.index??0;
  if(!negationPattern.test(body.slice(Math.max(0,index-16),index)))return true;
 }
 return false;
}

function isVagueSource(source:string):boolean {
 const normalized=source.trim().toLowerCase()
  .replace(/^(?:资料来源|信息来源|来源)\s*[:：]/,'')
  .replace(/[\s。！？!?，,；;：:（）()【】[\]《》<>“”"'‘’_-]/g,'');
 return vagueSources.has(normalized);
}

function isTimeSensitive(claim:DraftClaim,fact:Fact):boolean {
 return timeSensitivePattern.test(fact.label)
  ||timeSensitivePattern.test(fact.value)
  ||temporalContextPattern.test(fact.value)
  ||temporalContextPattern.test(claim.text);
}

/**
 * Deterministic local checks only. This does not replace native review/export gates,
 * verify facts against the outside world, or treat an execution failure as content failure.
 */
export function inspectDraftQuality(draft:Draft,product:Product,brandId:string):QualityInspectionResult {
 const issues:QualityIssue[]=[];
 const claims=draft.claims??[];
 let claimsWithEvidence=0;

 if(draft.brandId!==brandId||product.brandId!==brandId||draft.productId!==product.id){
  issues.push({code:'draft_product_mismatch',severity:'block',message:'稿件与当前品牌或商品不一致，请先核对归属。'});
 }
 if(!claims.length){
  issues.push({code:'no_key_claims',severity:'block',message:'尚未登记关键主张，无法建立正文与事实依据的对应关系。'});
 }

 for(const claim of claims){
  const claimIssue=(code:QualityIssueCode,severity:QualityIssueSeverity,message:string)=>{
   issues.push({code,severity,message,claimId:claim.id});
  };
  if(!claim.text.trim()||!draft.body.includes(claim.text)){
   claimIssue('claim_not_in_body','block','关键主张为空或未逐字出现在正文中，请核对正文与主张记录。');
  }

  const fact=product.facts.find(item=>item.id===claim.factId);
  if(!fact){
   claimIssue('fact_not_found','block','关键主张关联的事实不存在，请重新选择当前商品的事实依据。');
   continue;
  }

  const approved=factIsApproved(fact);
  const hasSource=Boolean(fact.source.trim());
  // Body presence is reported separately; this count describes evidence linkage only.
  if(fact.visibility==='public'&&approved&&hasSource)claimsWithEvidence+=1;
  if(fact.visibility!=='public'){
   claimIssue('fact_not_public','block','关键主张关联了私有事实，不能作为公开内容依据。');
  }
  if(!approved){
   claimIssue('fact_not_approved','block','关键主张关联的事实尚未审核通过，请先完成事实审核。');
  }
  if(!hasSource){
   claimIssue('fact_source_missing','block','关键主张关联的事实缺少来源，请补充可核验的来源信息。');
  }else if(isVagueSource(fact.source)){
   claimIssue('source_vague','review','该主张的来源仅为模糊描述，请补充可定位、可核验的具体来源。');
  }
  if(!fact.validUntil?.trim()&&isTimeSensitive(claim,fact)){
   claimIssue('valid_until_missing','review','该主张涉及时效性信息，但依据事实未填写有效期，请补充并人工核对时效。');
  }
 }

 if(hasUnnegatedMatch(draft.body,absoluteClaimPattern)){
  issues.push({code:'absolute_claim',severity:'review',message:'正文含绝对化或排他性措辞，请人工核对依据并修改无法证实的表达。'});
 }
 if(hasUnnegatedMatch(draft.body,efficacyPromisePattern)){
  issues.push({code:'efficacy_promise',severity:'review',message:'正文含功效或结果承诺，请人工核对适用范围、限制与证据，避免作保证性表达。'});
 }

 return {
  status:issues.some(issue=>issue.severity==='block')?'blocked':issues.length?'review':'pass',
  issues,
  claimsTotal:claims.length,
  claimsWithEvidence
 };
}
