import {act,cleanup,fireEvent,render,screen,waitFor,within} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import {setTestTransport} from '../api';
import type {WorkspaceSnapshot} from '../types';
import AttributionWorkbench from './AttributionWorkbench';
const data:WorkspaceSnapshot={products:[{id:'p',brandId:'b',name:'虚构台灯',sku:'TEST',category:'灯',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''}],drafts:[],questions:[],publications:[],observations:[]};
const dataset={id:'ds',name:'模拟订单',createdAt:'2026-10-23T00:00:00Z',evidenceMode:'simulated',sourceLabel:'虚构演练',currency:'CNY',coverageStart:'2026-10-01T00:00:00Z',coverageEnd:'2026-10-08T00:00:00Z',counts:{visits:2,orders:3,refunds:1},contentHash:'hash'};
const preview={...dataset,products:[{id:'p',sku:'TEST',name:'虚构台灯',revision:1}],counts:{visits:2,orders:3,refunds:1,duplicates:1},alreadyImported:false};
const metrics={visits:2,visitingSessions:2,paidOrders:2,cancelledOrders:1,attributedOrders:1,unknownOrders:1,convertedSessions:1,grossMinor:2000,refundMinor:1000,netMinor:1000,attributedNetMinor:0,unknownNetMinor:1000,sessionConversionRate:0.5,unknownOrderRate:0.5,lookbackCoverageIncomplete:true};
const report={datasetId:'ds',contentHash:'hash',evidenceMode:'simulated',currency:'CNY',period:{from:dataset.coverageStart,to:dataset.coverageEnd,refundAsOf:dataset.coverageEnd,lookbackDays:7},summary:metrics,bySource:[{...metrics,source:'unknown'}],byProduct:[{...metrics,sku:'TEST',name:'虚构台灯快照'}],weekly:[{...metrics,weekStart:'2026-09-28T00:00:00Z'}],orders:[{orderId:'o',sku:'TEST',occurredAt:dataset.coverageStart,status:'paid',amountMinor:1000,refundMinor:1000,netMinor:0,source:'unknown',sourceEvidence:'unknown',visitId:null,reason:'missing_session',visitInPeriod:false}],limitations:['本地结果不证明因果增量'],files:[]};
const exchange={format:'m-geo-attribution-v1',evidenceMode:'simulated',sourceLabel:'虚构演练'};
let actions:{action:string;payload:Record<string,unknown>}[]=[];
afterEach(()=>{cleanup();setTestTransport(undefined);actions=[]});
function transport(override?:(action:string,payload:Record<string,unknown>)=>unknown){setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{actions.push({action,payload});const custom=override?.(action,payload);if(custom!==undefined)return await custom as T;return ({'attribution.list':{datasets:[dataset]},'attribution.template':{exchange,files:[]},'attribution.preview':preview,'attribution.apply':{datasetId:'ds',alreadyImported:false,evidenceMode:'simulated'},'attribution.report':report}[action]??null) as T})}
async function setup(){render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/模拟订单/})}
function input(){fireEvent.change(screen.getByLabelText('数据集名称'),{target:{value:'演练'}});fireEvent.change(screen.getByLabelText('交换 JSON'),{target:{value:'{"fictional":true}'}})}
async function inspect(){input();fireEvent.click(screen.getByRole('button',{name:'检查导入预览'}));await screen.findByRole('region',{name:'导入预览'})}
async function calculate(){fireEvent.change(screen.getByLabelText('已保存数据集'),{target:{value:'ds'}});fireEvent.click(screen.getByRole('button',{name:'计算归因'}));await screen.findByRole('region',{name:'归因结果'})}
it('loads a simulated template locally without saving and requires explicit preview then apply',async()=>{transport();await setup();fireEvent.click(screen.getByRole('button',{name:'填入模拟模板'}));await waitFor(()=>expect(screen.getByLabelText('交换 JSON')).toHaveValue(JSON.stringify(exchange,null,2)));expect(actions.some(c=>c.action==='attribution.apply')).toBe(false);expect(screen.getByRole('button',{name:'保存不可变数据集'})).toBeDisabled();fireEvent.change(screen.getByLabelText('数据集名称'),{target:{value:'演练'}});fireEvent.click(screen.getByRole('button',{name:'检查导入预览'}));await screen.findByRole('region',{name:'导入预览'});fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));await screen.findByText('数据集已保存');expect(actions.filter(c=>c.action==='attribution.apply')).toHaveLength(1);expect(actions.find(c=>c.action==='attribution.apply')?.payload).toMatchObject({brandId:'b',name:'演练',expectedHash:'hash'});expect(actions.filter(c=>c.action==='attribution.list')).toHaveLength(2)});
it('labels exact replay and applies it once',async()=>{transport(a=>a==='attribution.preview'?{...preview,alreadyImported:true}:a==='attribution.apply'?{datasetId:'ds',alreadyImported:true,evidenceMode:'simulated'}:undefined);await setup();await inspect();expect(screen.getByText('相同内容已导入，将复用现有数据集')).toBeInTheDocument();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));await screen.findByText('已复用现有数据集，未重复保存')});
it('after a committed save and failed refresh only retries loading',async()=>{let lists=0;transport(a=>a==='attribution.list'?(++lists===2?Promise.reject(new Error('refresh failed')):{datasets:[dataset]}):undefined);await setup();await inspect();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));await screen.findByText(/资料已保存，但列表刷新失败/);expect(screen.getByRole('button',{name:'保存不可变数据集'})).toBeDisabled();expect(screen.queryByRole('region',{name:'导入预览'})).toBeNull();fireEvent.click(screen.getByRole('button',{name:'重新加载资料'}));await screen.findByText('数据集已保存');expect(actions.filter(c=>c.action==='attribution.apply')).toHaveLength(1);expect(lists).toBe(3)});
it('ignores preview arriving after input changes',async()=>{let finish!:(v:unknown)=>void;transport(a=>a==='attribution.preview'?new Promise(resolve=>{finish=resolve}):undefined);await setup();input();fireEvent.click(screen.getByRole('button',{name:'检查导入预览'}));fireEvent.change(screen.getByLabelText('交换 JSON'),{target:{value:'changed'}});await act(async()=>finish(preview));expect(screen.queryByRole('region',{name:'导入预览'})).toBeNull();expect(screen.getByRole('button',{name:'保存不可变数据集'})).toBeDisabled()});
it('ignores a former brand preview and resets on product snapshot changes',async()=>{let finish!:(v:unknown)=>void;transport(a=>a==='attribution.preview'?new Promise(resolve=>{finish=resolve}):undefined);const view=render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/模拟订单/});input();fireEvent.click(screen.getByRole('button',{name:'检查导入预览'}));view.rerender(<AttributionWorkbench brandId="c" data={data}/>);await act(async()=>finish(preview));expect(screen.queryByRole('region',{name:'导入预览'})).toBeNull();expect(screen.getByLabelText('交换 JSON')).toHaveValue('');expect(screen.getByText('请先为当前品牌创建带 SKU 的商品。')).toBeInTheDocument()});
it('preserves text on file cancellation and report on export cancellation',async()=>{transport();await setup();input();fireEvent.click(screen.getByRole('button',{name:'选择本地 JSON'}));await screen.findByText('已取消选择，保留当前内容');expect(screen.getByLabelText('交换 JSON')).toHaveValue('{"fictional":true}');await calculate();fireEvent.click(screen.getByRole('button',{name:'导出归因证据 ZIP'}));await screen.findByText('已取消导出');expect(screen.getByRole('region',{name:'归因结果'})).toBeInTheDocument();expect(actions.find(c=>c.action==='file.exportAttribution')?.payload).toEqual(actions.find(c=>c.action==='attribution.report')?.payload)});
it('invalidates report and export when time or lookback changes and ignores late report',async()=>{let finish!:(v:unknown)=>void;let calls=0;transport(a=>a==='attribution.report'&&++calls===2?new Promise(resolve=>{finish=resolve}):undefined);await setup();await calculate();fireEvent.change(screen.getByLabelText('回溯天数'),{target:{value:'8'}});expect(screen.queryByRole('region',{name:'归因结果'})).toBeNull();expect(screen.getByRole('button',{name:'导出归因证据 ZIP'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'计算归因'}));fireEvent.change(screen.getByLabelText('结束时间（UTC，不含）'),{target:{value:'2026-10-07T00:00:01'}});await act(async()=>finish(report));expect(screen.queryByRole('region',{name:'归因结果'})).toBeNull()});
it('renders group metrics, currency, refund cutoff, unknown evidence and zero values',async()=>{transport();await setup();await calculate();const result=screen.getByRole('region',{name:'归因结果'});expect(within(result).getByText(/退款统计截至.*2026-10-08/)).toBeInTheDocument();expect(within(result).getByText('虚构台灯快照 · TEST')).toBeInTheDocument();expect(within(result).getAllByText('CNY 0.00').length).toBeGreaterThan(0);expect(within(result).getByText('缺少会话标识')).toBeInTheDocument();expect(within(result).getByText(/来源与周分组的会话数不可相加/)).toBeInTheDocument();expect(within(result).getAllByText(/回溯覆盖不完整/)).toHaveLength(4);expect(within(result).getByText(/本地结果不证明因果增量/)).toBeInTheDocument()});
it('displays null conversion as not applicable, not zero percent',async()=>{transport(a=>a==='attribution.report'?{...report,summary:{...metrics,visitingSessions:0,sessionConversionRate:null,unknownOrderRate:null},bySource:[],byProduct:[],weekly:[]}:undefined);await setup();await calculate();expect(within(screen.getByRole('region',{name:'汇总指标'})).getAllByText('不适用（分母为 0）')).toHaveLength(2)});
it('keeps edited content when an earlier template or file response arrives',async()=>{let finish!:(v:unknown)=>void;transport(a=>a==='attribution.template'?new Promise(resolve=>{finish=resolve}):undefined);await setup();fireEvent.click(screen.getByRole('button',{name:'填入模拟模板'}));fireEvent.change(screen.getByLabelText('交换 JSON'),{target:{value:'newer text'}});await act(async()=>finish({exchange,files:[]}));expect(screen.getByLabelText('交换 JSON')).toHaveValue('newer text')});

it('invalidates a valid preview when the saved workspace product revision changes',async()=>{transport();const view=render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/模拟订单/});await inspect();view.rerender(<AttributionWorkbench brandId="b" data={{...data,products:[{...data.products[0],revision:2}]}}/>);expect(screen.queryByRole('region',{name:'导入预览'})).toBeNull();expect(screen.getByRole('button',{name:'保存不可变数据集'})).toBeDisabled();expect(screen.getByLabelText('交换 JSON')).toHaveValue('')});
it('keeps UTC whole seconds, full coverage defaults and a bounded editable window',async()=>{transport();await setup();fireEvent.change(screen.getByLabelText('已保存数据集'),{target:{value:'ds'}});expect(screen.getByLabelText('开始时间（UTC，包含）')).toHaveValue('2026-10-01T00:00');expect(screen.getByLabelText('回溯天数')).toHaveValue(7);fireEvent.change(screen.getByLabelText('开始时间（UTC，包含）'),{target:{value:'2026-10-01T00:00:01'}});fireEvent.change(screen.getByLabelText('回溯天数'),{target:{value:'31'}});expect(screen.getByRole('button',{name:'计算归因'})).toBeDisabled();fireEvent.change(screen.getByLabelText('回溯天数'),{target:{value:'2'}});fireEvent.click(screen.getByRole('button',{name:'计算归因'}));await screen.findByRole('region',{name:'归因结果'});expect(actions.find(c=>c.action==='attribution.report')?.payload).toEqual({brandId:'b',datasetId:'ds',from:'2026-10-01T00:00:01Z',to:'2026-10-08T00:00:00Z',lookbackDays:2})});
it('does not replace edited import text with a late local file response',async()=>{let finish!:(v:unknown)=>void;transport(a=>a==='file.openText'?new Promise(resolve=>{finish=resolve}):undefined);await setup();input();fireEvent.click(screen.getByRole('button',{name:'选择本地 JSON'}));fireEvent.change(screen.getByLabelText('交换 JSON'),{target:{value:'newer text'}});await act(async()=>finish({name:'old.json',content:'old text'}));expect(screen.getByLabelText('交换 JSON')).toHaveValue('newer text')});
it('ignores stale export feedback after a dataset parameter change',async()=>{let finish!:(v:unknown)=>void;transport(a=>a==='file.exportAttribution'?new Promise(resolve=>{finish=resolve}):undefined);await setup();await calculate();fireEvent.click(screen.getByRole('button',{name:'导出归因证据 ZIP'}));fireEvent.change(screen.getByLabelText('回溯天数'),{target:{value:'3'}});await act(async()=>finish({path:'old.zip'}));expect(screen.queryByText('归因证据 ZIP 已保存')).toBeNull();expect(screen.queryByRole('region',{name:'归因结果'})).toBeNull();expect(screen.getByRole('button',{name:'导出归因证据 ZIP'})).toBeDisabled()});
it('restores dataset choices on page reentry but requires a fresh calculation',async()=>{transport();const view=render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/模拟订单/});await calculate();view.unmount();render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/模拟订单/});expect(screen.getByLabelText('已保存数据集')).toHaveValue('');expect(screen.getByRole('button',{name:'导出归因证据 ZIP'})).toBeDisabled();expect(actions.filter(c=>c.action==='attribution.apply')).toHaveLength(0)});

it('selects a newly persisted dataset with full coverage after list refresh',async()=>{let lists=0;const saved={...dataset,id:'new-ds',name:'新保存演练'};transport(a=>a==='attribution.list'?{datasets:++lists===1?[]:[saved]}:a==='attribution.apply'?{datasetId:'new-ds',alreadyImported:false,evidenceMode:'simulated'}:undefined);render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByText('尚无数据集，请先检查并明确保存。');await inspect();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));await screen.findByText('数据集已保存');expect(screen.getByLabelText('已保存数据集')).toHaveValue('new-ds');expect(screen.getByLabelText('开始时间（UTC，包含）')).toHaveValue('2026-10-01T00:00');expect(screen.getByRole('button',{name:'计算归因'})).toBeEnabled()});

it('sets new dataset defaults after reload-only recovery of a committed write',async()=>{let lists=0;const saved={...dataset,id:'new-ds',name:'新保存演练'};transport(a=>a==='attribution.list'?(++lists===1?{datasets:[]}:lists===2?Promise.reject(new Error('refresh failed')):{datasets:[saved]}):a==='attribution.apply'?{datasetId:'new-ds',alreadyImported:false,evidenceMode:'simulated'}:undefined);render(<AttributionWorkbench brandId="b" data={data}/>);await screen.findByText('尚无数据集，请先检查并明确保存。');await inspect();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));await screen.findByText(/资料已保存，但列表刷新失败/);fireEvent.click(screen.getByRole('button',{name:'重新加载资料'}));await screen.findByText('数据集已保存');expect(screen.getByLabelText('已保存数据集')).toHaveValue('new-ds');expect(screen.getByRole('button',{name:'计算归因'})).toBeEnabled();expect(actions.filter(c=>c.action==='attribution.apply')).toHaveLength(1)});

it('ignores a superseded initial list failure after the saved-dataset reload succeeds',async()=>{
 let lists=0,rejectInitial!:(reason:Error)=>void;
 transport(a=>a==='attribution.list'&&++lists===1?new Promise((_,reject)=>{rejectInitial=reject}):undefined);
 render(<AttributionWorkbench brandId="b" data={data}/>);
 await inspect();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));
 await screen.findByText('数据集已保存');
 await act(async()=>rejectInitial(new Error('obsolete initial list error')));
 expect(screen.getByLabelText('已保存数据集')).toHaveValue('ds');
 expect(screen.getByRole('button',{name:'计算归因'})).toBeEnabled();
 expect(screen.queryByText('obsolete initial list error')).toBeNull();
 expect(screen.queryByText('正在加载数据集…')).toBeNull();
});

it('keeps loading the newer list when the superseded initial list completes',async()=>{
 let lists=0,finishInitial!:(value:unknown)=>void,finishSaved!:(value:unknown)=>void;
 transport(a=>a==='attribution.list'?new Promise(resolve=>{if(++lists===1)finishInitial=resolve;else finishSaved=resolve}):undefined);
 render(<AttributionWorkbench brandId="b" data={data}/>);
 await inspect();fireEvent.click(screen.getByRole('button',{name:'保存不可变数据集'}));
 await waitFor(()=>expect(lists).toBe(2));
 await act(async()=>finishInitial({datasets:[]}));
 expect(screen.getByText('正在加载数据集…')).toBeInTheDocument();
 await act(async()=>finishSaved({datasets:[dataset]}));
 await screen.findByText('数据集已保存');
 expect(screen.queryByText('正在加载数据集…')).toBeNull();
 expect(screen.getByLabelText('已保存数据集')).toHaveValue('ds');
});

it('keeps the brand-keyed history inspector across same-brand snapshot refresh',async()=>{
 const history={
  inspectionId:'history-ticket',
  report:{
   format:'m-geo-attribution-inspection-v1',
   profile:'attribution-v1',
   source:{fileName:'frozen-history.zip',sha256:'a'.repeat(64),compressedBytes:100,uncompressedBytes:200,fileCount:9},
   brandId:'b',datasetId:'old-dataset',contentHash:'b'.repeat(64),
   evidenceMode:'simulated',evidenceLabel:'模拟数据',status:'passed',currency:'CNY',
   coverage:{from:dataset.coverageStart,to:dataset.coverageEnd},period:report.period,
   counts:{visits:2,orders:0,refunds:1,products:0},summary:metrics,
   bySource:[],weekly:[],checks:[],
   caveat:'文件一致性不证明因果销量',reportFingerprint:'c'.repeat(64),
  },
 };
 transport((action,payload)=>action==='file.inspectAttributionReport'?history:action==='file.readAttributionInspectionPage'?{inspectionId:payload.inspectionId,kind:payload.kind,page:0,pageSize:20,total:0,rows:[]}:undefined);
 const view=render(<AttributionWorkbench brandId="b" data={data}/>);
 await screen.findByRole('option',{name:/模拟订单/});
 fireEvent.click(screen.getByRole('button',{name:'选择历史归因 ZIP'}));await screen.findByText('frozen-history.zip');
 view.rerender(<AttributionWorkbench brandId="b" data={{...data,products:[{...data.products[0],revision:2}]}}/>);
 expect(screen.getByText('frozen-history.zip')).toBeInTheDocument();
 expect(actions.filter(c=>c.action==='file.inspectAttributionReport')).toHaveLength(1);
 view.rerender(<AttributionWorkbench brandId="c" data={data}/>);
 expect(screen.queryByText('frozen-history.zip')).toBeNull();
 expect(screen.getByText('尚未选择历史归因 ZIP。')).toBeInTheDocument();
});

it('allows historical selection without current products, with a truncated snapshot and a failed dataset list',async()=>{
 transport(action=>action==='attribution.list'?Promise.reject(new Error('数据集列表不可用')):undefined);
 render(<AttributionWorkbench brandId="b" data={{...data,products:[],truncated:true}}/>);
 await screen.findByText('数据集列表不可用');
 expect(screen.getByRole('button',{name:'选择历史归因 ZIP'})).toBeEnabled();
 fireEvent.click(screen.getByRole('button',{name:'选择历史归因 ZIP'}));
 await screen.findByText('已取消选择历史归因 ZIP');
 expect(actions.find(c=>c.action==='file.inspectAttributionReport')?.payload).toEqual({brandId:'b'});
 expect(actions.some(c=>['attribution.apply','product.save','workspace.restore'].includes(c.action))).toBe(false);
});
