import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {PageProps} from '../App';
import type {ContentAsset} from '../assetTypes';
import AssetLibrary from './AssetLibrary';

const product={id:'p',brandId:'b',name:'虚构保温杯',sku:'ASSET-CUP',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
const props:PageProps={brand:{id:'b',name:'虚构品牌',createdAt:''},data:{products:[product],questions:[],drafts:[],observations:[],publications:[]},refresh:vi.fn(async()=>{})};
function asset(overrides:Partial<ContentAsset>={}):ContentAsset{return {id:'asset-b',brandId:'b',kind:'keyword',productId:'p',text:'虚构通勤关键词',source:'虚构人工资料',reviewStatus:'pending',reviewNote:'',reviewedAt:null,revision:3,createdAt:'2026-10-01T08:00:00Z',updatedAt:'2026-10-03T08:00:00Z',history:[{revision:1,action:'created',text:'虚构初版关键词',source:'虚构人工资料',productId:'p',reviewStatus:'pending',reviewNote:'',reviewedAt:null,changedAt:'2026-10-01T08:00:00Z'},{revision:2,action:'approved',text:'虚构初版关键词',source:'虚构人工资料',productId:'p',reviewStatus:'approved',reviewNote:'',reviewedAt:'2026-10-02T08:00:00Z',changedAt:'2026-10-02T08:00:00Z'},{revision:3,action:'revised',text:'虚构通勤关键词',source:'虚构人工资料',productId:'p',reviewStatus:'pending',reviewNote:'',reviewedAt:null,changedAt:'2026-10-03T08:00:00Z'}],...overrides}}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('loads_assets_and_filters_by_kind_without_treating_them_as_product_facts',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});if(action==='asset.list')return (payload.kind?[asset({kind:'title',text:'虚构标题'})]:[asset()]) as T;throw Error(action)});
 render(<AssetLibrary {...props}/>);
 expect(await screen.findByRole('button',{name:/虚构通勤关键词/})).toBeInTheDocument();
 expect(screen.getByText(/仅供人工复用，不是已审核商品事实/)).toBeInTheDocument();
 fireEvent.change(screen.getByLabelText('资产类型筛选'),{target:{value:'title'}});
 expect(await screen.findByRole('button',{name:/虚构标题/})).toBeInTheDocument();
 expect(calls).toEqual([{action:'asset.list',payload:{brandId:'b'}},{action:'asset.list',payload:{brandId:'b',kind:'title'}}]);
});

it('creates_pending_asset_with_stable_request_id_and_does_not_review_or_publish',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});if(action==='asset.list')return [] as T;
  if(action==='asset.create')return asset({id:'created',kind:'title',text:payload.text as string,source:payload.source as string,productId:null,revision:1,history:[{revision:1,action:'created',text:payload.text as string,source:payload.source as string,productId:null,reviewStatus:'pending',reviewNote:'',reviewedAt:null,changedAt:'2026-10-04T08:00:00Z'}]}) as T;
  throw Error(action);
 });
 render(<AssetLibrary {...props}/>);await screen.findByText('还没有内容资产');
 fireEvent.change(screen.getByLabelText('资产类型'),{target:{value:'title'}});
 fireEvent.change(screen.getByLabelText('关键词或标题文本'),{target:{value:'虚构人工标题'}});
 fireEvent.change(screen.getByLabelText('人工记录来源'),{target:{value:'虚构标题资料'}});
 fireEvent.click(screen.getByRole('button',{name:'创建待审核资产'}));
 await screen.findByText('内容资产已创建，状态为待审核');
 const request=calls.find(call=>call.action==='asset.create');
 expect(request?.payload).toEqual({brandId:'b',kind:'title',text:'虚构人工标题',source:'虚构标题资料',productId:null,requestId:expect.any(String)});
 expect(calls.filter(call=>['asset.review','publication.add','ai.start'].includes(call.action))).toHaveLength(0);
 expect(screen.getByLabelText('关键词或标题文本')).toHaveValue('');
});

it('preserves_failed_input_and_reuses_request_id_for_unchanged_retry',async()=>{
 const requestIds:string[]=[];let attempts=0;
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='asset.list')return [] as T;
  if(action==='asset.create'){requestIds.push(payload.requestId as string);if(++attempts===1)throw Error('虚构保存失败');return asset({id:'created',revision:1}) as T}
  throw Error(action);
 });
 render(<AssetLibrary {...props}/>);await screen.findByText('还没有内容资产');fillForm();
 fireEvent.click(screen.getByRole('button',{name:'创建待审核资产'}));await screen.findByText('虚构保存失败');
 expect(screen.getByLabelText('关键词或标题文本')).toHaveValue('虚构人工关键词');
 expect(screen.getByLabelText('人工记录来源')).toHaveValue('虚构人工来源');
 fireEvent.click(screen.getByRole('button',{name:'创建待审核资产'}));await screen.findByText('内容资产已创建，状态为待审核');
 expect(requestIds).toHaveLength(2);expect(requestIds[0]).toBe(requestIds[1]);
});

it('prevents_duplicate_creation_while_request_is_pending',async()=>{
 let finish!:(value:ContentAsset)=>void;const actions:string[]=[];
 setTestTransport(async<T,>(action:string)=>{actions.push(action);if(action==='asset.list')return [] as T;if(action==='asset.create')return await new Promise<T>(resolve=>{finish=value=>resolve(value as T)});throw Error(action)});
 render(<AssetLibrary {...props}/>);await screen.findByText('还没有内容资产');fillForm();
 const button=screen.getByRole('button',{name:'创建待审核资产'});fireEvent.click(button);fireEvent.click(button);
 await waitFor(()=>expect(actions.filter(action=>action==='asset.create')).toHaveLength(1));
 await act(async()=>finish(asset({id:'created',revision:1})));
 await screen.findByText('内容资产已创建，状态为待审核');
 expect(actions.filter(action=>action==='asset.create')).toHaveLength(1);
});

it('revises_approved_asset_back_to_pending_with_current_revision_and_history',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];const approved=asset({reviewStatus:'approved',reviewedAt:'2026-10-03T09:00:00Z'});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});if(action==='asset.list')return [approved] as T;
  if(action==='asset.revise')return asset({...approved,text:payload.text as string,source:payload.source as string,productId:null,reviewStatus:'pending',reviewedAt:null,revision:4,updatedAt:'2026-10-04T09:00:00Z',history:[...approved.history,{revision:4,action:'revised',text:payload.text as string,source:payload.source as string,productId:null,reviewStatus:'pending',reviewNote:'',reviewedAt:null,changedAt:'2026-10-04T09:00:00Z'}]}) as T;
  throw Error(action);
 });
 render(<AssetLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构通勤关键词/}));
 fireEvent.click(screen.getByRole('button',{name:'编辑此资产'}));
 expect(screen.getByLabelText('资产类型')).toBeDisabled();
 fireEvent.change(screen.getByLabelText('关键词或标题文本'),{target:{value:'虚构修订关键词'}});
 fireEvent.change(screen.getByLabelText('关联商品（可选）'),{target:{value:''}});
 fireEvent.click(screen.getByRole('button',{name:'保存修订并回到待审核'}));
 await screen.findByText('内容资产已修订并回到待审核');
 expect(calls.find(call=>call.action==='asset.revise')?.payload).toEqual({brandId:'b',id:'asset-b',expectedRevision:3,text:'虚构修订关键词',source:'虚构人工资料',productId:null});
 expect(screen.getByText('人工修订 · 修订 4 · 待审核')).toBeInTheDocument();
 expect(screen.getByText('审核通过 · 修订 2 · 已通过')).toBeInTheDocument();
});

it('requires_rejection_reason_and_preserves_note_when_review_fails',async()=>{
 setTestTransport(async<T,>(action:string)=>{if(action==='asset.list')return [asset()] as T;if(action==='asset.review')throw Error('虚构审核失败');throw Error(action)});
 render(<AssetLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构通勤关键词/}));
 expect(screen.getByRole('button',{name:'人工驳回'})).toBeDisabled();
 fireEvent.change(screen.getByLabelText('审核理由（驳回必填）'),{target:{value:'  虚构依据不足  '}});
 fireEvent.click(screen.getByRole('button',{name:'人工驳回'}));await screen.findByText('虚构审核失败');
 expect(screen.getByLabelText('审核理由（驳回必填）')).toHaveValue('  虚构依据不足  ');
});

it('archives_without_deleting_history_and_restores_the_same_asset',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 const approved=asset({reviewStatus:'approved',reviewedAt:'2026-10-03T09:00:00Z'});
 const archived=asset({...approved,revision:4,archivedAt:'2026-10-04T09:00:00Z',archiveNote:'虚构活动结束',history:[...approved.history,{revision:4,action:'archived',text:approved.text,source:approved.source,productId:'p',reviewStatus:'approved',reviewNote:'',reviewedAt:approved.reviewedAt,changedAt:'2026-10-04T09:00:00Z'}]});
 const restored=asset({...approved,revision:5,archivedAt:null,archiveNote:'',history:[...archived.history,{revision:5,action:'restored',text:approved.text,source:approved.source,productId:'p',reviewStatus:'approved',reviewNote:'',reviewedAt:approved.reviewedAt,changedAt:'2026-10-05T09:00:00Z'}]});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});if(action==='asset.list')return[approved]as T;if(action==='asset.setArchived')return(payload.archived?archived:restored)as T;throw Error(action)});
 render(<AssetLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构通勤关键词/}));
 fireEvent.change(screen.getByLabelText('归档说明（可选）'),{target:{value:'虚构活动结束'}});
 fireEvent.click(screen.getByRole('button',{name:'归档此资产'}));
 await screen.findByText('内容资产已归档，历史稿件快照仍保留');
 expect(calls.at(-1)?.payload).toEqual({brandId:'b',id:'asset-b',expectedRevision:3,archived:true,note:'虚构活动结束'});
 expect(screen.getByText('归档保留 · 修订 4 · 已通过')).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'编辑此资产'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'恢复此资产'}));
 await screen.findByText('内容资产已恢复，可继续修订或按当前审核状态使用');
 expect(calls.at(-1)?.payload).toEqual({brandId:'b',id:'asset-b',expectedRevision:4,archived:false,note:''});
 expect(screen.getByText('恢复使用 · 修订 5 · 已通过')).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'编辑此资产'})).toBeEnabled();
});

it('unmounts_old_brand_state_and_ignores_late_list_response',async()=>{
 const pending=new Map<string,(value:ContentAsset[])=>void>();
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{if(action!=='asset.list')throw Error(action);return await new Promise<T>(resolve=>{pending.set(payload.brandId as string,value=>resolve(value as T))})});
 const view=render(<AssetLibrary {...props}/>);await waitFor(()=>expect(pending.has('b')).toBe(true));
 view.rerender(<AssetLibrary {...props} brand={{id:'c',name:'虚构新品牌',createdAt:''}}/>);
 await waitFor(()=>expect(pending.has('c')).toBe(true));
 await act(async()=>pending.get('c')!([]));await screen.findByText('还没有内容资产');
 await act(async()=>pending.get('b')!([asset({text:'旧品牌迟到资产'})]));
 expect(screen.queryByText('旧品牌迟到资产')).toBeNull();
});

function fillForm(){
 fireEvent.change(screen.getByLabelText('关键词或标题文本'),{target:{value:'虚构人工关键词'}});
 fireEvent.change(screen.getByLabelText('人工记录来源'),{target:{value:'虚构人工来源'}});
}
