import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import KnowledgeRecall from '../components/KnowledgeRecall';
import type {KnowledgeRecallHit} from '../knowledgeTypes';
import type {Draft,Product,WorkspaceSnapshot} from '../types';
import {DraftEditor} from './Content';

const hit:KnowledgeRecallHit={documentId:'doc',documentRevision:4,documentTitle:'虚构保温杯手册',source:'虚构规格资料第 2 页',productId:'p',risk:'medium',effectiveAt:'2026-10-24',reviewedAt:'2026-10-24T00:00:00Z',chunkId:'chunk',chunkOrdinal:0,text:'容量为虚构 500mL。',sha256:'a'.repeat(64),score:38};
function deferred<T>(){let resolve!:(value:T)=>void,reject!:(reason:Error)=>void;const promise=new Promise<T>((res,rej)=>{resolve=res;reject=rej});return{promise,resolve,reject}}
function search(value:string){fireEvent.change(screen.getByLabelText('知识召回关键词'),{target:{value}});fireEvent.click(screen.getByRole('button',{name:/检索已审核知识|检索中/}))}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('searches with explicit brand and product scope and renders traceable plain text',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});return[{...hit,text:'<img src=x onerror=alert(1)> 虚构纯文本'}] as T});
 const {container}=render(<KnowledgeRecall brandId="b" productId="p"/>);
 search('容量 500');
 await screen.findByText('虚构保温杯手册');
 expect(calls).toEqual([{action:'knowledge.recall',payload:{brandId:'b',productId:'p',query:'容量 500',limit:20}}]);
 expect(screen.getByText(/文档审核版本 v4/)).toHaveTextContent('来源：虚构规格资料第 2 页');
 expect(screen.getByText(`SHA-256 ${'a'.repeat(64)}`)).toBeInTheDocument();
 expect(screen.getByText('<img src=x onerror=alert(1)> 虚构纯文本')).toBeInTheDocument();
 expect(container.querySelector('img')).toBeNull();
 expect(screen.queryByRole('button',{name:'插入正文'})).toBeNull();
});

it('ignores a late older response after a newer search succeeds',async()=>{
 const first=deferred<KnowledgeRecallHit[]>(),second=deferred<KnowledgeRecallHit[]>();
 setTestTransport(async<T,>(_action:string,payload:Record<string,unknown>)=>(payload.query==='旧查询'?first.promise:second.promise) as T);
 render(<KnowledgeRecall brandId="b" productId="p"/>);
 search('旧查询');search('新查询');
 await act(async()=>second.resolve([{...hit,documentTitle:'新查询结果'}]));
 await screen.findByText('新查询结果');
 await act(async()=>first.resolve([{...hit,documentTitle:'迟到的旧结果'}]));
 expect(screen.getByText('新查询结果')).toBeInTheDocument();
 expect(screen.queryByText('迟到的旧结果')).toBeNull();
 expect(screen.getByText('上次成功检索：新查询 · 1 条')).toBeInTheDocument();
});

it('keeps the last successful results after failure and allows retry',async()=>{
 let attempt=0;
 setTestTransport(async<T,>(_action:string,payload:Record<string,unknown>)=>{if(payload.query==='失败')throw new Error('虚构读取失败');attempt++;return[{...hit,documentTitle:`成功结果 ${attempt}`}] as T});
 render(<KnowledgeRecall brandId="b" productId="p"/>);
 search('首次');await screen.findByText('成功结果 1');
 search('失败');await screen.findByRole('alert');
 expect(screen.getByText('成功结果 1')).toBeInTheDocument();
 expect(screen.getByRole('alert')).toHaveTextContent('虚构读取失败');
 search('重试');await screen.findByText('成功结果 2');
 expect(screen.queryByRole('alert')).toBeNull();
});

it('invalidates pending work and clears results when product scope changes',async()=>{
 const pending=deferred<KnowledgeRecallHit[]>();
 setTestTransport(async<T,>()=>pending.promise as T);
 const view=render(<KnowledgeRecall brandId="b" productId="p"/>);
 search('旧商品');
 view.rerender(<KnowledgeRecall brandId="b" productId="other"/>);
 await act(async()=>pending.resolve([{...hit,documentTitle:'旧商品迟到结果'}]));
 expect(screen.queryByText('旧商品迟到结果')).toBeNull();
 expect(screen.getByLabelText('知识召回关键词')).toHaveValue('');
 expect(screen.queryByText(/上次成功检索/)).toBeNull();
 view.unmount();
});

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'capacity',label:'容量',value:'500mL',source:'虚构规格表第 2 页',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构介绍',body:'容量：500mL',claims:[{id:'c',text:'容量：500mL',factId:'capacity'}],assetIds:[],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:null,createdAt:''};
it('integrates recall into the draft editor without modifying or approving the draft',async()=>{
 const actions:string[]=[];
 setTestTransport(async<T,>(action:string)=>{actions.push(action);return[hit] as T});
 const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[],observations:[],assets:[]};
 render(<DraftEditor brand={{id:'b',name:'虚构品牌',createdAt:''}} data={data} refresh={vi.fn(async()=>{})} draft={draft}/>);
 search('容量');
 await screen.findByTestId('knowledge-recall-hit');
 expect(actions).toEqual(['knowledge.recall']);
 expect(screen.getByLabelText('草稿标题')).toHaveValue(draft.title);
 expect(screen.getByLabelText('草稿正文')).toHaveValue(draft.body);
 expect(screen.getByRole('button',{name:'保存修改'})).toBeDisabled();
 expect(screen.getByRole('button',{name:/确认内容并审核/})).toBeEnabled();
 await waitFor(()=>expect(screen.getByText(/不是 AI 语义检索/)).toBeInTheDocument());
});
