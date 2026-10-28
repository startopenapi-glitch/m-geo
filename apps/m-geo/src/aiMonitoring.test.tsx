import {afterEach,expect,it,vi} from 'vitest';
import {fireEvent,render,screen,waitFor} from '@testing-library/react';
import {AiMonitorPanel,observationOriginLabel} from './aiMonitoring';
import {setTestTransport} from './api';
import type {Observation,WorkspaceSnapshot} from './types';

afterEach(()=>setTestTransport(undefined));
function fixture():WorkspaceSnapshot{return {
  products:[{id:'p',brandId:'b',name:'虚构商品',sku:'FICTION-001',category:'测试',officialUrl:'',sourceText:'私密原文',privateNotes:'私密备注',confirmedAt:'2026-10-24T00:00:00Z',revision:1,updatedAt:'2026-10-24T00:00:00Z',facts:[{id:'f',label:'容量',value:'500mL',source:'虚构公开规格',visibility:'public',validUntil:null}]}],
  questions:[{id:'q',brandId:'b',productId:'p',text:'家用容量如何选择？',intent:'selection',origin:'customer',source:'虚构问题',branded:false,confirmed:true}],
  campaigns:[{id:'c',brandId:'b',name:'虚构真实监测计划',evidenceMode:'recorded',questions:[{id:'q',productId:'p',text:'家用容量如何选择？'}],platforms:['doubao'],repetitions:2,modelLabel:'deepseek-chat',searchMode:'API；未联网',context:'固定条件',createdAt:'2026-10-24T00:00:00Z'}],
  observations:[],drafts:[],publications:[]
}}
function observation():Observation{return {id:'o',brandId:'b',productId:'p',questionId:'q',querySnapshot:'固定问题',branded:false,platform:'doubao',surface:'consumer',phase:'baseline',modelLabel:'m',searchMode:'off',testedAt:'2026-10-24T00:00:00Z',answer:'回答',mentioned:false,accuracy:'unknown',citedUrl:'',evidenceNote:''}}

it('labels real API, manual imports and simulations without changing their surface',()=>{
  const manual=observation();
  expect(observationOriginLabel(manual,fixture().campaigns)).toBe('手动导入');
  const real:Observation={...manual,surface:'api',sourceKind:'real_api',transport:'real_api'};
  expect(observationOriginLabel(real,fixture().campaigns)).toBe('真实 API');
  const simulated={...manual,assessment:{campaignId:'sim',repetition:1,context:'模拟',recommendation:'unknown' as const,rank:null,competitors:''}};
  expect(observationOriginLabel(simulated,[{...fixture().campaigns![0],id:'sim',evidenceMode:'simulated'}])).toBe('模拟');
  expect(manual.surface).toBe('consumer');
});

it('prepares without sending and requires explicit public-data billing consent',async()=>{
  const actions:{action:string;payload:Record<string,unknown>}[]=[];
  const prepared={preparationHash:'prep-hash',monitorVersion:1,brandId:'b',campaignId:'c',productId:'p',questionId:'q',platform:'doubao',phase:'baseline',repetition:1,surface:'api',evidenceMode:'recorded',sourceKind:'real_api',transport:'real_api',modelLabel:'deepseek-chat',searchMode:'API；未联网',configVersion:7,publicSnapshot:{brandName:'测试品牌',productName:'虚构商品',sku:'FICTION-001',facts:[{label:'容量',value:'500mL',source:'虚构公开规格'}]}};
  const running={id:'job',brandId:'b',kind:'monitor',status:'queued',message:'等待真实 API 处理',resultText:null};
  const succeeded={...running,status:'succeeded',message:'真实 API 回答已记录',resultText:'虚构商品容量为 500mL。',responseModel:'deepseek-chat',providerRequestId:'request-1',citations:['https://example.com/evidence'],observationId:'o'};
  setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
    actions.push({action,payload});
    if(action==='ai.monitor.prepare')return prepared as T;
    if(action==='ai.monitor.start')return running as T;
    if(action==='ai.jobs')return [succeeded] as T;
    throw new Error(`unexpected ${action}`);
  });
  const refresh=vi.fn().mockResolvedValue(undefined);
  render(<AiMonitorPanel brandId="b" data={fixture()} refresh={refresh}/>);
  fireEvent.change(screen.getByLabelText('真实监测固定计划'),{target:{value:'c'}});
  fireEvent.change(screen.getByLabelText('真实监测商品'),{target:{value:'p'}});
  fireEvent.change(screen.getByLabelText('真实监测固定问题'),{target:{value:'q'}});
  fireEvent.click(screen.getByRole('button',{name:'准备公开资料（不发送）'}));
  expect(await screen.findByText('prep-hash')).toBeTruthy();
  expect(screen.getByRole('heading',{name:'真实 API 知情问答测试'})).toBeTruthy();
  expect(screen.getByText(/已提供商品公开事实的知情测试/)).toBeTruthy();
  expect(screen.getByText(/不能作为自然发现或消费者平台推荐证据/)).toBeTruthy();
  expect(actions.map(item=>item.action)).toEqual(['ai.monitor.prepare']);
  const start=screen.getByRole('button',{name:'发送一次真实 API 监测'}) as HTMLButtonElement;
  expect(start.disabled).toBe(true);
  fireEvent.click(screen.getByLabelText('同意向已保存的模型服务发送上述公开资料，并了解本次真实 API 调用可能产生费用'));
  expect(start.disabled).toBe(false);
  fireEvent.click(start);
  expect(await screen.findByText('虚构商品容量为 500mL。')).toBeTruthy();
  await waitFor(()=>expect(refresh).toHaveBeenCalledOnce());
  const startCall=actions.find(item=>item.action==='ai.monitor.start');
  expect(startCall?.payload).toEqual({brandId:'b',preparationHash:'prep-hash',consent:true});
  expect(JSON.stringify(actions)).not.toContain('apiKey');
  expect(JSON.stringify(actions)).not.toContain('私密原文');
  expect(screen.getByText('https://example.com/evidence')).toBeTruthy();
});

it('does not offer simulated campaigns for real API monitoring',()=>{
  const data=fixture();data.campaigns=[{...data.campaigns![0],evidenceMode:'simulated'}];
  render(<AiMonitorPanel brandId="b" data={data} refresh={vi.fn()}/>);
  expect(screen.getByText('请先建立“人工实测记录”固定测试计划。模拟计划不能用于真实 API 监测。')).toBeTruthy();
  expect(screen.queryByRole('button',{name:'准备公开资料（不发送）'})).toBeNull();
});
