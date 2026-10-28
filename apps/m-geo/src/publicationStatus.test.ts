import {describe,expect,it} from 'vitest';
import {publicationStatus} from './publicationStatus';
import type {Draft,Product,Publication,WorkspaceSnapshot} from './types';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'容量',value:'500mL',source:'虚构规格表',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',channel:'jd',channelTemplateVersion:1,title:'虚构京东稿',body:'容量：500mL',claims:[{id:'c',text:'容量：500mL',factId:'f'}],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:'2026-10-24T01:00:00Z',createdAt:'2026-10-24T01:00:00Z'};
const publication:Publication={id:'pub',brandId:'b',draftId:'d',draftRevision:1,channel:'京东',url:'https://example.com/published',publishedAt:'2026-10-24T02:00:00Z',note:'',contentSnapshot:'# 固定快照'};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[publication],observations:[]};
const at=new Date('2026-10-24T03:00:00Z');

describe('publication version status',()=>{
 it('marks an unchanged reviewed version current',()=>expect(publicationStatus(data,publication,'b',at)).toBe('current'));
 it('marks an older published draft as having a newer draft',()=>expect(publicationStatus({...data,drafts:[{...draft,revision:2}]},publication,'b',at)).toBe('newer_draft'));
 it('prioritizes product changes over a newer draft',()=>expect(publicationStatus({...data,products:[{...product,revision:2}],drafts:[{...draft,revision:2}]},publication,'b',at)).toBe('product_changed'));
 it('marks expired facts for review even without a revision change',()=>expect(publicationStatus({...data,products:[{...product,facts:[{...product.facts[0],validUntil:'2026-10-23'}]}]},publication,'b',at)).toBe('product_changed'));
 it('does not guess when historical records are absent from a truncated snapshot',()=>expect(publicationStatus({...data,drafts:[]},publication,'b',at)).toBe('historical'));
});
