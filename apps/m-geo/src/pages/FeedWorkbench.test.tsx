import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import {setTestTransport} from '../api';
import type {WorkspaceSnapshot} from '../types';
import FeedWorkbench from './FeedWorkbench';
const data:WorkspaceSnapshot={products:[{id:'p',brandId:'b',name:'虚构台灯',sku:'TEST',category:'灯',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:'2026-10-23',updatedAt:''}],drafts:[{id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'已审核介绍',body:'虚构介绍',sourceRevision:1,revision:2,generationMode:'local',reviewedAt:'2026-10-23',createdAt:''}],questions:[],publications:[],observations:[]};
const result={manifest:{},packageId:'package',summary:{selected:1,ready:0,blocked:1},rows:[{productId:'p',sku:'TEST',localStatus:'blocked',issues:[{field:'price',code:'missing',message:'价格待补充'}]}],externalChecks:['待核实资格'],files:[{path:'report.json',content:'{}'}]};
afterEach(()=>{cleanup();setTestTransport(undefined)});
function select(){fireEvent.click(screen.getByRole('checkbox',{name:/虚构台灯/}));fireEvent.change(screen.getByLabelText('虚构台灯的已审核介绍'),{target:{value:'d'}})}
it('ignores a late preview after selection changes',async()=>{let finish!:(v:unknown)=>void;setTestTransport(async<T,>()=>await new Promise<T>(resolve=>{finish=v=>resolve(v as T)}));render(<FeedWorkbench brandId="b" data={data}/>);select();fireEvent.click(screen.getByText('检查 Feed 草稿'));fireEvent.click(screen.getByRole('checkbox',{name:/虚构台灯/}));await act(async()=>finish(result));expect(screen.queryByText('价格待补充')).toBeNull();expect(screen.getByText('导出草稿与检查报告')).toBeDisabled();});
it('exports blocked reports with the same explicit request and reports cancel',async()=>{const calls:{action:string;payload:unknown}[]=[];setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});return (action==='feed.prepare'?result:null) as T});render(<FeedWorkbench brandId="b" data={data}/>);select();fireEvent.click(screen.getByText('检查 Feed 草稿'));await screen.findByText('价格待补充');expect(screen.getByText(/仅导出检查报告/)).toBeInTheDocument();fireEvent.click(screen.getByText('导出草稿与检查报告'));await screen.findByText('已取消保存');expect(calls.map(c=>c.action)).toEqual(['feed.prepare','file.exportFeed']);expect(calls[0].payload).toEqual(calls[1].payload);});
it('clears selection and results on snapshot or brand change',async()=>{setTestTransport(async<T,>()=>result as T);const view=render(<FeedWorkbench brandId="b" data={data}/>);select();fireEvent.click(screen.getByText('检查 Feed 草稿'));await screen.findByText('价格待补充');view.rerender(<FeedWorkbench brandId="b" data={{...data,products:[{...data.products[0],revision:2}]}}/>);await waitFor(()=>expect(screen.getByText('检查 Feed 草稿')).toBeDisabled());expect(screen.queryByText('价格待补充')).toBeNull();expect(screen.getByRole('checkbox',{name:/虚构台灯/})).not.toBeChecked();});

const historyReport={format:'m-geo-channel-inspection-v1',operation:'comparison',profile:'mgeo-catalog-v1',source:{fileName:'历史通道.zip',sha256:'a'.repeat(64),compressedBytes:1000,uncompressedBytes:2000,fileCount:4},brandId:'b',packageId:'p'.repeat(64),baselinePackageId:'q'.repeat(64),status:'passed',counts:{selected:1,baselineSelected:1,rows:1,ready:1,blocked:0},summary:{added:0,removed:0,changed:0,unchanged:1},checks:[{code:'frozen_evidence',status:'passed',message:'冻结证据有效'},{code:'report',status:'passed',message:'报告一致'},{code:'notice',status:'passed',message:'声明一致'}],caveat:'仅检查历史内部一致性；哈希不是签名。',reportFingerprint:'f'.repeat(64)};
function historyTransport(){setTestTransport(async<T,>(action:string)=>{if(action==='file.inspectChannelReport')return {inspectionId:'ticket',report:historyReport} as T;if(action==='file.readChannelInspectionPage')return {inspectionId:'ticket',page:0,pageSize:20,total:1,rows:[{sku:'冻结历史 SKU',change:'unchanged',fields:[]}]} as T;return result as T})}
it.each([false,true])('allows historical inspection with no current products/drafts, truncated=%s',async truncated=>{
 historyTransport();render(<FeedWorkbench brandId="b" data={{...data,products:[],drafts:[],truncated}}/>);
 expect(screen.getByRole('heading',{name:'历史商品通道操作 ZIP 复核'})).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByText('冻结历史 SKU');
 expect(screen.getByText('检查 Feed 草稿')).toBeDisabled();
});
it('preserves historical result across same-brand snapshot changes while resetting live selection, and clears history on brand return',async()=>{
 historyTransport();const view=render(<FeedWorkbench brandId="b" data={data}/>);select();
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByText('冻结历史 SKU');
 view.rerender(<FeedWorkbench brandId="b" data={{...data,products:[{...data.products[0],revision:2}]}}/>);
 expect(screen.getByText('冻结历史 SKU')).toBeInTheDocument();expect(screen.getByRole('checkbox',{name:/虚构台灯/})).not.toBeChecked();
 expect(screen.getAllByRole('heading')[0]).toHaveTextContent('历史商品通道操作 ZIP 复核');
 view.rerender(<FeedWorkbench brandId="other" data={data}/>);expect(screen.queryByText('冻结历史 SKU')).toBeNull();
 view.rerender(<FeedWorkbench brandId="b" data={data}/>);expect(screen.queryByText('冻结历史 SKU')).toBeNull();
 expect(screen.getByRole('button',{name:'导出通道复核结论'})).toBeDisabled();
});
