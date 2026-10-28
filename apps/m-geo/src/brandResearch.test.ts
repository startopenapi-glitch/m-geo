import {afterEach,describe,expect,it,vi} from 'vitest';
import {setTestTransport} from './api';
import {aliasesFromText,confirmResearchToQuestion,listResearch,normalizeResearchQuestion,researchDuplicateKey,saveBrandProfile} from './brandResearch';

afterEach(()=>{setTestTransport(undefined);vi.clearAllMocks()});

describe('brand research helpers',()=>{
 it('normalizes conservative duplicate forms without claiming semantic matching',()=>{
  expect(normalizeResearchQuestion(' 请问 Ｎｅｂｕｌａ 怎么清洗吗？ ')).toBe('nebula怎么清洗');
  expect(normalizeResearchQuestion('nebula 怎么清洗呢')).toBe('nebula怎么清洗');
  expect(normalizeResearchQuestion('怎么清洗')).not.toBe(normalizeResearchQuestion('怎么保养'));
  expect(researchDuplicateKey(null,'A')).not.toBe(researchDuplicateKey('product','A'));
 });
 it('deduplicates aliases case insensitively and ignores blanks',()=>{
  expect(aliasesFromText('星河，star river\n星河\nSTAR RIVER,, 星 河')).toEqual(['星河','star river','星 河']);
 });
});

describe('brand research transport contract',()=>{
 it('saves only profile fields and omits updatedAt',async()=>{
  const calls:{action:string;payload:Record<string,unknown>}[]=[];
  setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});return {id:'b',name:'虚构品牌',createdAt:'',profile:{...payload,updatedAt:'2026-01-01T00:00:00Z'}} as T});
  await saveBrandProfile({brandId:'b',aliases:['星河'],officialUrl:'https://example.com',manufacturer:'虚构厂商',category:'虚构分类',description:'虚构介绍'});
  expect(calls).toEqual([{action:'brand.profile.save',payload:{brandId:'b',aliases:['星河'],officialUrl:'https://example.com',manufacturer:'虚构厂商',category:'虚构分类',description:'虚构介绍'}}]);
 });
 it('omits absent list filters and keeps confirmToQuestion revision',async()=>{
  const calls:{action:string;payload:Record<string,unknown>}[]=[];
  setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});return (action==='research.list'?{items:[],clusters:[],truncated:false}:{research:{},question:{confirmed:true}}) as T});
  await listResearch({brandId:'b',status:'confirmed',productId:undefined,query:undefined});
  await confirmResearchToQuestion({brandId:'b',id:'r',expectedRevision:4});
  expect(calls).toEqual([
   {action:'research.list',payload:{brandId:'b',status:'confirmed'}},
   {action:'research.confirmToQuestion',payload:{brandId:'b',id:'r',expectedRevision:4}},
  ]);
 });
});
