import {afterEach,describe,it,expect,vi} from 'vitest';
import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import CollectionPanel from './CollectionPanel';
import {setTestTransport} from '../api';
import type {PageProps} from '../App';
const props=()=>({brand:{id:'b'},data:{campaigns:[{id:'c',name:'虚构采集计划'}]},refresh:vi.fn(async()=>{})}) as unknown as PageProps;
const result={canApply:true,token:'token',readyCount:1,skippedCount:0,rows:Array.from({length:75},(_,i)=>({row:i+1,status:'ready'}))};
function select(){fireEvent.change(screen.getByLabelText('采集固定计划'),{target:{value:'c'}});fireEvent.change(screen.getByLabelText('采集证据 JSON'),{target:{value:'{}'}})}
afterEach(()=>setTestTransport(undefined));
describe('collection panel',()=>{
 it('bounds preview and clears stale content and phase results',async()=>{setTestTransport((async()=>result) as never);render(<CollectionPanel {...props()}/>);select();fireEvent.click(screen.getByText('预览证据导入'));await screen.findByText('第 50 行 · 待保存');expect(screen.queryByText('第 51 行 · 待保存')).toBeNull();fireEvent.change(screen.getByLabelText('采集阶段'),{target:{value:'followup'}});expect(screen.queryByText('确认原子导入')).toBeNull()});
 it('ignores a late local read after editing',async()=>{let resolve!:(v:unknown)=>void;setTestTransport((()=>new Promise(r=>resolve=r)) as never);render(<CollectionPanel {...props()}/>);select();fireEvent.click(screen.getByText('读取采集 JSON'));fireEvent.change(screen.getByLabelText('采集证据 JSON'),{target:{value:'manual'}});resolve({content:'late'});await waitFor(()=>expect(screen.getByLabelText('采集证据 JSON')).toHaveValue('manual'))});
 it('uses atomic confirmed import and reload-only recovery',async()=>{const actions:string[]=[];const p=props();p.refresh=vi.fn(async()=>{throw Error('刷新失败')});setTestTransport((async(a:string)=>{actions.push(a);return a==='collection.preview'?result:{created:1}}) as never);render(<CollectionPanel {...p}/>);select();fireEvent.click(screen.getByText('预览证据导入'));await screen.findByText('确认原子导入');fireEvent.click(screen.getByText('确认原子导入'));await screen.findByText('重新加载资料');expect(screen.queryByText('确认原子导入')).toBeNull();fireEvent.click(screen.getByText('重新加载资料'));await waitFor(()=>expect(p.refresh).toHaveBeenCalledTimes(2));expect(actions.filter(a=>a==='collection.apply')).toHaveLength(1)});
});

describe('collection evidence classification',()=>{
 const warning='模拟证据：仅验证流程，不代表真实表现';
 function classified(mode?:'simulated'|'recorded'){
  const p=props();
  p.data.campaigns![0].evidenceMode=mode;
  return p;
 }
 it.each(['paste','open'])('retains simulated classification after computing and %s through confirmation',async(method)=>{
  setTestTransport((async(action:string)=>action==='collection.prepare'
   ? {summary:{expected:1,completed:0,remaining:1,evidenceMode:'simulated'}}
   : action==='file.openText'?{content:'filled JSON'}:result) as never);
  render(<CollectionPanel {...classified('simulated')}/>);
  fireEvent.change(screen.getByLabelText('采集固定计划'),{target:{value:'c'}});
  expect(screen.getByLabelText('采集证据模式')).toHaveTextContent(warning);
  fireEvent.click(screen.getByText('计算采集清单'));
  await screen.findByText(/共 1 个槽位/);
  if(method==='open'){
   fireEvent.click(screen.getByText('读取采集 JSON'));
   await waitFor(()=>expect(screen.getByLabelText('采集证据 JSON')).toHaveValue('filled JSON'));
  }else fireEvent.change(screen.getByLabelText('采集证据 JSON'),{target:{value:'filled JSON'}});
  expect(screen.queryByText(/共 1 个槽位/)).toBeNull();
  expect(screen.getByLabelText('采集证据模式')).toHaveTextContent(warning);
  fireEvent.click(screen.getByText('预览证据导入'));
  await screen.findByText('确认原子导入');
  expect(screen.getByLabelText('采集证据模式')).toHaveTextContent(warning);
  fireEvent.change(screen.getByLabelText('采集阶段'),{target:{value:'followup'}});
  expect(screen.getByLabelText('采集证据模式')).toHaveTextContent(warning);
 });
 it.each(['simulated',undefined] as const)('classifies direct paste for %s campaigns without a computed packet',async(mode)=>{
  setTestTransport((async()=>result) as never);
  render(<CollectionPanel {...classified(mode)}/>);
  select();
  fireEvent.click(screen.getByText('预览证据导入'));
  await screen.findByText('确认原子导入');
  expect(screen.getByLabelText('采集证据模式')).toHaveTextContent(mode==='simulated'?warning:'历史证据模式未指定');
 });
});
