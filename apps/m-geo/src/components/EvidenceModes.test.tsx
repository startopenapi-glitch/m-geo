import {render,screen,fireEvent} from '@testing-library/react';
import {describe,it,expect,vi} from 'vitest';
import CommerceDashboard from './CommerceDashboard';
import ExperimentPanel from './ExperimentPanel';
import type {PageProps} from '../App';
import {setTestTransport} from '../api';
const product={id:'p',name:'虚构商品',facts:[],confirmedAt:null};
const question={id:'q',productId:'p',text:'如何选购杯子？',confirmed:true,branded:false};
const campaign=(id:string,evidenceMode?:string)=>({id,name:id,evidenceMode,questions:[question],platforms:['doubao'],repetitions:1,context:'新对话'});
const observation=(id:string)=>({id,productId:'p',questionId:'q',surface:'consumer',mentioned:true,citedUrl:'',accuracy:'unknown',assessment:{campaignId:id,recommendation:'yes'}});
const props={brand:{id:'b'},data:{products:[product],questions:[question],drafts:[],publications:[],campaigns:[campaign('sim','simulated'),campaign('real','recorded'),campaign('old')],observations:[observation('sim'),observation('real'),observation('old')]},refresh:vi.fn()} as unknown as PageProps;
describe('evidence provenance boundaries',()=>{
 it('counts only explicitly recorded consumer samples as recommendation evidence',()=>{
  render(<CommerceDashboard data={props.data}/>);
  expect(screen.getByText(/历史推荐证据 1 条/)).toBeTruthy();
  expect(screen.getByText(/模拟样本 1 条.*未分类 1 条/)).toBeTruthy();
 });
 it('keeps old evidence unclassified and defaults new experiments to simulation',async()=>{
  let sent:Record<string,unknown>|undefined;
  setTestTransport((async(action:string,payload:Record<string,unknown>)=>{if(action==='campaign.create')sent=payload;return {}}) as never);
  render(<ExperimentPanel {...props}/>);
  expect(screen.getByText(/历史性质未分类/)).toBeTruthy();
  fireEvent.click(screen.getByText('新建基线与复测计划'));
  expect((screen.getByLabelText('样本性质') as HTMLSelectElement).value).toBe('simulated');
  fireEvent.change(screen.getByLabelText('测试计划名称'),{target:{value:'虚构实验'}});
  fireEvent.click(screen.getByRole('checkbox'));
  fireEvent.click(screen.getByText('保存固定测试计划'));
  await vi.waitFor(()=>expect(sent?.evidenceMode).toBe('simulated'));
  setTestTransport(undefined);
 });
});
