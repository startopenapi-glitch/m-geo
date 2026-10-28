import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport,type Transport} from '../api';
import type {Draft,Product,Publication,WorkspaceSnapshot} from '../types';
import type {PublicationWithLifecycle} from '../publicationStatus';
import Publications from './Publications';

const product:Product={id:'p',brandId:'b',name:'虚构折叠灯',sku:'SIM-LAMP-19',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'色温',value:'4000K',source:'虚构规格表',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构商品稿',body:'色温：4000K',sourceRevision:1,revision:1,generationMode:'local',reviewedAt:'2026-10-24T01:00:00Z',createdAt:'2026-10-24T01:00:00Z'};
const publication:PublicationWithLifecycle={id:'pub',brandId:'b',draftId:'d',draftRevision:1,channel:'品牌官网',url:'http://example.com/sim-lamp',publishedAt:'2026-10-24T02:00:00Z',note:'',contentSnapshot:'# 固定快照',lifecycle:'online',readbacks:[{fetchedAt:'2026-10-24T02:30:00Z',httpStatus:200,finalUrl:'http://example.com/sim-lamp',title:'虚构折叠灯',bodySha256:'a'.repeat(64),draftRevision:1,matchedContent:true,errorCategory:null,transport:'http'}]};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[publication],observations:[]};
const brand={id:'b',name:'虚构回读品牌',createdAt:''};
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('reads back only after an explicit click and shows bounded metadata',async()=>{
 const transport=vi.fn(async()=>publication);
 setTestTransport(transport as Transport);
 render(<Publications brand={brand} data={data} refresh={vi.fn(async()=>{})}/>);
 expect(transport).not.toHaveBeenCalled();
 expect(screen.getByText(/HTTP 公开 URL：连接未加密/)).toBeInTheDocument();
 expect(screen.getByText(new RegExp(`正文 SHA256：${'a'.repeat(64)}`))).toBeInTheDocument();
 expect(screen.queryByText('虚构正文')).not.toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'回读此公开 URL'}));
 await waitFor(()=>expect(transport).toHaveBeenCalledWith('publication.readback',{brandId:'b',id:'pub'}));
 expect(await screen.findByText(/公开 URL 回读结果已保存；不代表平台收录/)).toBeInTheDocument();
});

it('requires withdrawal reason and explicit confirmation',async()=>{
 const transport=vi.fn(async()=>publication);
 setTestTransport(transport as Transport);
 render(<Publications brand={brand} data={{...data,publications:[{...publication,lifecycle:undefined,readbacks:undefined} as Publication]}} refresh={vi.fn(async()=>{})}/>);
 fireEvent.click(screen.getByRole('button',{name:'登记撤回'}));
 const save=screen.getByRole('button',{name:'保存撤回登记'});
 expect(save).toBeDisabled();
 fireEvent.change(screen.getByLabelText('撤回原因与核查说明'),{target:{value:'虚构页面整改，平台缓存仍待人工核查'}});
 fireEvent.change(screen.getByLabelText('撤回状态'),{target:{value:'withdrawal_unverified'}});
 expect(save).toBeDisabled();
 fireEvent.click(screen.getByRole('checkbox'));
 expect(save).toBeEnabled();
 fireEvent.click(save);
 await waitFor(()=>expect(transport).toHaveBeenCalledWith('publication.withdraw',{brandId:'b',id:'pub',status:'withdrawal_unverified',reason:'虚构页面整改，平台缓存仍待人工核查',confirm:true}));
 expect(await screen.findByText(/撤回登记已保存；不代表远端已删除/)).toBeInTheDocument();
});

it('does not offer ordinary readback after withdrawal',()=>{
 render(<Publications brand={brand} data={{...data,publications:[{...publication,lifecycle:'withdrawn',withdrawal:{status:'withdrawn',reason:'虚构人工撤回',requestedAt:'2026-10-24T03:00:00Z',updatedAt:'2026-10-24T03:00:00Z'}} as Publication]}} refresh={vi.fn(async()=>{})}/>);
 expect(screen.getByRole('button',{name:'回读此公开 URL'})).toBeDisabled();
 expect(screen.getByText('已撤回（人工登记）',{selector:'.badge'})).toBeInTheDocument();
});
