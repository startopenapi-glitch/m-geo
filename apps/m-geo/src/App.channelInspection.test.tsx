import {act,cleanup,fireEvent,render,screen,within} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import App from './App';
import {setTestTransport} from './api';
import type {ChannelInspectionPage,ChannelInspectionSelection} from './channelInspectionTypes';
import type {WorkspaceSnapshot} from './types';
const brandA='00000000-0000-4000-8000-000000000001',brandB='00000000-0000-4000-8000-000000000002',ticket='00000000-0000-4000-8000-000000000003';
const empty:WorkspaceSnapshot={products:[],questions:[],drafts:[],publications:[],observations:[]};
const selection:ChannelInspectionSelection={inspectionId:ticket,report:{format:'m-geo-channel-inspection-v1',operation:'comparison',profile:'mgeo-catalog-v1',source:{fileName:'fictional.zip',sha256:'a'.repeat(64),compressedBytes:100,uncompressedBytes:400,fileCount:4},brandId:brandA,packageId:'b'.repeat(64),baselinePackageId:'c'.repeat(64),status:'passed',counts:{selected:21,baselineSelected:21,rows:21,ready:21,blocked:0},summary:{added:0,removed:0,changed:0,unchanged:21},checks:[{code:'frozen_evidence',status:'passed',message:'有效'},{code:'report',status:'passed',message:'一致'},{code:'notice',status:'passed',message:'一致'}],caveat:'仅本地历史内部一致性，不代表平台结果。',reportFingerprint:'f'.repeat(64)}};
function page(n=0):ChannelInspectionPage{return {inspectionId:ticket,page:n,pageSize:20,total:21,rows:Array.from({length:n?1:20},(_,i)=>({sku:`FICTION-${n*20+i}`,change:'unchanged',fields:[]}))};}
function deferred<T>(){let resolve!:(v:T)=>void,reject!:(e:Error)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no});return {promise,resolve,reject};}
afterEach(()=>{cleanup();setTestTransport(undefined)});
function install(override:(action:string,p:Record<string,unknown>)=>unknown){
 setTestTransport(async<T,>(action:string,p:Record<string,unknown>)=>{
  const custom=override(action,p);if(custom!==undefined)return await custom as T;
  if(action==='brand.list')return [{id:brandA,name:'虚构甲品牌',createdAt:''},{id:brandB,name:'虚构乙品牌',createdAt:''}] as T;
  if(action==='workspace.snapshot')return empty as T;
  if(action==='file.inspectChannelReport')return selection as T;
  if(action==='file.readChannelInspectionPage')return page(p.page as number) as T;
  throw Error(action);
 });
}
async function enter(){render(<App/>);await screen.findByText('让 AI 准确理解你的商品');fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));fireEvent.click(screen.getByRole('button',{name:'渠道资料'}));await screen.findByRole('heading',{name:'历史商品通道操作 ZIP 复核'});}
function attemptReplacement(){fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:brandB}});fireEvent.click(screen.getByRole('button',{name:'添加品牌'}));fireEvent.click(screen.getByRole('button',{name:'首页'}));}
function expectLocked(){
 expect(screen.getByLabelText('当前品牌')).toBeDisabled();expect(screen.getByLabelText('当前品牌')).toHaveValue(brandA);
 expect(screen.getByRole('button',{name:'添加品牌'})).toBeDisabled();for(const b of within(screen.getByRole('navigation',{name:'主要导航'})).getAllByRole('button'))expect(b).toBeDisabled();
 expect(screen.getByRole('heading',{name:'历史商品通道操作 ZIP 复核'})).toBeInTheDocument();expect(screen.queryByRole('heading',{name:'新品牌'})).toBeNull();
 expect(screen.getByText(/等待.*本地.*读取或保存.*文件.*取消/)).toBeInTheDocument();
}
async function expectReleased(){
 expect(screen.getByLabelText('当前品牌')).toBeEnabled();expect(screen.getByRole('button',{name:'添加品牌'})).toBeEnabled();expect(screen.getByRole('button',{name:'首页'})).toBeEnabled();
 fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:brandB}});await screen.findByText('尚未选择历史通道 ZIP。');expect(screen.getByLabelText('当前品牌')).toHaveValue(brandB);expect(screen.queryByText('fictional.zip')).toBeNull();
 fireEvent.click(screen.getByRole('button',{name:'首页'}));await screen.findByText('让 AI 准确理解你的商品');
}
it.each(['cancel','success','failure'] as const)('locks App context throughout native select and first page until %s settles',async outcome=>{
 const pending=deferred<ChannelInspectionSelection|null>(),firstPage=deferred<ChannelInspectionPage>();
 install(action=>{if(action==='file.inspectChannelReport'){attemptReplacement();return pending.promise}if(action==='file.readChannelInspectionPage')return firstPage.promise});
 await enter();fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));attemptReplacement();expectLocked();
 await act(async()=>{if(outcome==='failure')pending.reject(Error('选择失败'));else pending.resolve(outcome==='cancel'?null:selection)});
 if(outcome==='success'){attemptReplacement();expectLocked();await act(async()=>firstPage.resolve(page()));await screen.findByRole('table')}
 else await screen.findByText(outcome==='cancel'?'已取消选择历史通道 ZIP':'选择失败');
 await expectReleased();
});
it.each(['cancel','success','failure'] as const)('locks App through the entire export promise after Save closes, then releases on %s',async outcome=>{
 const pending=deferred<{saved:true}|null>();
 install(action=>{if(action==='file.exportChannelInspection'){attemptReplacement();return pending.promise}});
 await enter();fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByRole('table');
 fireEvent.click(screen.getByRole('button',{name:'导出通道复核结论'}));attemptReplacement();expectLocked();
 // Save has closed, but post-dialog validation/write has not settled.
 await act(async()=>Promise.resolve());attemptReplacement();expectLocked();
 await act(async()=>{if(outcome==='failure')pending.reject(Error('保存失败'));else pending.resolve(outcome==='cancel'?null:{saved:true})});
 await screen.findByText(outcome==='failure'?'保存失败':outcome==='cancel'?'已取消保存通道复核结论':'通道复核结论 ZIP 已保存');
 await expectReleased();
});
it('keeps App context during a deferred later page and releases after failure for retry',async()=>{
 const pending=deferred<ChannelInspectionPage>();let fail=true;
 install((action,p)=>action==='file.readChannelInspectionPage'&&p.page===1&&fail?pending.promise:undefined);
 await enter();fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByRole('table');
 fireEvent.click(screen.getByRole('button',{name:'下一页'}));attemptReplacement();expectLocked();
 await act(async()=>pending.reject(Error('分页失败')));await screen.findByText('分页失败');
 expect(screen.getByLabelText('当前品牌')).toBeEnabled();fail=false;
 fireEvent.click(screen.getByRole('button',{name:'重试读取历史明细'}));await screen.findByText('FICTION-20');
 await expectReleased();
});
it('keeps the active history context when an earlier brand creation finishes, then allows selecting the created brand',async()=>{
 const creating=deferred<{id:string;name:string;createdAt:string}>(),historical=deferred<ChannelInspectionSelection|null>();
 const created={id:'00000000-0000-4000-8000-000000000004',name:'新建虚构品牌',createdAt:''};
 install(action=>action==='brand.create'?creating.promise:action==='file.inspectChannelReport'?historical.promise:undefined);
 render(<App/>);await screen.findByText('让 AI 准确理解你的商品');
 fireEvent.click(screen.getByRole('button',{name:'添加品牌'}));
 fireEvent.change(screen.getByLabelText('品牌名称'),{target:{value:created.name}});
 fireEvent.click(screen.getByRole('button',{name:'创建品牌'}));
 // Abandon the pending create form and enter a valid existing brand's history.
 fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:brandB}});
 await screen.findByText('让 AI 准确理解你的商品');
 fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));fireEvent.click(screen.getByRole('button',{name:'渠道资料'}));
 await screen.findByRole('heading',{name:'历史商品通道操作 ZIP 复核'});
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));
 await act(async()=>creating.resolve(created));
 expect(screen.getByRole('option',{name:created.name})).toBeInTheDocument();
 expect(screen.getByLabelText('当前品牌')).toHaveValue(brandB);
 expect(screen.getByLabelText('当前品牌')).toBeDisabled();
 expect(screen.getByRole('button',{name:'渠道资料'})).toHaveAttribute('aria-current','page');
 expect(screen.getByRole('heading',{name:'历史商品通道操作 ZIP 复核'})).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'选择历史通道 ZIP'})).toBeDisabled();
 await act(async()=>historical.resolve(null));
 await screen.findByText('已取消选择历史通道 ZIP');
 expect(screen.getByLabelText('当前品牌')).toHaveValue(brandB);
 fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:created.id}});
 await screen.findByText('尚未选择历史通道 ZIP。');
 expect(screen.getByLabelText('当前品牌')).toHaveValue(created.id);
 expect(screen.getByRole('button',{name:'渠道资料'})).toHaveAttribute('aria-current','page');
});
