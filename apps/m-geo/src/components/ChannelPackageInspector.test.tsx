import {act,cleanup,fireEvent,render,screen,within} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {ChannelInspectionPage,ChannelInspectionReport,ChannelInspectionSelection} from '../channelInspectionTypes';
import ChannelPackageInspector from './ChannelPackageInspector';

const brandId='00000000-0000-4000-8000-000000000001',inspectionId='00000000-0000-4000-8000-000000000002';
const hostile='<script>alert(1)</script> https://example.invalid =SUM(A1)';
const report:ChannelInspectionReport={format:'m-geo-channel-inspection-v1',operation:'comparison',profile:'mgeo-catalog-v1',source:{fileName:'历史通道.zip',sha256:'a'.repeat(64),compressedBytes:1200,uncompressedBytes:3600,fileCount:4},brandId,packageId:'b'.repeat(64),baselinePackageId:'c'.repeat(64),status:'passed',counts:{selected:20,baselineSelected:2,rows:21,ready:19,blocked:1},summary:{added:19,removed:1,changed:1,unchanged:0},checks:[{code:'frozen_evidence',status:'passed',message:'冻结证据有效'},{code:'report',status:'passed',message:'报告一致'},{code:'notice',status:'passed',message:'声明一致'}],caveat:'仅检查历史内部一致性；不证明当前新鲜度或真实审核，不代表平台提交、接受或排名。哈希不是签名。选择不是完整商家目录；模拟状态不是真实结果。',reportFingerprint:'f'.repeat(64)};
const selection:ChannelInspectionSelection={inspectionId,report};
function page(target=0,ticket=inspectionId):ChannelInspectionPage{return {inspectionId:ticket,page:target,pageSize:20,total:21,rows:Array.from({length:target===0?20:1},(_,index)=>({sku:index<2&&target===0?'复用SKU':`SKU-${target*20+index}`,change:target===1?'removed':'added',fields:[index===0?hostile:'price']}))};}
function deferred<T>(){let resolve!:(value:T)=>void;const promise=new Promise<T>(yes=>{resolve=yes});return {promise,resolve};}
function transport(override?:(action:string,payload:Record<string,unknown>)=>unknown){
 const calls=vi.fn(async(action:string,payload:Record<string,unknown>)=>{
  const custom=override?.(action,payload);if(custom!==undefined)return await custom;
  if(action==='file.inspectChannelReport')return selection;
  if(action==='file.readChannelInspectionPage')return page(payload.page as number,payload.inspectionId as string);
  if(action==='file.exportChannelInspection')return null;
  throw new Error(`Unexpected action: ${action}`);
 });
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>await calls(action,payload) as T);return calls;
}
async function open(){fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByRole('table',{name:'历史通道操作明细'});}
afterEach(()=>{cleanup();setTestTransport(undefined)});

it('starts empty and consumes exact native identifier-only selection/page contracts',async()=>{
 const calls=transport();render(<ChannelPackageInspector brandId={brandId}/>);
 expect(screen.getByText('尚未选择历史通道 ZIP。')).toBeInTheDocument();expect(screen.getByRole('button',{name:'导出通道复核结论'})).toBeDisabled();
 expect(screen.getByText(/支持比较、模拟回执、回执核对.*不支持原始 Feed\/XML/)).toBeInTheDocument();
 await open();expect(calls.mock.calls).toEqual([['file.inspectChannelReport',{brandId}],['file.readChannelInspectionPage',{brandId,inspectionId,page:0}]]);
 expect(screen.getByText('一致性检查通过')).toBeInTheDocument();expect(screen.getByText('历史本地就绪 19 件 · 历史阻断 1 件')).toBeInTheDocument();
 expect(screen.getByText('本地新增 19 · 本地移除 1 · 变化 1 · 无变化 0')).toBeInTheDocument();
 expect(screen.getByText(report.caveat)).toBeInTheDocument();expect(screen.getByText(report.source.sha256)).toBeInTheDocument();
 expect(screen.getByText(/展开 3600 字节/)).toBeInTheDocument();expect(screen.getByText('mgeo-catalog-v1')).toBeInTheDocument();
 for(const label of ['冻结证据有效','报告一致','声明一致'])expect(screen.getByText(new RegExp(label))).toBeInTheDocument();
 expect(within(screen.getByRole('table')).getAllByRole('row')).toHaveLength(21);
 expect(screen.getAllByText('复用SKU')).toHaveLength(2);
});
it('paginates 20 plus remaining rows, clears old rows while loading, and resets on reselection',async()=>{
 const next=deferred<ChannelInspectionPage>();let selections=0;
 const calls=transport((action,payload)=>action==='file.readChannelInspectionPage'&&payload.page===1?next.promise:action==='file.inspectChannelReport'?{...selection,inspectionId:++selections===1?inspectionId:'new-ticket'}:undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);await open();
 expect(screen.getByRole('button',{name:'上一页'})).toBeDisabled();expect(screen.getByText('第 1/2 页 · 共 21 条')).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'下一页'}));expect(screen.queryByRole('table')).toBeNull();expect(screen.getByText('历史通道.zip')).toBeInTheDocument();
 await act(async()=>next.resolve(page(1)));expect(await screen.findByText('SKU-20')).toBeInTheDocument();
 expect(within(screen.getByRole('table')).getAllByRole('row')).toHaveLength(2);expect(screen.getByRole('button',{name:'下一页'})).toBeDisabled();
 await open();expect(screen.getByText('第 1/2 页 · 共 21 条')).toBeInTheDocument();
 expect(calls.mock.calls.at(-1)).toEqual(['file.readChannelInspectionPage',{brandId,inspectionId:'new-ticket',page:0}]);
});
it('keeps blocked historical results readable and exportable',async()=>{
 transport(a=>a==='file.inspectChannelReport'?{...selection,report:{...report,status:'blocked',checks:[report.checks[0],{code:'report',status:'failed',message:'报告不一致'},report.checks[2]]}}:undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);await open();expect(screen.getByText('一致性检查未通过')).toBeInTheDocument();
 expect(screen.getByText(/报告不一致/)).toBeInTheDocument();expect(screen.getByRole('button',{name:'导出通道复核结论'})).toBeEnabled();
});
it('renders receipt-check rows as text with missing and unknown preserved',async()=>{
 transport(a=>a==='file.inspectChannelReport'?{...selection,report:{...report,operation:'receipt-check',baselinePackageId:null,counts:{selected:4,baselineSelected:0,rows:4,ready:4,blocked:0},summary:{accepted:1,rejected:1,unknown:1,missing:1}}}:a==='file.readChannelInspectionPage'?{inspectionId,page:0,pageSize:20,total:4,rows:['accepted','rejected','unknown','missing'].map((status,index)=>({sku:`receipt-${index}`,rowHash:'d'.repeat(64),status,code:'SIM',message:hostile}))}:undefined);
 const view=render(<ChannelPackageInspector brandId={brandId}/>);await open();
 expect(screen.getByText('模拟接受 1 · 模拟拒绝 1 · 未知 1 · 缺失 1')).toBeInTheDocument();
 const table=within(screen.getByRole('table'));for(const label of ['模拟接受','模拟拒绝','未知','缺失'])expect(table.getByText(label)).toBeInTheDocument();
 expect(table.getAllByText(hostile)).toHaveLength(4);expect(view.container.querySelector('script,a,iframe')).toBeNull();
});
it('renders a full uniform simulation without implying a real platform result',async()=>{
 transport(a=>a==='file.inspectChannelReport'?{...selection,report:{...report,operation:'simulation',baselinePackageId:null,counts:{selected:1,baselineSelected:0,rows:1,ready:1,blocked:0},summary:{accepted:0,rejected:0,unknown:1,missing:0}}}:a==='file.readChannelInspectionPage'?{inspectionId,page:0,pageSize:20,total:1,rows:[{sku:'simulated-only',rowHash:'d'.repeat(64),status:'unknown',code:'SIMULATED_UNKNOWN',message:'仅本机模拟；无法确定平台结果'}]}:undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);await open();
 expect(screen.getByText('模拟回执（simulation）')).toBeInTheDocument();expect(screen.getByText('模拟接受 0 · 模拟拒绝 0 · 未知 1 · 缺失 0')).toBeInTheDocument();
 expect(screen.getByText('仅本机模拟；无法确定平台结果')).toBeInTheDocument();expect(screen.getByText(report.caveat)).toBeInTheDocument();
});
it('uses accessible scroll region, wraps long identifiers and renders source/comparison fields as inert text',async()=>{
 transport(a=>a==='file.inspectChannelReport'?{...selection,report:{...report,source:{...report.source,fileName:hostile}}}:undefined);
 const view=render(<ChannelPackageInspector brandId={brandId}/>);await open();
 const region=screen.getByRole('region',{name:'历史通道操作明细可横向滚动表格'});expect(region).toHaveClass('attribution-table-wrap');expect(region).toHaveAttribute('tabindex','0');
 expect(screen.getByText(report.reportFingerprint)).toHaveStyle({overflowWrap:'anywhere'});
 expect(view.container.textContent).toContain(hostile);expect(view.container.querySelector('script,a,iframe')).toBeNull();
 expect(screen.getByRole('navigation',{name:'历史通道明细分页'})).toBeInTheDocument();
});
it('exports only the ticket/fingerprint and retains the report on export cancellation',async()=>{
 const calls=transport();render(<ChannelPackageInspector brandId={brandId}/>);await open();
 fireEvent.click(screen.getByRole('button',{name:'导出通道复核结论'}));await screen.findByText('已取消保存通道复核结论');
 expect(screen.getByText('历史通道.zip')).toBeInTheDocument();expect(calls.mock.calls.at(-1)).toEqual(['file.exportChannelInspection',{brandId,inspectionId,reportFingerprint:report.reportFingerprint}]);
 expect(screen.getByText(/汇总操作报告.*inspection.json.*report.md.*文件名.*谨慎分享/)).toBeInTheDocument();
});
it.each(['cancel','fail'])('clears an earlier report/rows/export when reselection ends in %s',async outcome=>{
 let selects=0;transport(a=>a==='file.inspectChannelReport'&&++selects>1?outcome==='cancel'?null:Promise.reject(new Error('通道证据包无效或不受支持')):undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);await open();fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));
 await screen.findByText(outcome==='cancel'?'已取消选择历史通道 ZIP':'通道证据包无效或不受支持');expect(screen.queryByText('历史通道.zip')).toBeNull();expect(screen.queryByRole('table')).toBeNull();expect(screen.getByRole('button',{name:'导出通道复核结论'})).toBeDisabled();
});
it('blocks synchronous double clicks for selection, page and export using a synchronous lock',async()=>{
 const select=deferred<ChannelInspectionSelection>(),read=deferred<ChannelInspectionPage>(),save=deferred<{saved:true}>();
 const calls=transport((a,p)=>a==='file.inspectChannelReport'?select.promise:a==='file.readChannelInspectionPage'&&p.page===1?read.promise:a==='file.exportChannelInspection'?save.promise:undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);
 function twice(name:string){const button=screen.getByRole('button',{name});act(()=>{button.click();button.click()})}
 twice('选择历史通道 ZIP');expect(calls.mock.calls.filter(([a])=>a==='file.inspectChannelReport')).toHaveLength(1);
 await act(async()=>select.resolve(selection));await screen.findByRole('table');twice('下一页');expect(calls.mock.calls.filter(([,p])=>p.page===1)).toHaveLength(1);
 await act(async()=>read.resolve(page(1)));twice('导出通道复核结论');expect(calls.mock.calls.filter(([a])=>a==='file.exportChannelInspection')).toHaveLength(1);
 await act(async()=>save.resolve({saved:true}));await screen.findByText('通道复核结论 ZIP 已保存');
});
it.each(['selection','page','export'])('suppresses a delayed %s after brand change',async pending=>{
 const late=deferred<unknown>();const calls=transport((a,p)=>pending==='selection'&&a==='file.inspectChannelReport'||pending==='page'&&a==='file.readChannelInspectionPage'&&p.page===1||pending==='export'&&a==='file.exportChannelInspection'?late.promise:undefined);
 const view=render(<ChannelPackageInspector brandId={brandId}/>);
 if(pending==='selection')fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));else{await open();fireEvent.click(screen.getByRole('button',{name:pending==='page'?'下一页':'导出通道复核结论'}))}
 view.rerender(<ChannelPackageInspector brandId="other-brand"/>);const count=calls.mock.calls.length;
 await act(async()=>late.resolve(pending==='selection'?selection:pending==='page'?page(1):{saved:true}));
 expect(screen.getByText('尚未选择历史通道 ZIP。')).toBeInTheDocument();expect(screen.queryByText('通道复核结论 ZIP 已保存')).toBeNull();expect(calls.mock.calls).toHaveLength(count);
});
it.each(['selection','page','export'])('suppresses a delayed %s after unmount/re-entry',async pending=>{
 const late=deferred<unknown>();transport((a,p)=>pending==='selection'&&a==='file.inspectChannelReport'||pending==='page'&&a==='file.readChannelInspectionPage'&&p.page===1||pending==='export'&&a==='file.exportChannelInspection'?late.promise:undefined);
 const view=render(<ChannelPackageInspector brandId={brandId}/>);
 if(pending==='selection')fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));else{await open();fireEvent.click(screen.getByRole('button',{name:pending==='page'?'下一页':'导出通道复核结论'}))}
 view.unmount();render(<ChannelPackageInspector brandId={brandId}/>);
 await act(async()=>late.resolve(pending==='selection'?selection:pending==='page'?page(1):{saved:true}));
 expect(screen.getByText('尚未选择历史通道 ZIP。')).toBeInTheDocument();expect(screen.queryByText('通道复核结论 ZIP 已保存')).toBeNull();
});
it('retains the aggregate report on page failure and retries without selecting again',async()=>{
 let reads=0;const calls=transport(a=>a==='file.readChannelInspectionPage'&&++reads===1?Promise.reject(new Error('无法读取历史页')):undefined);
 render(<ChannelPackageInspector brandId={brandId}/>);fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByText('无法读取历史页');expect(screen.getByText('历史通道.zip')).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'重试读取历史明细'}));await screen.findByRole('table');expect(calls.mock.calls.filter(([a])=>a==='file.inspectChannelReport')).toHaveLength(1);
});
it('ignores pages that do not match the current native ticket/page',async()=>{
 transport(a=>a==='file.readChannelInspectionPage'?page(0,'wrong-ticket'):undefined);render(<ChannelPackageInspector brandId={brandId}/>);
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByRole('button',{name:'重试读取历史明细'});expect(screen.queryByRole('table')).toBeNull();
});

it('reports busy synchronously through selection and its initial page, then releases on page completion',async()=>{
 const selected=deferred<ChannelInspectionSelection>(),firstPage=deferred<ChannelInspectionPage>();
 const busy=vi.fn();let readStarted=false;
 transport(a=>{if(a==='file.inspectChannelReport'){expect(busy).toHaveBeenLastCalledWith(true);return selected.promise}if(a==='file.readChannelInspectionPage'){readStarted=true;return firstPage.promise}});
 render(<ChannelPackageInspector brandId={brandId} onBusyChange={busy}/>);
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));expect(busy.mock.calls).toEqual([[true]]);
 await act(async()=>selected.resolve(selection));expect(readStarted).toBe(true);expect(busy.mock.calls).toEqual([[true]]);
 await act(async()=>firstPage.resolve(page()));expect(busy.mock.calls).toEqual([[true],[false]]);
});
it('releases the parent busy lock on a page failure and allows retry',async()=>{
 const busy=vi.fn();let fail=true;
 transport(a=>a==='file.readChannelInspectionPage'&&fail?Promise.reject(new Error('分页失败')):undefined);
 render(<ChannelPackageInspector brandId={brandId} onBusyChange={busy}/>);
 fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));await screen.findByText('分页失败');
 expect(busy.mock.calls).toEqual([[true],[false]]);fail=false;
 fireEvent.click(screen.getByRole('button',{name:'重试读取历史明细'}));await screen.findByRole('table');
 expect(busy.mock.calls).toEqual([[true],[false],[true],[false]]);
});
it.each(['selection','page','export'])('releases parent busy on unmount during %s without a late release',async pending=>{
 const late=deferred<unknown>(),busy=vi.fn();transport((a,p)=>pending==='selection'&&a==='file.inspectChannelReport'||pending==='page'&&a==='file.readChannelInspectionPage'&&p.page===1||pending==='export'&&a==='file.exportChannelInspection'?late.promise:undefined);
 const view=render(<ChannelPackageInspector brandId={brandId} onBusyChange={busy}/>);
 if(pending==='selection')fireEvent.click(screen.getByRole('button',{name:'选择历史通道 ZIP'}));else{await open();busy.mockClear();fireEvent.click(screen.getByRole('button',{name:pending==='page'?'下一页':'导出通道复核结论'}))}
 expect(busy.mock.calls).toEqual([[true]]);view.unmount();expect(busy.mock.calls).toEqual([[true],[false]]);
 await act(async()=>late.resolve(pending==='selection'?selection:pending==='page'?page(1):{saved:true}));
 expect(busy.mock.calls).toEqual([[true],[false]]);
});
