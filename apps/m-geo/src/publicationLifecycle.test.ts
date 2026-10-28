import {describe,expect,it} from 'vitest';
import {latestReadback,publicationDisplayStatus,publicationLifecycle,publicationStatus,type PublicationWithLifecycle} from './publicationStatus';
import type {Draft,Product,WorkspaceSnapshot} from './types';

const product:Product={id:'p',brandId:'b',name:'虚构折叠灯',sku:'SIM-LAMP-19',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'色温',value:'4000K',source:'虚构规格表',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构商品稿',body:'色温：4000K',sourceRevision:1,revision:1,generationMode:'local',reviewedAt:'2026-10-24T01:00:00Z',createdAt:'2026-10-24T01:00:00Z'};
const publication:PublicationWithLifecycle={id:'pub',brandId:'b',draftId:'d',draftRevision:1,channel:'品牌官网',url:'https://example.com/sim-lamp',publishedAt:'2026-10-24T02:00:00Z',note:'',contentSnapshot:'# 固定快照'};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[publication],observations:[]};
const at=new Date('2026-10-24T03:00:00Z');

describe('publication lifecycle state machine',()=>{
 it('distinguishes registration, transient pending, online, changed and unreachable',()=>{
  expect(publicationLifecycle(publication)).toBe('registered');
  expect(publicationLifecycle(publication,true)).toBe('pending_readback');
  for(const lifecycle of ['online','content_changed','unreachable'] as const)expect(publicationLifecycle({...publication,lifecycle} as PublicationWithLifecycle)).toBe(lifecycle);
 });
 it('keeps withdrawal authoritative over pending and earlier online state',()=>{
  const withdrawn:PublicationWithLifecycle={...publication,lifecycle:'online',withdrawal:{status:'withdrawn',reason:'虚构人工撤回',requestedAt:'2026-10-24T03:00:00Z',updatedAt:'2026-10-24T03:00:00Z'}};
  expect(publicationLifecycle(withdrawn,true)).toBe('withdrawn');
  expect(publicationDisplayStatus(data,withdrawn,'b',true,at)).toBe('withdrawn');
 });
 it('keeps local version review separate from public URL state',()=>{
  const online={...publication,lifecycle:'online' as const,readbacks:[{fetchedAt:'2026-10-24T03:00:00Z',httpStatus:200,finalUrl:publication.url,title:'虚构折叠灯',bodySha256:'a'.repeat(64),draftRevision:1,matchedContent:true,errorCategory:null,transport:'https' as const}]};
  expect(publicationDisplayStatus(data,online,'b',false,at)).toBe('online');
  expect(publicationStatus({...data,drafts:[{...draft,revision:2}]},online,'b',at)).toBe('newer_draft');
  expect(latestReadback(online)?.bodySha256).toBe('a'.repeat(64));
 });
 it('preserves legacy version filtering when no lifecycle fields exist',()=>expect(publicationDisplayStatus(data,publication,'b',false,at)).toBe('current'));
});
