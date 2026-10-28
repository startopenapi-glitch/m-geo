import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Product} from '../types';
import TrialPreparation from './TrialPreparation';
const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'TEST',category:'水杯',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
const props={brand:{id:'b',name:'虚构品牌',createdAt:''},data:{products:[product],questions:[],drafts:[],observations:[],publications:[]},refresh:vi.fn(async()=>{})};
afterEach(()=>{cleanup();setTestTransport(undefined)});
it('prepares without saving and invalidates on data changes',async()=>{
 const actions:string[]=[];
 setTestTransport(async <T,>(action:string)=>{actions.push(action);return {token:'t',products:[],candidates:[{productId:'p',text:'如何选水杯？',intent:'selection'}]} as T});
 const view=render(<TrialPreparation {...props}/>);
 fireEvent.click(screen.getByRole('checkbox',{name:/虚构水杯/}));
 fireEvent.click(screen.getByText('准备试点资料'));
 await screen.findByText('如何选水杯？');
 expect(actions).toEqual(['pilot.prepare']);
 view.rerender(<TrialPreparation {...props} data={{...props.data,products:[{...product,revision:2}]}}/>);
 expect(screen.queryByText('如何选水杯？')).toBeNull();
 expect(screen.getByText('保存候选问题（未确认）')).toBeDisabled();
});
it('uses reload-only recovery after saving',async()=>{
 const actions:string[]=[];let failures=1;
 setTestTransport(async <T,>(action:string)=>{actions.push(action);return (action==='pilot.createQuestions'?{created:10,skipped:0,questions:[]}:{token:'t',products:[],candidates:[]}) as T});
 render(<TrialPreparation {...props} refresh={async()=>{if(failures--)throw Error('refresh')}}/>);
 fireEvent.click(screen.getByRole('checkbox',{name:/虚构水杯/}));fireEvent.click(screen.getByText('准备试点资料'));
 await waitFor(()=>expect(screen.getByText('保存候选问题（未确认）')).toBeEnabled());
 fireEvent.click(screen.getByText('保存候选问题（未确认）'));
 await screen.findByText('重新加载资料');fireEvent.click(screen.getByText('重新加载资料'));
 await screen.findByText(/新增 10 条待确认问题.*跳过 0 条已有问题/);
 expect(actions.filter(a=>a==='pilot.createQuestions')).toHaveLength(1);
});
it('ignores a late preparation response after a data revision',async()=>{
 let finish!:(value:unknown)=>void;
 setTestTransport(async <T,>()=>await new Promise<T>(resolve=>{finish=value=>resolve(value as T)}));
 const view=render(<TrialPreparation {...props}/>);
 fireEvent.click(screen.getByRole('checkbox',{name:/虚构水杯/}));fireEvent.click(screen.getByText('准备试点资料'));
 view.rerender(<TrialPreparation {...props} data={{...props.data,products:[{...product,revision:2}]}}/>);
 finish({token:'old',products:[],candidates:[{productId:'p',text:'过时结果',intent:'selection'}]});
 await waitFor(()=>expect(screen.getByText('准备试点资料')).toBeEnabled());
 expect(screen.queryByText('过时结果')).toBeNull();
 expect(screen.getByText('保存候选问题（未确认）')).toBeDisabled();
});

it('reports skipped confirmed candidates without relabelling them unconfirmed',async()=>{
 const refresh=vi.fn(async()=>{});
 setTestTransport(async <T,>(action:string)=>{
  if(action==='pilot.createQuestions')return {created:0,skipped:10,questions:[{confirmed:true}]} as T;
  return {token:'t',products:[],candidates:[]} as T;
 });
 render(<TrialPreparation {...props} refresh={refresh}/>);
 fireEvent.click(screen.getByRole('checkbox',{name:/虚构水杯/}));
 fireEvent.click(screen.getByText('准备试点资料'));
 await waitFor(()=>expect(screen.getByText('保存候选问题（未确认）')).toBeEnabled());
 fireEvent.click(screen.getByText('保存候选问题（未确认）'));
 await screen.findByText(/新增 0 条待确认问题.*跳过 10 条已有问题/);
 expect(screen.queryByText(/候选问题已保存，尚未确认/)).toBeNull();
 expect(refresh).toHaveBeenCalledTimes(1);
});
