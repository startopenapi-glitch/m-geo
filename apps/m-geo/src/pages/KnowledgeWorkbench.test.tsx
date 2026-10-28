import {act,cleanup,fireEvent,render,screen,waitFor,within} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {PageProps} from '../App';
import type {KnowledgeDocument,KnowledgeDocumentSummary} from '../knowledgeTypes';
import KnowledgeWorkbench from './KnowledgeWorkbench';

const product={id:'p',brandId:'b',name:'虚构台灯',sku:'TEST-LAMP',category:'灯具',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
const props:PageProps={brand:{id:'b',name:'虚构品牌',createdAt:''},data:{products:[product],questions:[],drafts:[],observations:[],publications:[]},refresh:vi.fn(async()=>{})};
function summary(overrides:Partial<KnowledgeDocumentSummary>={}):KnowledgeDocumentSummary{return {id:'doc-b',brandId:'b',productId:'p',title:'虚构台灯保养指南',source:'虚构产品手册',format:'markdown',effectiveAt:'2026-11-01',risk:'low',reviewStatus:'pending',reviewedAt:null,revision:7,chunkCount:1,createdAt:'2026-10-01T08:00:00Z',updatedAt:'2026-10-02T08:00:00Z',...overrides}}
function detail(overrides:Partial<KnowledgeDocument>={}):KnowledgeDocument{return {...summary(),content:'# 保养指南\n\n每月清洁虚构灯罩。',chunks:[{id:'chunk-1',ordinal:1,text:'每月清洁虚构灯罩。',sha256:'fictional-sha256'}],reviews:[{id:'review-old',documentId:'doc-b',revision:6,decision:'rejected',note:'旧版依据不足',createdAt:'2026-10-02T09:00:00Z'}],...overrides}}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('loads_summaries_then_lazily_previews_detail_chunks_and_review_history',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});if(action==='knowledge.list')return (payload.documentId?detail():[summary()]) as T;throw Error(action)});
 render(<KnowledgeWorkbench {...props}/>);
 const item=await screen.findByRole('button',{name:/虚构台灯保养指南/});
 expect(item).toHaveAttribute('aria-pressed','false');
 expect(item.textContent).toContain('1 个切片');
 expect(calls).toEqual([{action:'knowledge.list',payload:{brandId:'b'}}]);
 expect(screen.queryByText('每月清洁虚构灯罩。')).toBeNull();
 fireEvent.click(item);
 const region=await screen.findByRole('region',{name:'文档详情'});
 expect(calls[1]).toEqual({action:'knowledge.list',payload:{brandId:'b',documentId:'doc-b'}});
 expect(within(region).getByText('虚构台灯')).toBeInTheDocument();
 expect(within(region).getByText('低风险')).toBeInTheDocument();
 expect(within(region).getByText('待审核')).toBeInTheDocument();
 expect(within(region).getByText('每月清洁虚构灯罩。')).toBeInTheDocument();
 expect(within(region).getByText('切片 1')).toBeInTheDocument();
 expect(within(region).getByText('SHA-256：fictional-sha256')).toBeInTheDocument();
 expect(within(region).getByText('审核历史')).toBeInTheDocument();
 expect(within(region).getByText('旧版依据不足')).toBeInTheDocument();
 expect(within(region).getByText('已驳回 · 修订 6')).toBeInTheDocument();
 expect(calls.filter(call=>call.action==='knowledge.review')).toHaveLength(0);
});

it('creates_pending_document_with_date_and_uses_returned_detail_without_review',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='knowledge.list')return [] as T;
  if(action==='knowledge.import')return detail({id:'created',title:payload.title as string,source:payload.source as string,format:payload.format as KnowledgeDocument['format'],effectiveAt:payload.effectiveAt as string,risk:payload.risk as KnowledgeDocument['risk'],content:payload.content as string,reviewStatus:'pending',revision:1,chunkCount:0,chunks:[],reviews:[]}) as T;
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await screen.findByText('还没有知识文档');
 expect(screen.getByLabelText('文档标题')).toHaveAttribute('maxlength','300');
 expect(screen.getByLabelText('资料来源')).toHaveAttribute('maxlength','2000');
 expect(screen.getByLabelText('生效日期（可选）')).toHaveAttribute('type','date');
 fireEvent.change(screen.getByLabelText('关联商品'),{target:{value:'p'}});
 fireEvent.change(screen.getByLabelText('文档标题'),{target:{value:'虚构安装说明'}});
 fireEvent.change(screen.getByLabelText('资料来源'),{target:{value:'虚构安装手册'}});
 fireEvent.change(screen.getByLabelText('内容格式'),{target:{value:'text'}});
 fireEvent.change(screen.getByLabelText('生效日期（可选）'),{target:{value:'2026-12-01'}});
 fireEvent.change(screen.getByLabelText('风险等级'),{target:{value:'high'}});
 fireEvent.change(screen.getByLabelText('正文'),{target:{value:'仅供演示的虚构安装正文'}});
 fireEvent.click(screen.getByRole('button',{name:'创建待审核文档'}));
 await screen.findByText(/知识文档已创建，状态为待审核/);
 const request=calls.find(call=>call.action==='knowledge.import');
 expect(request?.payload).toEqual({brandId:'b',productId:'p',title:'虚构安装说明',source:'虚构安装手册',format:'text',effectiveAt:'2026-12-01',risk:'high',content:'仅供演示的虚构安装正文',requestId:expect.any(String)});
 expect(calls.filter(call=>call.action==='knowledge.review')).toHaveLength(0);
 expect(calls.filter(call=>call.action==='knowledge.list')).toHaveLength(1);
 expect(screen.getByLabelText('文档标题')).toHaveValue('');
 expect(screen.getByRole('button',{name:/虚构安装说明/})).toHaveAttribute('aria-pressed','true');
 expect(screen.getByText('尚无审核记录。')).toBeInTheDocument();
});

it('preserves_unsubmitted_input_when_creation_fails',async()=>{
 setTestTransport(async<T,>(action:string)=>{if(action==='knowledge.list')return [] as T;throw Error('虚构保存失败')});
 render(<KnowledgeWorkbench {...props}/>);
 await screen.findByText('还没有知识文档');
 fillRequiredFields();
 fireEvent.change(screen.getByLabelText('关联商品'),{target:{value:'p'}});
 fireEvent.click(screen.getByRole('button',{name:'创建待审核文档'}));
 await screen.findByText('虚构保存失败');
 expect(screen.getByLabelText('文档标题')).toHaveValue('虚构安装说明');
 expect(screen.getByLabelText('资料来源')).toHaveValue('虚构安装手册');
 expect(screen.getByLabelText('正文')).toHaveValue('虚构正文');
 expect(screen.getByLabelText('关联商品')).toHaveValue('p');
});

it('approves_with_optional_blank_note_and_prevents_duplicate_review',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];let finish!:(value:KnowledgeDocument)=>void;
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='knowledge.list')return (payload.documentId?detail():[summary()]) as T;
  if(action==='knowledge.review')return await new Promise<T>(resolve=>{finish=value=>resolve(value as T)});
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await selectDocument();
 fireEvent.click(screen.getByRole('button',{name:'人工审核通过'}));
 fireEvent.click(screen.getByRole('button',{name:'人工审核通过'}));
 await waitFor(()=>expect(calls.filter(call=>call.action==='knowledge.review')).toHaveLength(1));
 expect(calls.find(call=>call.action==='knowledge.review')?.payload).toEqual({brandId:'b',id:'doc-b',expectedRevision:7,decision:'approved',note:''});
 await act(async()=>finish(detail({reviewStatus:'approved',reviewedAt:'2026-10-03T09:00:00Z',revision:8,reviews:[{id:'review-new',documentId:'doc-b',revision:8,decision:'approved',note:'',createdAt:'2026-10-03T09:00:00Z'}]})));
 await screen.findByText('知识文档已人工审核通过');
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeDisabled();
 expect(screen.getByText('已通过 · 修订 8')).toBeInTheDocument();
 expect(screen.getByText('未填写审核理由')).toBeInTheDocument();
});

it('requires_rejection_note_and_saves_it_in_review_history',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='knowledge.list')return (payload.documentId?detail():[summary()]) as T;
  if(action==='knowledge.review')return detail({reviewStatus:'rejected',reviewedAt:'2026-10-03T09:00:00Z',revision:8,reviews:[{id:'review-new',documentId:'doc-b',revision:8,decision:'rejected',note:payload.note as string,createdAt:'2026-10-03T09:00:00Z'}]}) as T;
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await selectDocument();
 expect(screen.getByRole('button',{name:'人工驳回'})).toBeDisabled();
 fireEvent.change(screen.getByLabelText('审核理由（驳回必填）'),{target:{value:'  当前版本缺少依据  '}});
 fireEvent.click(screen.getByRole('button',{name:'人工驳回'}));
 await screen.findByText('知识文档已人工驳回');
 expect(calls.find(call=>call.action==='knowledge.review')?.payload).toEqual({brandId:'b',id:'doc-b',expectedRevision:7,decision:'rejected',note:'当前版本缺少依据'});
 expect(screen.getByText('当前版本缺少依据')).toBeInTheDocument();
 expect(screen.getByLabelText('审核理由（驳回必填）')).toHaveValue('');
});

it('preserves_review_note_when_review_fails',async()=>{
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{if(action==='knowledge.list')return (payload.documentId?detail():[summary()]) as T;throw Error('审核保存失败')});
 render(<KnowledgeWorkbench {...props}/>);
 await selectDocument();
 fireEvent.change(screen.getByLabelText('审核理由（驳回必填）'),{target:{value:'保留这条驳回理由'}});
 fireEvent.click(screen.getByRole('button',{name:'人工驳回'}));
 await screen.findByText('审核保存失败');
 expect(screen.getByLabelText('审核理由（驳回必填）')).toHaveValue('保留这条驳回理由');
});

it('ignores_late_detail_after_selection_changes',async()=>{
 const pending=new Map<string,(value:KnowledgeDocument)=>void>();
 const first=summary(),second=summary({id:'doc-2',title:'第二份虚构文档'});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='knowledge.list'&&!payload.documentId)return [first,second] as T;
  if(action==='knowledge.list')return await new Promise<T>(resolve=>{pending.set(payload.documentId as string,value=>resolve(value as T))});
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 fireEvent.click(await screen.findByRole('button',{name:/虚构台灯保养指南/}));
 await waitFor(()=>expect(pending.has('doc-b')).toBe(true));
 fireEvent.click(screen.getByRole('button',{name:/第二份虚构文档/}));
 await waitFor(()=>expect(pending.has('doc-2')).toBe(true));
 await act(async()=>pending.get('doc-2')!(detail({id:'doc-2',title:'第二份虚构文档',content:'第二份正文',chunks:[],reviews:[],chunkCount:0})));
 await screen.findByText('第二份正文');
 await act(async()=>pending.get('doc-b')!(detail({content:'第一份迟到正文'})));
 expect(screen.queryByText('第一份迟到正文')).toBeNull();
 expect(screen.getByText('第二份正文')).toBeInTheDocument();
});

it('ignores_late_detail_after_brand_change',async()=>{
 let finish!:(value:KnowledgeDocument)=>void;
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='knowledge.list'&&payload.documentId)return await new Promise<T>(resolve=>{finish=value=>resolve(value as T)});
  if(action==='knowledge.list')return (payload.brandId==='b'?[summary()]:[]) as T;
  throw Error(action);
 });
 const view=render(<KnowledgeWorkbench {...props}/>);
 fireEvent.click(await screen.findByRole('button',{name:/虚构台灯保养指南/}));
 await screen.findByText('正在读取文档详情…');
 await waitFor(()=>expect(finish).toBeDefined());
 view.rerender(<KnowledgeWorkbench {...props} brand={{id:'c',name:'虚构新品牌',createdAt:''}}/>);
 await screen.findByText('还没有知识文档');
 await act(async()=>finish(detail({content:'旧品牌迟到正文'})));
 expect(screen.queryByText('旧品牌迟到正文')).toBeNull();
 expect(screen.queryByRole('region',{name:'文档详情'})).toBeNull();
});

it('ignores_late_summary_list_after_brand_change',async()=>{
 const pending=new Map<string,(value:KnowledgeDocumentSummary[])=>void>();
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{if(action!=='knowledge.list')throw Error(action);return await new Promise<T>(resolve=>{pending.set(payload.brandId as string,value=>resolve(value as T))})});
 const view=render(<KnowledgeWorkbench {...props}/>);
 await waitFor(()=>expect(pending.has('b')).toBe(true));
 view.rerender(<KnowledgeWorkbench {...props} brand={{id:'c',name:'虚构新品牌',createdAt:''}}/>);
 await waitFor(()=>expect(pending.has('c')).toBe(true));
 await act(async()=>pending.get('c')!([summary({id:'doc-c',brandId:'c',productId:null,title:'当前品牌文档'})]));
 await screen.findByRole('button',{name:/当前品牌文档/});
 await act(async()=>pending.get('b')!([summary({title:'旧品牌迟到文档'})]));
 expect(screen.queryByText('旧品牌迟到文档')).toBeNull();
 expect(screen.getByRole('button',{name:/当前品牌文档/})).toBeInTheDocument();
});

it('prevents_duplicate_creation_while_import_is_pending',async()=>{
 const calls:string[]=[];let finish!:(value:KnowledgeDocument)=>void;
 setTestTransport(async<T,>(action:string)=>{
  calls.push(action);
  if(action==='knowledge.list')return [] as T;
  if(action==='knowledge.import')return await new Promise<T>(resolve=>{finish=value=>resolve(value as T)});
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await screen.findByText('还没有知识文档');
 fillRequiredFields();
 const button=screen.getByRole('button',{name:'创建待审核文档'});
 fireEvent.click(button);fireEvent.click(button);
 await waitFor(()=>expect(calls.filter(action=>action==='knowledge.import')).toHaveLength(1));
 await act(async()=>finish(detail({id:'created',revision:1,reviews:[]})));
 await screen.findByText(/知识文档已创建/);
 expect(calls.filter(action=>action==='knowledge.import')).toHaveLength(1);
});

it('retries_summary_and_detail_reads_after_failures',async()=>{
 let lists=0,details=0;
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  expect(action).toBe('knowledge.list');
  if(payload.documentId){if(++details===1)throw Error('详情读取失败');return detail() as T}
  if(++lists===1)throw Error('列表读取失败');
  return [summary()] as T;
 });
 render(<KnowledgeWorkbench {...props}/>);
 await screen.findByText('列表读取失败');
 fireEvent.click(screen.getByRole('button',{name:'重试读取'}));
 fireEvent.click(await screen.findByRole('button',{name:/虚构台灯保养指南/}));
 await screen.findByText('详情读取失败');
 fireEvent.click(screen.getByRole('button',{name:'重试详情'}));
 await screen.findByRole('region',{name:'文档详情'});
 expect(lists).toBe(2);expect(details).toBe(2);
});

it('revises_an_approved_document_and_requires_fresh_review_without_losing_history',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 const approved=detail({reviewStatus:'approved',reviewedAt:'2026-10-03T09:00:00Z'});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='knowledge.list')return (payload.documentId?approved:[summary({reviewStatus:'approved',reviewedAt:'2026-10-03T09:00:00Z'})]) as T;
  if(action==='knowledge.revise')return detail({title:payload.title as string,content:payload.content as string,reviewStatus:'pending',reviewedAt:null,revision:8,reviews:approved.reviews}) as T;
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await selectDocument();
 fireEvent.click(screen.getByRole('button',{name:'修订此文档'}));
 expect(screen.getByLabelText('文档标题')).toHaveValue('虚构台灯保养指南');
 fireEvent.change(screen.getByLabelText('文档标题'),{target:{value:'虚构台灯保养指南 v2'}});
 fireEvent.change(screen.getByLabelText('正文'),{target:{value:'每月清洁虚构灯罩，先断开电源。'}});
 fireEvent.click(screen.getByRole('button',{name:'保存修订并重新审核'}));
 await screen.findByText('知识文档已修订，旧审核已失效，请重新审核');
 const request=calls.find(call=>call.action==='knowledge.revise');
 expect(request?.payload).toEqual({brandId:'b',id:'doc-b',expectedRevision:7,productId:'p',title:'虚构台灯保养指南 v2',source:'虚构产品手册',format:'markdown',effectiveAt:'2026-11-01',risk:'low',content:'每月清洁虚构灯罩，先断开电源。',requestId:expect.any(String)});
 expect(screen.getByText('待审核',{selector:'dd'})).toBeInTheDocument();
 expect(screen.getByText('旧版依据不足')).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeEnabled();
});

it('preserves_revision_input_and_reuses_request_id_when_saving_fails',async()=>{
 const requests:Record<string,unknown>[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='knowledge.list')return (payload.documentId?detail():[summary()]) as T;
  if(action==='knowledge.revise'){requests.push(payload);throw Error('虚构修订失败')}
  throw Error(action);
 });
 render(<KnowledgeWorkbench {...props}/>);
 await selectDocument();fireEvent.click(screen.getByRole('button',{name:'修订此文档'}));
 fireEvent.change(screen.getByLabelText('正文'),{target:{value:'修订后的虚构正文'}});
 fireEvent.click(screen.getByRole('button',{name:'保存修订并重新审核'}));
 await screen.findByText('虚构修订失败');
 fireEvent.click(screen.getByRole('button',{name:'保存修订并重新审核'}));
 await waitFor(()=>expect(requests).toHaveLength(2));
 expect(requests[0].requestId).toBe(requests[1].requestId);
 expect(screen.getByLabelText('正文')).toHaveValue('修订后的虚构正文');
});

async function selectDocument(){fireEvent.click(await screen.findByRole('button',{name:/虚构台灯保养指南/}));await screen.findByRole('region',{name:'文档详情'})}
function fillRequiredFields(){
 fireEvent.change(screen.getByLabelText('文档标题'),{target:{value:'虚构安装说明'}});
 fireEvent.change(screen.getByLabelText('资料来源'),{target:{value:'虚构安装手册'}});
 fireEvent.change(screen.getByLabelText('正文'),{target:{value:'虚构正文'}});
}
