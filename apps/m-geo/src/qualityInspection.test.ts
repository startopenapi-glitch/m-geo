import {describe,expect,it} from 'vitest';
import {inspectDraftQuality,localQualityInspectionDisclaimer} from './qualityInspection';
import type {Draft,Fact,Product} from './types';

const approvedFact=(overrides:Partial<Fact>={}):Fact=>({
 id:'fact-capacity',
 label:'容量',
 value:'500 mL',
 source:'虚构品牌《杯具规格书》第 3 页',
 visibility:'public',
 validUntil:null,
 reviewStatus:'approved',
 reviewedAt:'2026-01-01T00:00:00Z',
 updatedAt:'2026-01-01T00:00:00Z',
 ...overrides
});

const makeProduct=(facts:Fact[]=[approvedFact()],overrides:Partial<Product>={}):Product=>({
 id:'product-demo',
 brandId:'brand-demo',
 name:'虚构星野保温杯',
 sku:'FICTIONAL-DEMO-001',
 category:'测试用品',
 officialUrl:'https://example.test/products/demo',
 sourceText:'虚构商品资料',
 privateNotes:'',
 facts,
 revision:1,
 confirmedAt:'2026-01-01T00:00:00Z',
 updatedAt:'2026-01-01T00:00:00Z',
 ...overrides
});

const makeDraft=(overrides:Partial<Draft>={}):Draft=>({
 id:'draft-demo',
 brandId:'brand-demo',
 productId:'product-demo',
 questionId:null,
 kind:'description',
 title:'虚构星野保温杯介绍',
 body:'容量为 500 mL。',
 claims:[{id:'claim-capacity',text:'500 mL',factId:'fact-capacity'}],
 sourceRevision:1,
 revision:1,
 generationMode:'local',
 reviewedAt:null,
 createdAt:'2026-01-01T00:00:00Z',
 ...overrides
});

const codes=(draft:Draft,product:Product=makeProduct())=>inspectDraftQuality(draft,product,'brand-demo').issues.map(issue=>issue.code);

describe('local draft quality inspection',()=>{
 it('passes only when local rules find no issue and keeps the human-review disclaimer explicit',()=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct(),'brand-demo');
  expect(result).toEqual({status:'pass',issues:[],claimsTotal:1,claimsWithEvidence:1});
  expect(localQualityInspectionDisclaimer).toContain('不能替代人工审核');
  expect(localQualityInspectionDisclaimer).toContain('不代表事实已核实');
 });

 it.each([
  ['draft belongs to another brand',makeDraft({brandId:'other-brand'}),makeProduct()],
  ['product belongs to another brand',makeDraft(),makeProduct([approvedFact()],{brandId:'other-brand'})],
  ['draft points to another product',makeDraft({productId:'other-product'}),makeProduct()]
 ])('blocks when %s',(_name,draft,product)=>{
  const result=inspectDraftQuality(draft,product,'brand-demo');
  expect(result.status).toBe('blocked');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'draft_product_mismatch',severity:'block'}));
 });

 it.each([undefined,[]])('blocks drafts without key claims',claims=>{
  const result=inspectDraftQuality(makeDraft({claims}),makeProduct(),'brand-demo');
  expect(result).toMatchObject({status:'blocked',claimsTotal:0,claimsWithEvidence:0});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'no_key_claims',severity:'block'}));
 });

 it.each(['','   ','500 毫升'])('blocks blank or non-verbatim claim text: %j',text=>{
  const result=inspectDraftQuality(makeDraft({claims:[{id:'claim-capacity',text,factId:'fact-capacity'}]}),makeProduct(),'brand-demo');
  expect(result.status).toBe('blocked');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'claim_not_in_body',claimId:'claim-capacity'}));
 });

 it('requires exact body text rather than normalizing away meaningful whitespace',()=>{
  expect(codes(makeDraft({claims:[{id:'claim-capacity',text:'500 mL ',factId:'fact-capacity'}]}))).toContain('claim_not_in_body');
 });

 it('blocks a claim whose fact does not exist',()=>{
  const result=inspectDraftQuality(makeDraft({claims:[{id:'claim-capacity',text:'500 mL',factId:'missing-fact'}]}),makeProduct(),'brand-demo');
  expect(result).toMatchObject({status:'blocked',claimsTotal:1,claimsWithEvidence:0});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'fact_not_found',claimId:'claim-capacity'}));
 });

 it('blocks private facts even when every other field is complete',()=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct([approvedFact({visibility:'private'})]),'brand-demo');
  expect(result).toMatchObject({status:'blocked',claimsWithEvidence:0});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'fact_not_public',claimId:'claim-capacity'}));
 });

 it.each(['pending','rejected'] as const)('blocks facts with review status %s',reviewStatus=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct([approvedFact({reviewStatus})]),'brand-demo');
  expect(result.status).toBe('blocked');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'fact_not_approved',claimId:'claim-capacity'}));
 });

 it('keeps legacy facts without a review status compatible with the native approved default',()=>{
  const legacy=approvedFact({reviewStatus:undefined,reviewedAt:null});
  expect(inspectDraftQuality(makeDraft(),makeProduct([legacy]),'brand-demo').status).toBe('pass');
 });

 it.each(['','   '])('blocks facts without a source: %j',source=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct([approvedFact({source})]),'brand-demo');
  expect(result).toMatchObject({status:'blocked',claimsWithEvidence:0});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'fact_source_missing',claimId:'claim-capacity'}));
 });

 it('counts evidence linkage independently from body placement and source specificity',()=>{
  const draft=makeDraft({body:'容量为 500 mL。',claims:[
   {id:'claim-good',text:'500 mL',factId:'fact-capacity'},
   {id:'claim-missing-body',text:'不在正文的主张',factId:'fact-vague'},
   {id:'claim-private',text:'500 mL',factId:'fact-private'}
  ]});
  const product=makeProduct([
   approvedFact(),
   approvedFact({id:'fact-vague',source:'网络'}),
   approvedFact({id:'fact-private',visibility:'private'})
  ]);
  const result=inspectDraftQuality(draft,product,'brand-demo');
  expect(result).toMatchObject({status:'blocked',claimsTotal:3,claimsWithEvidence:2});
  expect(codes(draft,product)).toEqual(expect.arrayContaining(['claim_not_in_body','fact_not_public','source_vague']));
 });

 it.each(['全网第一','100%有效','百分之百安全','绝对安全','永久有效','最安全','零风险'])('flags absolute wording for review: %s',wording=>{
  const draft=makeDraft({body:`容量为 500 mL。${wording}。`});
  const result=inspectDraftQuality(draft,makeProduct(),'brand-demo');
  expect(result.status).toBe('review');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'absolute_claim',severity:'review'}));
 });

 it.each(['保证有效','确保七天见效','承诺减肥','三天内见效','药到病除','包治百病'])('flags efficacy promises for review: %s',wording=>{
  const draft=makeDraft({body:`容量为 500 mL。${wording}。`});
  const result=inspectDraftQuality(draft,makeProduct(),'brand-demo');
  expect(result.status).toBe('review');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'efficacy_promise',severity:'review'}));
 });

 it.each(['并非全网第一','不是绝对安全','不代表零风险','不保证有效','不能保证七天见效'])('does not flag clearly negated wording: %s',wording=>{
  const draft=makeDraft({body:`容量为 500 mL。${wording}。`});
  expect(inspectDraftQuality(draft,makeProduct(),'brand-demo').status).toBe('pass');
 });

 it('does not treat ordinary wording as an absolute or efficacy promise',()=>{
  const draft=makeDraft({body:'容量为 500 mL。这里介绍清洁杯具的最佳实践，有效期说明见规格书。'});
  expect(inspectDraftQuality(draft,makeProduct(),'brand-demo').status).toBe('pass');
 });

 it('flags time-sensitive price information without a validity period',()=>{
  const fact=approvedFact({id:'fact-price',label:'活动价',value:'199 元',validUntil:null});
  const draft=makeDraft({body:'活动价为 199 元。',claims:[{id:'claim-price',text:'199 元',factId:'fact-price'}]});
  const result=inspectDraftQuality(draft,makeProduct([fact]),'brand-demo');
  expect(result).toMatchObject({status:'review',claimsWithEvidence:1});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'valid_until_missing',claimId:'claim-price'}));
 });

 it('accepts a supplied validity period without deciding whether it is currently valid',()=>{
  const fact=approvedFact({id:'fact-price',label:'活动价',value:'199 元',validUntil:'2026-12-31'});
  const draft=makeDraft({body:'活动价为 199 元。',claims:[{id:'claim-price',text:'199 元',factId:'fact-price'}]});
  expect(inspectDraftQuality(draft,makeProduct([fact]),'brand-demo').status).toBe('pass');
 });

 it('flags temporal wording even when the fact label is otherwise stable',()=>{
  const draft=makeDraft({body:'目前容量为 500 mL。',claims:[{id:'claim-capacity',text:'目前容量为 500 mL',factId:'fact-capacity'}]});
  expect(codes(draft)).toContain('valid_until_missing');
 });

 it('does not demand a validity period for stable capacity information',()=>{
  expect(inspectDraftQuality(makeDraft(),makeProduct(),'brand-demo').issues).toEqual([]);
 });

 it.each(['网络','来源：官网','品牌提供','内部资料','AI生成'])('flags vague sources for review: %s',source=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct([approvedFact({source})]),'brand-demo');
  expect(result).toMatchObject({status:'review',claimsWithEvidence:1});
  expect(result.issues).toContainEqual(expect.objectContaining({code:'source_vague',severity:'review',claimId:'claim-capacity'}));
 });

 it.each(['虚构品牌《杯具规格书 2026 版》第 3 页','虚构实验室报告 DEMO-2026-01','https://example.test/specs/demo-v1'])('does not flag locatable sources: %s',source=>{
  const result=inspectDraftQuality(makeDraft(),makeProduct([approvedFact({source})]),'brand-demo');
  expect(result.status).toBe('pass');
 });

 it('gives blocking findings precedence over review reminders',()=>{
  const draft=makeDraft({body:'容量为 500 mL。全网第一，保证有效。',claims:[{id:'claim-capacity',text:'500 mL',factId:'missing-fact'}]});
  const result=inspectDraftQuality(draft,makeProduct(),'brand-demo');
  expect(result.status).toBe('blocked');
  expect(result.issues.map(issue=>issue.severity)).toEqual(expect.arrayContaining(['block','review']));
 });

 it('is deterministic and does not mutate its inputs',()=>{
  const draft=makeDraft({body:'容量为 500 mL。全网第一。'});
  const product=makeProduct();
  const draftBefore=structuredClone(draft);
  const productBefore=structuredClone(product);
  const first=inspectDraftQuality(draft,product,'brand-demo');
  const second=inspectDraftQuality(draft,product,'brand-demo');
  expect(second).toEqual(first);
  expect(draft).toEqual(draftBefore);
  expect(product).toEqual(productBefore);
 });
});
