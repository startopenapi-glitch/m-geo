import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {ContentAsset} from '../assetTypes';
import type {Draft,Product,WorkspaceSnapshot} from '../types';
import {DraftEditor} from './Content';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'capacity',label:'容量',value:'500mL',source:'虚构规格表第 2 页',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构介绍',body:'容量：500mL',claims:[{id:'c',text:'容量：500mL',factId:'capacity'}],assetIds:[],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:null,createdAt:''};
function asset(id:string,kind:ContentAsset['kind'],text:string,reviewStatus:ContentAsset['reviewStatus']='approved'):ContentAsset{return{id,brandId:'b',kind,text,source:'虚构内容 brief',productId:'p',reviewStatus,reviewNote:'',reviewedAt:reviewStatus==='approved'?'2026-10-24T00:00:00Z':null,revision:2,createdAt:'',updatedAt:'2026-10-24T00:00:00Z',history:[]}}
function props(assets:ContentAsset[],value:Draft=draft){const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[value],publications:[],observations:[],assets};return{brand:{id:'b',name:'虚构品牌',createdAt:''},data,refresh:vi.fn(async()=>{})}}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('inserts an approved keyword and saves only its id for native review',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});return{...draft,revision:2} as T});
 const input=props([asset('a','keyword','便携水杯')]);
 render(<DraftEditor {...input} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'插入正文'}));
 expect(screen.getByLabelText('草稿正文')).toHaveValue('容量：500mL\n\n便携水杯');
 expect(screen.getByLabelText('关联资产：便携水杯')).toBeChecked();
 fireEvent.click(screen.getByRole('button',{name:'保存修改'}));
 await waitFor(()=>expect(calls).toHaveLength(1));
 expect(calls[0].action).toBe('draft.save');
 expect(calls[0].payload.assetIds).toEqual(['a']);
 expect(calls[0].payload).not.toHaveProperty('assetSnapshots');
});

it('uses an approved title asset and prevents selecting a pending asset',()=>{
 const input=props([asset('title','title','500mL 虚构便携水杯'),asset('pending','keyword','未审核词','pending')]);
 render(<DraftEditor {...input} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'用于标题'}));
 expect(screen.getByLabelText('草稿标题')).toHaveValue('500mL 虚构便携水杯');
 expect(screen.getByLabelText('关联资产：500mL 虚构便携水杯')).toBeChecked();
 expect(screen.getByLabelText('关联资产：未审核词')).toBeDisabled();
 expect(screen.queryByRole('button',{name:'插入正文'})).toBeNull();
});

it('shows the frozen historical asset version after the current asset is revised',()=>{
 const reviewed:Draft={...draft,body:'容量：500mL\n\n便携水杯',reviewedAt:'2026-10-25T00:00:00Z',assetIds:['a'],assetSnapshots:[{snapshotVersion:1,assetId:'a',kind:'keyword',text:'便携水杯',source:'虚构内容 brief 第 1 版',productId:'p',assetRevision:2,assetReviewedAt:'2026-10-24T00:00:00Z',assetUpdatedAt:'2026-10-24T00:00:00Z'}]};
 const revised={...asset('a','keyword','新款水杯','pending'),revision:3,text:'新款水杯'};
 const input=props([revised],reviewed);
 render(<DraftEditor {...input} draft={reviewed}/>);
 expect(screen.getByTestId('asset-snapshot-a')).toHaveTextContent('便携水杯');
 expect(screen.getByTestId('asset-snapshot-a')).toHaveTextContent('审核版本 v2');
 expect(screen.getByText('本稿已冻结的资产版本')).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeEnabled();
});

it('keeps an archived linked asset visible for history without offering new insertion',()=>{
 const reviewed:Draft={...draft,body:'容量：500mL\n\n便携水杯',reviewedAt:'2026-10-25T00:00:00Z',assetIds:['a'],assetSnapshots:[{snapshotVersion:1,assetId:'a',kind:'keyword',text:'便携水杯',source:'虚构内容 brief',productId:'p',assetRevision:2,assetReviewedAt:'2026-10-24T00:00:00Z',assetUpdatedAt:'2026-10-24T00:00:00Z'}]};
 const archived={...asset('a','keyword','便携水杯'),archivedAt:'2026-10-25T01:00:00Z',archiveNote:'虚构归档',revision:3};
 const input=props([archived],reviewed);
 render(<DraftEditor {...input} draft={reviewed}/>);
 expect(screen.getByText(/已归档，仅供历史快照追溯/)).toBeInTheDocument();
 expect(screen.queryByRole('button',{name:'插入正文'})).toBeNull();
 expect(screen.getByTestId('asset-snapshot-a')).toHaveTextContent('审核版本 v2');
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeEnabled();
});
