import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Draft,Product,WorkspaceSnapshot} from '../types';
import {DraftEditor} from '../pages/Content';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'capacity',label:'容量',value:'500mL',source:'虚构规格表',visibility:'public',validUntil:null},{id:'cost',label:'成本',value:'私密',source:'内部',visibility:'private',validUntil:null}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构介绍',body:'容量：500mL',claims:[],sourceRevision:1,revision:1,generationMode:'ai',reviewedAt:null,createdAt:''};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[],observations:[]};
const props={brand:{id:'b',name:'虚构品牌',createdAt:''},data,refresh:vi.fn(async()=>{})};
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('blocks legacy or AI drafts without claim evidence and explains the gap',async()=>{
 setTestTransport(async()=>{throw Error('请至少登记一条关键主张，并关联有来源的公开事实')});
 render(<DraftEditor {...props} draft={draft}/>);
 expect(screen.getByText(/尚未登记关键主张，当前不能审核或导出/)).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'确认内容并审核'}));
 expect(await screen.findByRole('alert')).toHaveTextContent('至少登记一条关键主张');
});

it('saves explicit claim-to-fact links and never offers private facts',async()=>{
 let payload:Record<string,unknown>|undefined;
 setTestTransport(async <T,>(action:string,p:Record<string,unknown>)=>{if(action==='draft.save'){payload=p;return {...draft,claims:p.claims,revision:2} as T}throw Error(action)});
 render(<DraftEditor {...props} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'登记关键主张'}));
 fireEvent.change(screen.getByLabelText('关键主张 1'),{target:{value:'容量：500mL'}});
 expect(screen.getByLabelText('依据事实 1')).not.toHaveTextContent('私密');
 fireEvent.change(screen.getByLabelText('依据事实 1'),{target:{value:'capacity'}});
 fireEvent.click(screen.getByRole('button',{name:'保存修改'}));
 await waitFor(()=>expect(payload).toBeDefined());
 expect(payload?.claims).toEqual([expect.objectContaining({text:'容量：500mL',factId:'capacity'})]);
});

it('keeps a reviewed linked draft exportable only until its evidence mapping changes',()=>{
 const linked={...draft,claims:[{id:'c',text:'容量：500mL',factId:'capacity'}],reviewedAt:'2026-10-24T01:00:00Z'};
 render(<DraftEditor {...props} draft={linked}/>);
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeEnabled();
 fireEvent.change(screen.getByLabelText('依据事实 1'),{target:{value:''}});
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'确认内容并审核'})).toBeDisabled();
});

it('never offers pending facts as evidence and blocks their reviewed drafts',()=>{
 const pending={...product,facts:[...product.facts,{id:'unreviewed',label:'未审核功效',value:'虚构待证主张',source:'待核对资料',visibility:'public' as const,validUntil:null,reviewStatus:'pending' as const,reviewedAt:null,updatedAt:'2026-10-24T00:00:00Z'}]};
 const linked={...draft,claims:[{id:'c',text:'容量：500mL',factId:'capacity'}],reviewedAt:'2026-10-24T01:00:00Z'};
 render(<DraftEditor {...props} data={{...data,products:[pending]}} draft={linked}/>);
 expect(screen.getByLabelText('依据事实 1')).not.toHaveTextContent('未审核功效');
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();
});

it('shows frozen review evidence and removes it when an edited draft is saved',async()=>{
 const linked:Draft={...draft,claims:[{id:'c',text:'容量：500mL',factId:'capacity',evidence:{snapshotVersion:1,factId:'capacity',label:'容量',value:'500mL',source:'虚构规格表',validUntil:null,factReviewedAt:'2026-10-24T00:30:00Z',factUpdatedAt:'2026-10-24T00:20:00Z'}}],reviewedAt:'2026-10-24T01:00:00Z'};
 let payload:Record<string,unknown>|undefined;
 setTestTransport(async <T,>(action:string,p:Record<string,unknown>)=>{if(action==='draft.save'){payload=p;return {...linked,claims:p.claims,reviewedAt:null,revision:2} as T}throw Error(action)});
 render(<DraftEditor {...props} draft={linked}/>);
 expect(screen.getByTestId('evidence-snapshot-0')).toHaveTextContent('审核快照：容量：500mL · 来源：虚构规格表');
 fireEvent.change(screen.getByLabelText('草稿正文'),{target:{value:'容量：500mL。新版正文'}});
 fireEvent.click(screen.getByRole('button',{name:'保存修改'}));
 await waitFor(()=>expect(payload).toBeDefined());
 expect(payload?.claims).toEqual([{id:'c',text:'容量：500mL',factId:'capacity'}]);
});
