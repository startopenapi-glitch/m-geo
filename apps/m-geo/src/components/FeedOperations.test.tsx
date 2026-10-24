import {afterEach,describe,expect,it} from 'vitest';
import {act,fireEvent,render,screen,waitFor} from '@testing-library/react';
import FeedOperations from './FeedOperations';
import {setTestTransport} from '../api';
import type {FeedRequest} from '../feedTypes';
import type {Payload} from '../types';
const request:FeedRequest={brandId:'fictional-brand',profile:'mgeo-catalog-v1',items:[{productId:'fictional-product',expectedRevision:1,draftId:'fictional-draft',expectedDraftRevision:1}]};
const comparison={summary:{added:1,removed:0,changed:0,unchanged:0},rows:[{sku:'FICTIONAL-SKU',change:'added',fields:[]}],files:[]};
const simulated={receipt:{format:'m-geo-feed-receipt-v1',mode:'simulated',brandId:request.brandId,profile:request.profile,packageId:'fixture-package',rows:[{sku:'FICTIONAL-SKU',rowHash:'fixture-row',status:'accepted',code:'SIMULATED_ACCEPTED',message:'模拟接受'}]},summary:{accepted:1,rejected:0,unknown:0,missing:0},files:[]};
afterEach(()=>setTestTransport(undefined));
describe('local feed operations',()=>{
 it.each(['request','snapshot'] as const)('ignores late comparison after %s changes',async(kind)=>{
  let resolve!:(value:unknown)=>void;
  setTestTransport((async(action:string)=>action==='feed.compare'?new Promise(r=>{resolve=r}):null) as never);
  const view=render(<FeedOperations request={request} snapshotKey="one"/>);
  fireEvent.change(screen.getByLabelText('基线清单 JSON'),{target:{value:'{"baseline":true}'}});
  fireEvent.click(screen.getByRole('button',{name:'比较当前本地选择'}));
  view.rerender(<FeedOperations request={kind==='request'?{...request,brandId:'another-fictional-brand'}:request} snapshotKey={kind==='snapshot'?'two':'one'}/>);
  await act(async()=>resolve(comparison));
  expect(screen.queryByText('FICTIONAL-SKU')).toBeNull();
  expect(screen.getByRole('button',{name:'导出比较 ZIP'})).toBeDisabled();
 });
 it('retains imported content on cancel and ignores late import after brand change',async()=>{
  let resolve!:(value:unknown)=>void;
  let canceled=true;
  setTestTransport((async(action:string,payload:Payload)=>{if(action==='file.openText'){expect(payload).toEqual({kind:'evidence'});return canceled?null:new Promise(r=>{resolve=r})}return null}) as never);
  const view=render(<FeedOperations request={request} snapshotKey="one"/>);
  fireEvent.change(screen.getByLabelText('基线清单 JSON'),{target:{value:'previous baseline'}});
  fireEvent.click(screen.getByRole('button',{name:'导入基线清单'}));
  await screen.findByText('已取消导入，原内容已保留');
  expect(screen.getByLabelText('基线清单 JSON')).toHaveValue('previous baseline');
  canceled=false;
  fireEvent.click(screen.getByRole('button',{name:'导入基线清单'}));
  view.rerender(<FeedOperations request={{...request,brandId:'new-fictional-brand'}} snapshotKey="one"/>);
  await act(async()=>resolve({name:'old.json',content:'late old brand baseline'}));
  expect(screen.getByLabelText('基线清单 JSON')).toHaveValue('');
 });
 it('invalidates previews on edited inputs and exports the identical operation inputs',async()=>{
  const exports:unknown[]=[];
  setTestTransport((async(action:string,payload:Payload)=>{if(action==='feed.simulate')return simulated;if(action==='file.exportFeedOperation'){exports.push(payload);return null}return comparison}) as never);
  render(<FeedOperations request={request} snapshotKey="one"/>);
  fireEvent.click(screen.getByRole('button',{name:'生成模拟回执'}));
  await screen.findByText('模拟接受 1 · 模拟拒绝 0 · 未知 0 · 缺失 0');
  fireEvent.click(screen.getByRole('button',{name:'导出模拟回执 ZIP'}));
  await screen.findByText('已取消保存');
  expect(exports).toEqual([{...request,scenario:'accepted',operation:'simulate'}]);
  fireEvent.change(screen.getByLabelText('模拟场景'),{target:{value:'rejected'}});
  expect(screen.getByRole('button',{name:'导出模拟回执 ZIP'})).toBeDisabled();
  fireEvent.change(screen.getByLabelText('基线清单 JSON'),{target:{value:'first baseline'}});
  fireEvent.click(screen.getByRole('button',{name:'比较当前本地选择'}));
  await screen.findByText('FICTIONAL-SKU');
  fireEvent.change(screen.getByLabelText('基线清单 JSON'),{target:{value:'changed baseline'}});
  expect(screen.getByRole('button',{name:'导出比较 ZIP'})).toBeDisabled();
 });
 it('does not display a late check after receipt input changes and clears on unmount',async()=>{
  let resolve!:(value:unknown)=>void;
  setTestTransport((async()=>new Promise(r=>{resolve=r})) as never);
  const view=render(<FeedOperations request={request} snapshotKey="one"/>);
  fireEvent.change(screen.getByLabelText('模拟回执 JSON'),{target:{value:'old receipt'}});
  fireEvent.click(screen.getByRole('button',{name:'核对模拟回执'}));
  fireEvent.change(screen.getByLabelText('模拟回执 JSON'),{target:{value:'new receipt'}});
  await act(async()=>resolve({summary:{accepted:1,rejected:0,unknown:0,missing:0},rows:[],files:[]}));
  await waitFor(()=>expect(screen.queryByText('模拟接受 1 · 模拟拒绝 0 · 未知 0 · 缺失 0')).toBeNull());
  expect(screen.getByRole('button',{name:'导出回执核对 ZIP'})).toBeDisabled();
  view.unmount();
 });
 it('clears a prior receipt check when simulation replaces its input',async()=>{
  setTestTransport((async(action:string)=>action==='feed.simulate'?simulated:{summary:{accepted:0,rejected:0,unknown:0,missing:1},rows:[],files:[]}) as never);
  render(<FeedOperations request={request} snapshotKey="one"/>);
  fireEvent.change(screen.getByLabelText('模拟回执 JSON'),{target:{value:'previous receipt'}});
  fireEvent.click(screen.getByRole('button',{name:'核对模拟回执'}));
  await screen.findByText('模拟接受 0 · 模拟拒绝 0 · 未知 0 · 缺失 1');
  fireEvent.click(screen.getByRole('button',{name:'生成模拟回执'}));
  await screen.findByText('模拟接受 1 · 模拟拒绝 0 · 未知 0 · 缺失 0');
  expect(screen.queryByText('模拟接受 0 · 模拟拒绝 0 · 未知 0 · 缺失 1')).toBeNull();
  expect(screen.getByRole('button',{name:'导出回执核对 ZIP'})).toBeDisabled();
 });

});
