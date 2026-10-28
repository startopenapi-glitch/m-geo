import {describe,expect,it} from 'vitest';
import {geoWorkflow} from './workflow';
import type {Draft,Observation,Product,Question,WorkspaceSnapshot} from './types';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'DEMO-1',category:'水杯',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-01-01T00:00:00Z',updatedAt:'2026-01-01T00:00:00Z',facts:[{id:'f',label:'容量',value:'500 mL',source:'虚构规格书',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-01-01T00:00:00Z'}]};
const question:Question={id:'q',brandId:'b',productId:'p',text:'虚构水杯容量是多少？',intent:'selection',origin:'customer',source:'虚构咨询记录',branded:false,confirmed:true};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:'q',kind:'description',title:'虚构水杯',body:'容量为 500 mL。',claims:[{id:'c',text:'500 mL',factId:'f'}],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:'2026-01-01T00:00:00Z',createdAt:'2026-01-01T00:00:00Z'};
const observation:Observation={id:'o',brandId:'b',productId:'p',questionId:'q',querySnapshot:question.text,branded:false,platform:'doubao',surface:'consumer',phase:'baseline',modelLabel:'模拟模型',searchMode:'模拟',testedAt:'2026-01-01T00:00:00Z',answer:'模拟回答',mentioned:true,accuracy:'correct',citedUrl:'',evidenceNote:'模拟数据'};
const empty:WorkspaceSnapshot={products:[],questions:[],drafts:[],publications:[],observations:[]};

describe('GEO workflow',()=>{
 it('does not let later records skip an incomplete factual foundation',()=>{
  const result=geoWorkflow({...empty,observations:[observation]},'b');
  expect(result.next).toMatchObject({step:'facts',target:'products'});
  expect(result.steps.map(step=>step.id)).toEqual(['facts','questions','content','test','compare','optimize']);
 });

 it('requires confirmed questions before recommending a first draft',()=>{
  const unconfirmed=geoWorkflow({...empty,products:[product],questions:[{...question,confirmed:false}]},'b');
  expect(unconfirmed.next).toMatchObject({step:'questions',target:'pilot'});
  const confirmed=geoWorkflow({...empty,products:[product],questions:[question]},'b');
  expect(confirmed.next).toMatchObject({step:'content',target:'content'});
 });

 it('moves from reviewed content to comparison and then optimization',()=>{
  const baseline=geoWorkflow({...empty,products:[product],questions:[question],drafts:[draft],observations:[observation]},'b');
  expect(baseline.next).toMatchObject({step:'compare',target:'visibility'});
  expect(baseline.steps.find(step=>step.id==='content')?.complete).toBe(true);
  const followup=geoWorkflow({...empty,products:[product],questions:[question],drafts:[draft],observations:[observation,{...observation,id:'o2',phase:'followup'}]},'b');
  expect(followup.next).toMatchObject({step:'optimize',target:'optimization'});
 });
});
