import {describe,it,expect} from 'vitest';
import {buyingGaps,contentWarnings,compareSamples} from './commerce';
import type {Product,Observation} from './types';
const p={id:'p',facts:[],confirmedAt:null} as unknown as Product;
describe('commerce decisions',()=>{
 it('does not invent buying information or call unknown information complete',()=>{
  expect(buyingGaps(p)).toContain('价格');
  expect(buyingGaps({...p,facts:[{id:'commerce:price',label:'价格',value:'19.90',source:'',visibility:'public',validUntil:null}]})).toContain('价格');
 });
 it('flags claim language for human checking without calling it verified',()=>{
  expect(contentWarnings('全网第一，100%有效',p).join(' ')).toContain('绝对化');
 });
 it('separates unknown, negative and incomparable recommendations',()=>{
  const o={productId:'p',querySnapshot:'杯子怎么选',platform:'doubao',surface:'consumer',modelLabel:'v1',searchMode:'联网',branded:false,phase:'baseline',mentioned:true,citedUrl:''} as Observation;
  const rows=[o,{...o,id:'b',assessment:{campaignId:null,repetition:1,context:'新对话',recommendation:'no',rank:null,competitors:''}},{...o,id:'c',assessment:{campaignId:null,repetition:1,context:'新对话',recommendation:'yes',rank:1,competitors:''}}] as Observation[];
  const groups=compareSamples(rows);expect(groups).toHaveLength(2);
  expect(groups.find(g=>g.baseline.assessed===2)?.baseline.recommended).toBe(1);
  expect(compareSamples([])).toEqual([]);
 });
});
