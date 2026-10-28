import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Brand,Draft,Product,WorkspaceSnapshot} from '../types';
import {DraftEditor,Jobs} from '../pages/Content';
import Publications from '../pages/Publications';
import {now} from './ui';
import {draftCanExport} from './draftEligibility';
const brand:Brand={id:'a',name:'A',createdAt:''};
const product:Product={id:'p',brandId:'a',name:'P',sku:'one',category:'',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-01-01T00:00:00Z',updatedAt:'',facts:[{id:'f',label:'size',value:'500ml',source:'official',visibility:'public',validUntil:null}]};
const draft:Draft={id:'d',brandId:'a',productId:'p',questionId:null,kind:'faq',title:'Title',body:'Stored body',claims:[{id:'c',text:'Stored body',factId:'f'}],revision:1,sourceRevision:1,generationMode:'local',reviewedAt:'2026-01-01T00:00:00Z',createdAt:''};
const data:WorkspaceSnapshot={products:[product],drafts:[draft],questions:[],publications:[],observations:[]};
afterEach(()=>{cleanup();setTestTransport(undefined);vi.restoreAllMocks();vi.useRealTimers()});
it('refreshes exactly on running-to-succeeded transition, before opening the new draft',async()=>{
 let status='running';const refresh=vi.fn(async()=>{});
 setTestTransport(async <T,>()=>[{id:'j',brandId:'a',productId:'p',kind:'faq',status,message:'',resultText:null,draftId:status==='succeeded'?'d':null,createdAt:''}] as T);
 render(<Jobs brand={brand} refresh={refresh} openDraft={()=>{}}/>);
 await screen.findByText('生成中');status='succeeded';fireEvent.click(screen.getByRole('button',{name:'刷新'}));
 await screen.findByText('已完成');expect(refresh).toHaveBeenCalledTimes(1);
 fireEvent.click(screen.getByRole('button',{name:'刷新'}));await waitFor(()=>expect(refresh).toHaveBeenCalledTimes(1));
});
it('formats datetime-local using local components rather than slicing UTC',()=>{
 vi.useFakeTimers();vi.setSystemTime(new Date('2026-10-17T04:30:00Z'));
 vi.spyOn(Date.prototype,'getFullYear').mockReturnValue(2026);vi.spyOn(Date.prototype,'getMonth').mockReturnValue(9);vi.spyOn(Date.prototype,'getDate').mockReturnValue(17);vi.spyOn(Date.prototype,'getHours').mockReturnValue(12);vi.spyOn(Date.prototype,'getMinutes').mockReturnValue(30);
 expect(now()).toBe('2026-10-17T12:30');
});
it.each(['expired','question'] as const)('blocks reviewed-copy and publication selection when source is %s',reason=>{
 const invalid={...data,products:reason==='expired'?[{...product,facts:[{...product.facts[0],validUntil:'2020-01-01'}]}]:[product],drafts:[{...draft,questionId:reason==='question'?'q':null}],questions:reason==='question'?[{id:'q',brandId:'a',productId:'p',text:'why',intent:'usage' as const,origin:'customer' as const,source:'',branded:false,confirmed:false}]:[]};
 const {unmount}=render(<DraftEditor brand={brand} data={invalid} refresh={async()=>{}} draft={invalid.drafts[0]}/>);
 expect(screen.queryByText('当前已审核')).not.toBeInTheDocument();expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();expect(screen.queryByRole('button',{name:'复制内容'})).not.toBeInTheDocument();
 unmount();render(<Publications brand={brand} data={invalid} refresh={async()=>{}}/>);
 expect(screen.queryByRole('option',{name:'Title · v1'})).not.toBeInTheDocument();
});
it('copies only the content returned by a successful native export gate',async()=>{
 const writeText=vi.fn(async()=>{});Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText}});let request:any;
 setTestTransport(async <T,>(action:string,p:any)=>{expect(action).toBe('draft.export');request=p;return {fileName:'Title.md',content:'# Native checked content'} as T});
 render(<DraftEditor brand={brand} data={data} refresh={async()=>{}} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'复制内容'}));
 await waitFor(()=>expect(writeText).toHaveBeenCalledWith('# Native checked content'));
 expect(request).toEqual({brandId:'a',id:'d',expectedRevision:1,format:'markdown'});
});
it('does not copy when the native gate rejects newly expired source facts',async()=>{
 const writeText=vi.fn(async()=>{});Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText}});
 setTestTransport(async()=>{throw Error('商品公开事实已过期')});
 render(<DraftEditor brand={brand} data={data} refresh={async()=>{}} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'复制内容'}));
 expect(await screen.findByRole('alert')).toHaveTextContent('商品公开事实已过期');expect(writeText).not.toHaveBeenCalled();
});

it('uses inclusive UTC expiry dates and ignores expired private-only material',()=>{
 const expiring={...data,products:[{...product,facts:[{...product.facts[0],validUntil:'2026-10-17'},{...product.facts[0],id:'private',visibility:'private' as const,validUntil:'2000-01-01'}]}]};
 expect(draftCanExport(expiring,draft,'a',new Date('2026-10-17T23:59:59.999Z'))).toBe(true);
 expect(draftCanExport(expiring,draft,'a',new Date('2026-10-18T00:00:00Z'))).toBe(false);
});
it.each(['pending','rejected'] as const)('blocks export when linked evidence is %s',reviewStatus=>{
 const blocked={...data,products:[{...product,facts:[{...product.facts[0],reviewStatus,reviewedAt:null,updatedAt:'2026-10-24T00:00:00Z'}]}]};
 expect(draftCanExport(blocked,draft,'a',new Date('2026-10-24T00:00:00Z'))).toBe(false);
});
it('does not write delayed exported content after its brand page unmounts',async()=>{
 const writeText=vi.fn(async()=>{});Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText}});let finish!:(v:any)=>void;
 setTestTransport(async <T,>()=>await new Promise<T>(resolve=>{finish=resolve}));
 const {unmount}=render(<DraftEditor brand={brand} data={data} refresh={async()=>{}} draft={draft}/>);
 fireEvent.click(screen.getByRole('button',{name:'复制内容'}));unmount();
 await act(async()=>{finish({fileName:'old.md',content:'Old brand content'})});
 expect(writeText).not.toHaveBeenCalled();
});
