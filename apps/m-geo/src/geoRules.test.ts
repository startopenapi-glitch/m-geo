import {describe,expect,it} from 'vitest';
import {GEO_RULE_PACK_VERSION,cleanModelOutput,inspectGeoModelOutput,inspectGeoStructure,resolveGeoSections} from './geoRules';
import type {Kind} from './types';

const summary='答案摘要\n虚构星野杯容量为 500 mL。';
const facts='事实要点\n- 容量：500 mL。';
const conditions='适用条件\n仅适用于虚构星野杯 2026 款。';
const usage='使用说明\n装水前确认杯盖已清洁。';
const faq='常见问题\n问：可以装碳酸饮料吗？\n答：不建议，详见虚构产品手册。';
const sources='来源\n虚构星野品牌《杯具规格书 DEMO-1》第 3 页。';
const fullBody=[summary,facts,conditions,usage,faq,sources].join('\n\n');
const draft=(body=fullBody,kind:Kind='guide',channel:'jd'|'douyin'|null=null)=>({body,kind,channel});

 describe('GEO structure profiles',()=>{
 it('does not force all six sections onto an ordinary product description',()=>{
  const result=inspectGeoStructure(draft([summary,facts,sources].join('\n\n'),'description'));
  expect(result).toMatchObject({status:'pass',changed:false,rulePackVersion:GEO_RULE_PACK_VERSION});
  expect(result.requiredSections).toEqual(['answer_summary','key_facts','sources']);
 });

 it('accepts a single-brand comparison-intent draft without competitor content',()=>{
  const body=[summary,facts,conditions,sources].join('\n\n');
  const result=inspectGeoStructure(draft(body,'comparison'),'comparison');
  expect(result.status).toBe('pass');
  expect(result.requiredSections).not.toContain('usage_guidance');
  expect(result.requiredSections).not.toContain('faq');
  expect(body).not.toContain('竞品');
 });

 it('uses different requirements for usage and after-sales intents',()=>{
  const usageResult=inspectGeoStructure(draft([summary,conditions,usage,sources].join('\n\n')),'usage');
  expect(usageResult.status).toBe('pass');
  expect(usageResult.requiredSections).not.toContain('key_facts');
  const afterSales=inspectGeoStructure(draft([summary,conditions,usage,faq,sources].join('\n\n')),'after_sales');
  expect(afterSales.status).toBe('pass');
  expect(afterSales.requiredSections).toContain('faq');
 });

 it('accepts question-answer formatting for a FAQ-only structure',()=>{
  const body='问：虚构杯垫如何清洁？\n答：用湿布擦拭。\n\n'+sources;
  expect(inspectGeoStructure(draft(body,'faq')).status).toBe('pass');
 });

 it('blocks only the sections missing from the active profile',()=>{
  const result=inspectGeoStructure(draft([summary,sources].join('\n\n'),'description'));
  expect(result.status).toBe('block');
  expect(result.issues).toEqual([expect.objectContaining({code:'missing_required_section',section:'key_facts',severity:'block'})]);
 });

 it('does not count an empty heading as section content',()=>{
  const body=[summary,'事实要点\n\n'+sources].join('\n\n');
  expect(inspectGeoStructure(draft(body,'description')).issues).toContainEqual(expect.objectContaining({section:'key_facts',severity:'block'}));
 });

 it('adapts source-appendix requirements for short-form commerce channels',()=>{
  const body=[summary,facts].join('\n\n');
  expect(inspectGeoStructure(draft(body,'description','douyin')).status).toBe('pass');
  expect(inspectGeoStructure(draft(body,'description','jd')).status).toBe('block');
 });

 it('supports explicit channel overrides without changing other profiles',()=>{
  const config={requirements:{byChannel:{jd:{requiredSections:['key_facts'] as ['key_facts']}}}};
  expect(resolveGeoSections('description','general','jd',config)).toEqual(['key_facts']);
  expect(resolveGeoSections('description','general','tmall',config)).toContain('sources');
  expect(inspectGeoStructure(draft(facts,'description','jd'),'general',config).status).toBe('pass');
 });

 it('adds FAQ for manual public QA without treating it as automatic publishing',()=>{
  expect(resolveGeoSections('description','general','public_qa_manual')).toContain('faq');
 });

 it('blocks an empty body and retains an explanatory trace',()=>{
  const result=inspectGeoStructure(draft('   ','description'));
  expect(result.status).toBe('block');
  expect(result.issues.map(issue=>issue.code)).toContain('empty_body');
  expect(result.trace.length).toBeGreaterThan(1);
 });
});

describe('conservative model output cleaning',()=>{
 it('cleans only recognized outer prefaces, wrappers and standalone metadata',()=>{
  const input=`好的，\n\`\`\`markdown\n以下是生成的内容：\n已发布到官网。\nguaranteed ranking\n${[summary,facts,sources].join('\n\n')}\n\`\`\``;
  const result=cleanModelOutput(input);
  expect(result.status).toBe('auto-clean');
  expect(result.changed).toBe(true);
  expect(result.body).toBe([summary,facts,sources].join('\n\n'));
  expect(result.issues.map(issue=>issue.code)).toEqual(expect.arrayContaining(['model_preface_removed','code_fence_removed','publish_meta_removed']));
 });

 it('retains embedded publication or ranking claims for review instead of deleting substance',()=>{
  const input=summary+' 已发布，guaranteed ranking，请直接采用。';
  const result=cleanModelOutput(input);
  expect(result.body).toBe(input);
  expect(result.changed).toBe(false);
  expect(result.status).toBe('review');
  expect(result.issues).toContainEqual(expect.objectContaining({code:'unsafe_meta_retained',severity:'review'}));
 });

 it.each(['厂家保证两年内免费维修。','产品承诺依照虚构保修政策处理。','容量为 500 mL，来源待人工审核。'])('does not erase substantive commitments or unsupported claims: %s',input=>{
  const result=cleanModelOutput(input);
  expect(result.body).toBe(input);
  expect(result.changed).toBe(false);
  expect(result.trace).toContainEqual(expect.objectContaining({rule:'native_evidence_review',outcome:'skipped'}));
 });

 it('leaves inner code fences and ordinary markdown untouched',()=>{
  const input='答案摘要\n示例如下：\n```text\n虚构示例\n```';
  expect(cleanModelOutput(input).body).toBe(input);
 });

 it('inspects the cleaned body and reports auto-clean only when no higher severity remains',()=>{
  const result=inspectGeoModelOutput(draft('好的，\n'+[summary,facts,sources].join('\n\n'),'description'));
  expect(result).toMatchObject({status:'auto-clean',changed:true,intent:'general'});
  expect(result.body).not.toContain('好的');
  expect(result.issues.some(issue=>issue.severity==='block')).toBe(false);
 });

 it('gives blocking structure findings precedence over auto-clean findings',()=>{
  const result=inspectGeoModelOutput(draft('好的，\n'+summary,'description'));
  expect(result.status).toBe('block');
  expect(result.changed).toBe(true);
  expect(result.issues.map(issue=>issue.severity)).toEqual(expect.arrayContaining(['auto-clean','block']));
 });

 it('is deterministic and does not mutate its draft input',()=>{
  const input=draft('好的，\n'+fullBody);
  const before={...input};
  const first=inspectGeoModelOutput(input);
  expect(inspectGeoModelOutput(input)).toEqual(first);
  expect(input).toEqual(before);
  expect(first.trace.some(entry=>entry.rule==='native_evidence_review')).toBe(true);
 });
});
