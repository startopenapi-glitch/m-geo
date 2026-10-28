import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Brand,WorkspaceSnapshot} from '../types';
import type {OfflineWorkflow} from '../offlineWorkflow';
import OfflineWorkflowWorkbench from './OfflineWorkflowWorkbench';

const brand:Brand={id:'b',name:'虚构星灯',createdAt:'2026-10-28'};
const data:WorkspaceSnapshot={products:[{id:'p',brandId:'b',name:'虚构阅读灯',sku:'F001',category:'台灯',officialUrl:'',sourceText:'公开资料',privateNotes:'',facts:[],revision:1,confirmedAt:'2026-10-28',updatedAt:'2026-10-28'},{id:'unconfirmed',brandId:'b',name:'未确认商品',sku:'',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:'2026-10-28'},{id:'foreign',brandId:'other',name:'其他品牌商品',sku:'',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:'2026-10-28',updatedAt:'2026-10-28'}],questions:[],drafts:[],publications:[],observations:[]};
function fixture(status:OfflineWorkflow['status']='paused'):OfflineWorkflow{return {id:'task',brandId:'b',productId:'p',productRevision:1,requestId:'request-1',name:'虚构本地任务',revision:1,status,questions:['选购阅读灯应关注哪些指标？'],repetitions:1,draftId:null,createdAt:'2026-10-28',updatedAt:'2026-10-28',error:null,budget:{maxSamples:60,usedSamples:0,networkRequests:0,modelCalls:0,publications:0},nodes:['baseline','rewrite','audit','review','followup'].map((kind,i)=>({id:kind,kind:kind as OfflineWorkflow['nodes'][number]['kind'],dependsOn:i?[['baseline','rewrite','audit','review'][i-1]]:[],status:'pending',result:null,error:null}))}}
let tasks:OfflineWorkflow[]=[],calls:{action:string;payload:Record<string,unknown>}[]=[];
type Override=(action:string,payload:Record<string,unknown>)=>unknown;
function native(override?:Override){setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});const custom=override?.(action,payload);if(custom!==undefined)return await custom as T;
 if(action==='offlineWorkflow.list')return {tasks:structuredClone(tasks)} as T;
 if(action==='offlineWorkflow.get')return structuredClone(tasks.find(t=>t.id===payload.taskId)) as T;
 if(action==='offlineWorkflow.create'){const task={...fixture(),requestId:String(payload.requestId),name:String(payload.name),questions:payload.questions as string[],repetitions:Number(payload.repetitions)};tasks=[task];return structuredClone(task) as T}
 if(['resume','pause','cancel','retry'].some(a=>action===`offlineWorkflow.${a}`)){tasks=tasks.map(t=>({...t,status:action.endsWith('pause')||action.endsWith('retry')?'paused':action.endsWith('cancel')?'cancelled':'running',revision:t.revision+1}));return structuredClone(tasks[0]) as T}
 if(action==='file.exportOfflineWorkflow')return null as T;
 if(action==='offlineGeo.audit')return {scope:'offline_html',checks:[{id:'title',label:'页面标题',status:'pass',detail:'标题存在'}],passed:1,warning:0,blocking:0,networkRequests:0} as T;
 throw new Error(`Unexpected action: ${action}`);
})}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.useRealTimers();tasks=[];calls=[]});
const count=(action:string)=>calls.filter(c=>c.action===action).length;
async function setup(props:Partial<React.ComponentProps<typeof OfflineWorkflowWorkbench>>={}){const before=count('offlineWorkflow.list');const view=render(<OfflineWorkflowWorkbench brand={brand} data={data} refresh={async()=>{}} {...props}/>);await waitFor(()=>expect(count('offlineWorkflow.list')).toBe(before+1));return view}
async function create(){fireEvent.change(screen.getByLabelText('已确认商品'),{target:{value:'p'}});fireEvent.change(screen.getByLabelText('任务名称'),{target:{value:'虚构本地任务'}});fireEvent.click(screen.getByRole('button',{name:'创建暂停任务'}));await screen.findByRole('region',{name:'本地任务明细'})}
async function select(){await screen.findByRole('option',{name:/虚构本地任务/});fireEvent.change(screen.getByLabelText('已保存本地任务'),{target:{value:'task'}});await screen.findByRole('region',{name:'本地任务明细'})}

it('creates a paused simulated task once and starts only on explicit request',async()=>{
 native();await setup();expect(screen.queryByRole('option',{name:'未确认商品'})).toBeNull();expect(screen.queryByRole('option',{name:'其他品牌商品'})).toBeNull();
 await create();expect(screen.getByText('状态：已暂停')).toBeInTheDocument();expect(count('offlineWorkflow.resume')).toBe(0);
 const payload=calls.find(c=>c.action==='offlineWorkflow.create')!.payload;
 expect(payload).toMatchObject({brandId:'b',name:'虚构本地任务',productId:'p',repetitions:1,maxSamples:60,evidenceMode:'simulated'});expect(payload.requestId).toMatch(/^[0-9a-f-]{36}$/);expect(payload.questions).toHaveLength(3);
 fireEvent.click(screen.getByRole('button',{name:'开始 / 继续'}));await screen.findByText('状态：运行中');expect(calls.find(c=>c.action==='offlineWorkflow.resume')?.payload).toEqual({brandId:'b',taskId:'task',expectedRevision:1});
 expect(screen.getAllByText(/零模型调用.*零联网.*零发布/).length).toBeGreaterThan(0);expect(calls.some(c=>/step|realAi|ai\./.test(c.action))).toBe(false);
});
it('locks repeated create clicks and navigation while the native write is pending',async()=>{
 let finish!:(value:OfflineWorkflow)=>void;const busy=vi.fn();native(a=>a==='offlineWorkflow.create'?new Promise<OfflineWorkflow>(r=>{finish=r}):undefined);
 await setup({onBusyChange:busy});fireEvent.change(screen.getByLabelText('已确认商品'),{target:{value:'p'}});fireEvent.change(screen.getByLabelText('任务名称'),{target:{value:'虚构本地任务'}});
 const button=screen.getByRole('button',{name:'创建暂停任务'});fireEvent.click(button);fireEvent.click(button);expect(button).toBeDisabled();expect(busy).toHaveBeenLastCalledWith(true);expect(count('offlineWorkflow.create')).toBe(1);
 await act(async()=>finish(fixture()));await screen.findByRole('region',{name:'本地任务明细'});expect(busy).toHaveBeenLastCalledWith(false);
});
it('preserves the original request and payload after an uncertain create instead of duplicating',async()=>{
 let attempts=0;native((a,p)=>{if(a==='offlineWorkflow.create'){if(++attempts===1)return Promise.reject(new Error('响应丢失'));return {...fixture(),requestId:String(p.requestId)}}});await setup();
 fireEvent.change(screen.getByLabelText('已确认商品'),{target:{value:'p'}});fireEvent.change(screen.getByLabelText('任务名称'),{target:{value:'虚构本地任务'}});fireEvent.click(screen.getByRole('button',{name:'创建暂停任务'}));await screen.findByText(/创建结果未确认/);
 expect(screen.getByRole('button',{name:'创建暂停任务'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载任务'}));await screen.findByRole('button',{name:'重试原创建请求'});fireEvent.click(screen.getByRole('button',{name:'重试原创建请求'}));await screen.findByRole('region',{name:'本地任务明细'});
 expect(calls.filter(c=>c.action==='offlineWorkflow.create').map(c=>c.payload)).toEqual([calls.find(c=>c.action==='offlineWorkflow.create')!.payload,calls.find(c=>c.action==='offlineWorkflow.create')!.payload]);
});
it('rejects branded questions and sample counts beyond the bounded budget',async()=>{
 native();await setup();fireEvent.change(screen.getByLabelText('已确认商品'),{target:{value:'p'}});fireEvent.change(screen.getByLabelText('任务名称'),{target:{value:'有界任务'}});
 fireEvent.change(screen.getByLabelText('中性问题（每行一条）'),{target:{value:'虚构星灯好用吗？'}});expect(screen.getByRole('button',{name:'创建暂停任务'})).toBeDisabled();expect(screen.getByText(/问题不能包含品牌/)).toBeInTheDocument();
 fireEvent.change(screen.getByLabelText('中性问题（每行一条）'),{target:{value:'怎么选？\n如何比较？'}});fireEvent.change(screen.getByLabelText('两轮样本预算'),{target:{value:'3'}});expect(screen.getByRole('button',{name:'创建暂停任务'})).toBeDisabled();expect(count('offlineWorkflow.create')).toBe(0);
});
it('polls native progress without advancing the worker and clears polling on unmount',async()=>{
 vi.useFakeTimers({toFake:['setInterval','clearInterval']});tasks=[fixture('running')];native();await setup();await select();tasks=[{...fixture('awaiting_review'),revision:2,draftId:'draft'}];
 await act(async()=>{await vi.advanceTimersByTimeAsync(1600)});expect(screen.getByText('状态：等待人工审核')).toBeInTheDocument();expect(count('offlineWorkflow.get')).toBeGreaterThan(1);expect(calls.every(c=>['offlineWorkflow.list','offlineWorkflow.get'].includes(c.action))).toBe(true);
 cleanup();const before=calls.length;await act(async()=>{await vi.advanceTimersByTimeAsync(5000)});expect(calls).toHaveLength(before);
});
it('shows the review gate and only navigates to existing Content for approval',async()=>{
 tasks=[{...fixture('awaiting_review'),draftId:'draft'}];native();const navigate=vi.fn();await setup({onNavigate:navigate});await select();expect(screen.getByText(/候选稿仍须在内容页人工审核/)).toBeInTheDocument();expect(screen.getByRole('button',{name:'导出本地证据 ZIP'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'前往内容审核'}));await waitFor(()=>expect(navigate).toHaveBeenCalledWith('content'));expect(calls.some(c=>c.action==='draft.review')).toBe(false);
});
it('supports pause, resume, cancel and retry using the latest revision',async()=>{
 tasks=[fixture('running')];native();await setup();await select();fireEvent.click(screen.getByRole('button',{name:'暂停任务'}));await screen.findByText('状态：已暂停');fireEvent.click(screen.getByRole('button',{name:'开始 / 继续'}));await screen.findByText('状态：运行中');fireEvent.click(screen.getByRole('button',{name:'取消任务'}));await screen.findByText('状态：已取消');
 expect(calls.filter(c=>/offlineWorkflow\.(pause|resume|cancel)$/.test(c.action)).map(c=>c.payload.expectedRevision)).toEqual([1,2,3]);
 cleanup();tasks=[{...fixture('failed'),error:'公开事实版本已变化'}];await setup();await select();expect(screen.getByText('公开事实版本已变化')).toBeInTheDocument();fireEvent.click(screen.getByRole('button',{name:'重试失败任务'}));await screen.findByText('状态：已暂停');expect(count('offlineWorkflow.retry')).toBe(1);expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeEnabled();
});
it('blocks uncertain mutations until a successful reload and never replays automatically',async()=>{
 tasks=[fixture()];native(a=>a==='offlineWorkflow.resume'?Promise.reject(new Error('版本已变化')):undefined);await setup();await select();fireEvent.click(screen.getByRole('button',{name:'开始 / 继续'}));await screen.findByText(/操作结果未确认/);expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeDisabled();expect(screen.getByRole('button',{name:'取消任务'})).toBeDisabled();expect(screen.getByRole('button',{name:'暂停任务'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载任务'}));await screen.findByText('已加载本地保存版本，请核对后继续');expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeEnabled();expect(count('offlineWorkflow.resume')).toBe(1);
});
it('reports cancelled and saved exports and locks navigation during the save dialog',async()=>{
 tasks=[fixture('completed')];let finish!:(value:{saved:boolean})=>void;let exports=0;const busy=vi.fn();native(a=>a==='file.exportOfflineWorkflow'&&++exports===2?new Promise(r=>{finish=r}):undefined);await setup({onBusyChange:busy});await select();fireEvent.click(screen.getByRole('button',{name:'导出本地证据 ZIP'}));await screen.findByText('已取消导出，任务记录保留');fireEvent.click(screen.getByRole('button',{name:'导出本地证据 ZIP'}));expect(busy).toHaveBeenLastCalledWith(true);expect(screen.getByRole('button',{name:'导出本地证据 ZIP'})).toBeDisabled();await act(async()=>finish({saved:true}));await screen.findByText('本地证据 ZIP 已保存');expect(busy).toHaveBeenLastCalledWith(false);expect(calls.find(c=>c.action==='file.exportOfflineWorkflow')?.payload).toEqual({brandId:'b',taskId:'task',expectedRevision:1});
});
it('checks bounded offline HTML as text without executing markup or using a URL request',async()=>{
 native();await setup();const html='<script>window.__offlineExecuted=true</script><h1>虚构</h1>';fireEvent.change(screen.getByLabelText('离线 HTML 源码'),{target:{value:html}});fireEvent.click(screen.getByRole('button',{name:'检查离线 HTML'}));await screen.findByText('标题存在');expect(calls.find(c=>c.action==='offlineGeo.audit')?.payload).toEqual({brandId:'b',html});expect((window as unknown as Record<string,unknown>).__offlineExecuted).toBeUndefined();
 fireEvent.change(screen.getByLabelText('离线 HTML 源码'),{target:{value:'字'.repeat(100000)}});expect(screen.getByRole('button',{name:'检查离线 HTML'})).toBeDisabled();expect(count('offlineGeo.audit')).toBe(1);
});
it('renders sample and audit results as text with the simulated comparison boundary',async()=>{
 const task=fixture('completed');task.nodes[0]={...task.nodes[0],status:'completed',result:{sampleCount:1,mentionCount:1,citationCount:1,samples:[{question:'怎么选？',answer:'<img src=x onerror=alert(1)>',citations:['fixture:source'],mentioned:true,evidenceMode:'simulated',surface:'fixture'}],networkRequests:0,modelCalls:0}};task.nodes[2]={...task.nodes[2],status:'completed',result:{scope:'offline_html',checks:[{id:'title',label:'标题',status:'warning',detail:'需要核对'}],passed:0,warning:1,blocking:0,networkRequests:0}};task.nodes[4]={...task.nodes[4],status:'completed',result:{sampleCount:1,mentionCount:1,citationCount:1,samples:[],mentionDelta:0,comparisonLabel:'模拟夹具对照，不能证明真实提升',networkRequests:0,modelCalls:0}};
 tasks=[task];native();await setup();await select();expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeInTheDocument();expect(screen.getByText('需要核对')).toBeInTheDocument();expect(screen.getByText('模拟夹具对照，不能证明真实提升')).toBeInTheDocument();expect(document.querySelector('img')).toBeNull();
});
it('ignores a late previous-brand response and resets selection after a brand change',async()=>{
 let finish!:(value:OfflineWorkflow)=>void;tasks=[fixture()];native(a=>a==='offlineWorkflow.get'?new Promise<OfflineWorkflow>(r=>{finish=r}):undefined);const view=await setup();fireEvent.change(screen.getByLabelText('已保存本地任务'),{target:{value:'task'}});view.rerender(<OfflineWorkflowWorkbench brand={{...brand,id:'other',name:'其他品牌'}} data={{...data,products:[]}} refresh={async()=>{}}/>);await act(async()=>finish(fixture()));expect(screen.queryByRole('region',{name:'本地任务明细'})).toBeNull();expect(screen.getByLabelText('已保存本地任务')).toHaveValue('');
});
it('manual reload supersedes a pending selection read and releases controls',async()=>{
 let finish!:(value:OfflineWorkflow)=>void,reads=0;tasks=[fixture()];native(a=>a==='offlineWorkflow.get'&&++reads===1?new Promise<OfflineWorkflow>(r=>{finish=r}):undefined);await setup();fireEvent.change(screen.getByLabelText('已保存本地任务'),{target:{value:'task'}});fireEvent.click(screen.getByRole('button',{name:'重新加载任务'}));await screen.findByText('已加载本地保存版本，请核对后继续');expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeEnabled();await act(async()=>finish({...fixture('failed'),revision:1}));expect(screen.getByText('状态：已暂停')).toBeInTheDocument();
});
it('loads the worker-created candidate before navigating to Content and holds the context lock',async()=>{
 let finish!:()=>void;tasks=[{...fixture('awaiting_review'),draftId:'draft'}];native();const navigate=vi.fn(),busy=vi.fn(),refresh=vi.fn(()=>new Promise<void>(r=>{finish=r}));await setup({refresh,onNavigate:navigate,onBusyChange:busy});await select();fireEvent.click(screen.getByRole('button',{name:'前往内容审核'}));expect(refresh).toHaveBeenCalledTimes(1);expect(navigate).not.toHaveBeenCalled();expect(busy).toHaveBeenLastCalledWith(true);await act(async()=>finish());expect(navigate).toHaveBeenCalledWith('content');expect(busy).toHaveBeenLastCalledWith(false);
});

it.each(['paused','failed'] as const)('lets a stale %s task cancel while resume retry and export stay blocked',async status=>{
 tasks=[fixture(status)];native();const staleData={...data,products:data.products.map(p=>p.id==='p'?{...p,revision:2}:p)};
 await setup({data:staleData});await select();
 expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'重试失败任务'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'导出本地证据 ZIP'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'取消任务'})).toBeEnabled();
 fireEvent.click(screen.getByRole('button',{name:'取消任务'}));await screen.findByText('状态：已取消');
 expect(calls.find(c=>c.action==='offlineWorkflow.cancel')?.payload).toEqual({brandId:'b',taskId:'task',expectedRevision:1});
 expect(count('offlineWorkflow.resume')).toBe(0);expect(count('offlineWorkflow.retry')).toBe(0);expect(count('file.exportOfflineWorkflow')).toBe(0);
});
it('lets a stale running task pause and cancel with the current revision',async()=>{
 tasks=[fixture('running')];native();await setup({data:{...data,products:data.products.map(p=>p.id==='p'?{...p,revision:2}:p)}});await select();
 expect(screen.getByRole('button',{name:'暂停任务'})).toBeEnabled();fireEvent.click(screen.getByRole('button',{name:'暂停任务'}));await screen.findByText('状态：已暂停');
 expect(screen.getByRole('button',{name:'开始 / 继续'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'取消任务'}));await screen.findByText('状态：已取消');
 expect(calls.filter(c=>/offlineWorkflow\.(pause|cancel)$/.test(c.action)).map(c=>c.payload.expectedRevision)).toEqual([1,2]);
});
