import {describe,it,expect,vi} from 'vitest';
import {render,screen,fireEvent,waitFor,within} from '@testing-library/react';
import ExperimentReport from './ExperimentReport';
import {setTestTransport} from '../api';
import type {PageProps} from '../App';
const campaign={id:'fixture-campaign',name:'虚构测试计划'};
function props(campaigns:unknown[]=[]){return {brand:{id:'fixture-brand'},data:{campaigns},refresh:vi.fn()} as unknown as PageProps}
const report={summary:{expectedPerPhase:1,baselineCount:1,followupCount:0,missingCount:1,missing:[],platforms:[]}};
describe('local experiment report',()=>{
 it('shows empty state',()=>{render(<ExperimentReport {...props()}/>);expect(screen.getByText('先创建固定测试计划。')).toBeTruthy()});
 it('invalidates snapshot and ignores late results',async()=>{let resolve!:(v:unknown)=>void;setTestTransport((()=>new Promise(r=>resolve=r)) as never);const p=props([campaign]);const view=render(<ExperimentReport {...p}/>);fireEvent.change(screen.getByLabelText('报告固定计划'),{target:{value:campaign.id}});fireEvent.click(screen.getByText('计算本地报告'));view.rerender(<ExperimentReport {...props([campaign])}/>);resolve(report);await waitFor(()=>expect(screen.queryByText('基线 1/1')).toBeNull());setTestTransport(undefined)});
 it('reports canceled export',async()=>{setTestTransport((async(action:string)=>action==='campaign.report'?report:null) as never);render(<ExperimentReport {...props([campaign])}/>);fireEvent.change(screen.getByLabelText('报告固定计划'),{target:{value:campaign.id}});fireEvent.click(screen.getByText('计算本地报告'));await screen.findByText('基线 1/1');fireEvent.click(screen.getByText('导出实验 ZIP'));await screen.findByText('已取消保存实验报告');setTestTransport(undefined)});
 it('pages missing cells only while keeping full coverage, resets for campaign and smaller reports',async()=>{
  let count=45;
  setTestTransport((async()=>({summary:{expectedPerPhase:100,baselineCount:80,followupCount:75,missingCount:count,missing:Array.from({length:count},(_,i)=>({questionId:`q${i}`,questionText:`待补问题${i}`,platform:'doubao',repetition:1,phase:'baseline'})),platforms:[]}})) as never);
  render(<ExperimentReport {...props([campaign,{id:'fixture-second',name:'虚构第二计划'}])}/>);
  fireEvent.change(screen.getByLabelText('报告固定计划'),{target:{value:campaign.id}});fireEvent.click(screen.getByText('计算本地报告'));
  await screen.findByText('基线 80/100');
  fireEvent.click(screen.getByText('待补录单元 45'));
  const nav=screen.getByRole('navigation',{name:'待补录分页'});
  expect(screen.queryByText(/待补问题20 ·/)).toBeNull();
  fireEvent.click(within(nav).getByText('下一页'));fireEvent.click(within(nav).getByText('下一页'));
  expect(within(nav).getByText('第 3/3 页 · 共 45 条')).toBeTruthy();expect(screen.getByText(/待补问题44 ·/)).toBeTruthy();expect(screen.getByText('基线 80/100')).toBeTruthy();
  fireEvent.click(within(nav).getByText('上一页'));expect(within(nav).getByText('第 2/3 页 · 共 45 条')).toBeTruthy();
  count=2;fireEvent.click(screen.getByText('计算本地报告'));await screen.findByText('待补录单元 2');expect(within(nav).getByText('第 1/1 页 · 共 2 条')).toBeTruthy();
  fireEvent.change(screen.getByLabelText('报告固定计划'),{target:{value:'fixture-second'}});expect(screen.queryByText('基线 80/100')).toBeNull();
  setTestTransport(undefined);
 });

});
