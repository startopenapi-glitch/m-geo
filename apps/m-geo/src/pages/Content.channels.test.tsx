import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Draft,Product,WorkspaceSnapshot} from '../types';
import Content from './Content';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'容量',value:'500mL',source:'虚构规格表',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[],publications:[],observations:[]};
const refresh=vi.fn(async()=>{});
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('generates several local channel drafts in one transaction without starting AI',async()=>{
 const actions:{action:string;payload:Record<string,unknown>}[]=[];
 const drafts:Draft[]=['jd','xiaohongshu'].map((channel,index)=>({id:`d${index}`,brandId:'b',productId:'p',questionId:null,kind:index?'guide':'description',channel:channel as 'jd'|'xiaohongshu',channelTemplateVersion:1,title:`渠道稿 ${index}`,body:'容量：500mL',claims:[{id:`c${index}`,text:'容量：500mL',factId:'f'}],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:null,createdAt:''}));
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{actions.push({action,payload});if(action==='ai.jobs')return [] as T;if(action==='draft.generateChannels')return {brandId:'b',drafts} as T;throw Error(`unexpected ${action}`)});
 render(<Content brand={{id:'b',name:'虚构品牌',createdAt:''}} data={data} refresh={refresh}/>);
 fireEvent.change(screen.getByLabelText('用于生成的商品'),{target:{value:'p'}});
 fireEvent.click(screen.getByLabelText('京东 · 商品参数与详情'));
 fireEvent.click(screen.getByLabelText('小红书 · 场景选购与商品笔记'));
 expect(screen.queryByLabelText('生成方式')).toBeNull();
 expect(screen.getByText(/不调用模型、不连接平台账号、不自动发布/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'生成 2 个渠道草稿'}));
 await screen.findByText(/已生成 2 个渠道草稿/);
 await waitFor(()=>expect(refresh).toHaveBeenCalledTimes(1));
 expect(actions.filter(item=>item.action==='draft.generateChannels')).toEqual([{action:'draft.generateChannels',payload:{brandId:'b',productId:'p',channels:['jd','xiaohongshu']}}]);
 expect(actions.some(item=>item.action==='ai.start')).toBe(false);
});

it('keeps the original generic local payload when no channel is selected',async()=>{
 const actions:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{actions.push({action,payload});if(action==='ai.jobs')return [] as T;if(action==='draft.generateLocal')return {id:'generic'} as T;throw Error(`unexpected ${action}`)});
 render(<Content brand={{id:'b',name:'虚构品牌',createdAt:''}} data={data} refresh={refresh}/>);
 fireEvent.change(screen.getByLabelText('用于生成的商品'),{target:{value:'p'}});
 fireEvent.click(screen.getByRole('button',{name:'生成本地草稿'}));
 await screen.findByText('本地草稿已生成，请核对正文');
 expect(actions.find(item=>item.action==='draft.generateLocal')?.payload).toEqual({brandId:'b',productId:'p',kind:'description',questionId:null});
 expect(actions.some(item=>item.action==='draft.generateChannels')).toBe(false);
});
