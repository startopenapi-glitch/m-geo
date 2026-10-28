import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Draft,Product,Publication,WorkspaceSnapshot} from '../types';
import Publications from './Publications';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'容量',value:'500mL',source:'虚构规格表',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',channel:'jd',channelTemplateVersion:1,title:'虚构京东稿',body:'容量：500mL',claims:[{id:'c',text:'容量：500mL',factId:'f'}],sourceRevision:1,revision:2,generationMode:'local',reviewedAt:'2026-10-24T01:00:00Z',createdAt:'2026-10-24T01:00:00Z'};
const publication:Publication={id:'pub',brandId:'b',draftId:'d',draftRevision:1,channel:'京东',url:'https://example.com/published',publishedAt:'2026-10-24T02:00:00Z',note:'人工登记',contentSnapshot:'# 固定快照'};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[publication],observations:[]};
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('shows SKU and version status and defaults the actual channel from the draft',()=>{
 setTestTransport(async()=>{throw Error('不应发起网络或发布请求')});
 render(<Publications brand={{id:'b',name:'虚构品牌',createdAt:''}} data={data} refresh={vi.fn(async()=>{})}/>);
 expect(screen.getByText(/SKU：SIM-CUP/)).toBeInTheDocument();
 expect(screen.getByText('有新稿件',{selector:'.badge'})).toBeInTheDocument();
 expect(screen.getByText(/发布稿件 v1 · 当前稿件 v2/)).toBeInTheDocument();
 fireEvent.change(screen.getByLabelText('已审核稿件'),{target:{value:'d'}});
 expect(screen.getByLabelText('实际发布渠道')).toHaveValue('京东');
 expect(screen.getByText(/不会联网核查，也不代表平台已抓取、推荐或引用/)).toBeInTheDocument();
 fireEvent.change(screen.getByLabelText('状态筛选'),{target:{value:'current'}});
 expect(screen.getByText('当前筛选下没有记录')).toBeInTheDocument();
});
