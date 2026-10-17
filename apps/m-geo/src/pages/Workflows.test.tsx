import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Brand,Draft,Product,WorkspaceSnapshot} from '../types';
import Products from './Products';
import Content,{DraftEditor} from './Content';
import Settings from './Settings';
import Visibility from './Visibility';
const brand:Brand={id:'a',name:'晴山',createdAt:''};
const product:Product={id:'p',brandId:'a',name:'晴山水杯',sku:'CUP-01',category:'水杯',officialUrl:'https://example.com',sourceText:'RAW PRIVATE',privateNotes:'INTERNAL ONLY',facts:[{id:'f',label:'容量',value:'500ml',source:'官方规格',visibility:'public',validUntil:null}],revision:2,confirmedAt:'2026-10-17T00:00:00Z',updatedAt:''};
const draft:Draft={id:'d',brandId:'a',productId:'p',questionId:null,kind:'description',title:'晴山水杯介绍',body:'容量 500ml。依据：官方规格',sourceRevision:2,revision:3,generationMode:'local',reviewedAt:'2026-10-17T00:00:00Z',createdAt:'2026-10-17T00:00:00Z'};
const base:WorkspaceSnapshot={products:[product],questions:[],drafts:[],publications:[],observations:[]};
const props={brand,data:base,refresh:vi.fn(async()=>{})};
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});
describe('native workflow UI',()=>{
 it('offers the CSV template before selecting any import file',async()=>{
  const actions:string[]=[];
  setTestTransport(async <T,>(action:string)=>{actions.push(action);if(action==='file.saveTemplate')return {saved:true} as T;throw Error(action)});
  render(<Products {...props}/>);
  fireEvent.click(screen.getByRole('button',{name:'下载模板'}));
  await waitFor(()=>expect(actions).toEqual(['file.saveTemplate']));
 });
 it('requires saving changed facts before confirming the saved version',async()=>{
  setTestTransport(async <T,>()=>({}) as T);
  render(<Products {...props}/>);
  fireEvent.click(screen.getByRole('button',{name:/晴山水杯/}));
  expect(screen.getByRole('button',{name:'确认已保存的事实'})).toBeEnabled();
  fireEvent.change(screen.getByLabelText('事实 1 内容'),{target:{value:'600ml'}});
  expect(screen.getByRole('button',{name:'确认已保存的事实'})).toBeDisabled();
 });
 it('clears a newly submitted key immediately and never tests a connection without consent',async()=>{
  const actions:string[]=[];let resolveSave!:(v:any)=>void;
  setTestTransport(async <T,>(action:string)=>{actions.push(action);if(action==='ai.settings.get')return {baseUrl:'https://api.deepseek.com/v1',model:'deepseek-chat',version:1,hasKey:false} as T;if(action==='app.info')return {name:'M- GEO',version:'0.1',dataLocation:'/local',platform:'macos',releaseChannel:'internal',updateUrl:null} as T;if(action==='ai.settings.save')return await new Promise<T>(resolve=>{resolveSave=resolve});throw Error(action)});
  render(<Settings {...props} restore={async()=>{}} restoring={false}/>);
  await screen.findByDisplayValue('deepseek-chat');
  fireEvent.change(screen.getByLabelText('API Key'),{target:{value:'secret-for-test'}});
  fireEvent.click(screen.getByRole('button',{name:'保存模型配置'}));
  expect(screen.getByLabelText('API Key')).toHaveValue('');
  expect(actions).not.toContain('ai.test');
  resolveSave({baseUrl:'https://api.deepseek.com/v1',model:'deepseek-chat',version:2,hasKey:true});
  await screen.findByText('模型配置已保存，未发送网络请求');
  expect(screen.getByRole('button',{name:'测试已保存的连接'})).toBeDisabled();
 });
 it('requires AI generation consent and sends identifiers rather than private text',async()=>{
  const payloads:any[]=[];
  setTestTransport(async <T,>(action:string,p:any)=>{if(action==='ai.jobs')return [] as T;if(action==='ai.start'){payloads.push(p);return {id:'job'} as T}throw Error(action)});
  render(<Content {...props}/>);
  fireEvent.change(screen.getByLabelText('用于生成的商品'),{target:{value:'p'}});
  fireEvent.change(screen.getByLabelText('生成方式'),{target:{value:'ai'}});
  expect(screen.getByRole('button',{name:'生成 AI草稿'})).toBeDisabled();
  fireEvent.click(screen.getByLabelText(/同意发送当前商品/));
  fireEvent.click(screen.getByRole('button',{name:'生成 AI草稿'}));
  await screen.findByText('AI 任务已创建，请查看下方进度');
  expect(payloads).toEqual([{brandId:'a',productId:'p',kind:'description',questionId:null,consent:true}]);
 });
 it('disables review and export immediately after editing a reviewed draft',async()=>{
  render(<DraftEditor {...props} draft={draft}/>);
  expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeEnabled();
  fireEvent.change(screen.getByLabelText('草稿正文'),{target:{value:'编辑后的内容'}});
  expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();
  expect(screen.getByRole('button',{name:'确认内容并审核'})).toBeDisabled();
 });
 it('keeps different models in separate baseline/follow-up cohorts',()=>{
  const observation={id:'o1',brandId:'a',productId:'p',questionId:'q',querySnapshot:'水杯怎么选？',branded:false,platform:'doubao',surface:'consumer',phase:'baseline',modelLabel:'版本甲',searchMode:'联网',testedAt:'2026-10-17T00:00:00Z',answer:'原始回答',mentioned:true,accuracy:'unknown',citedUrl:'',evidenceNote:''} as const;
  render(<Visibility {...props} data={{...base,observations:[observation,{...observation,id:'o2',modelLabel:'版本乙',phase:'followup'}]}}/>);
  expect(screen.getAllByText('缺少同条件的基线或复测，暂不能比较。')).toHaveLength(2);
  expect(screen.queryByText('两阶段已有同条件样本；结果仅代表这些记录。')).not.toBeInTheDocument();
 });
 it('sends a cleared fact expiry as null and keeps the optimistic revision',async()=>{
  let payload:any;
  setTestTransport(async <T,>(action:string,p:any)=>{if(action==='product.save'){payload=p;return product as T}throw Error(action)});
  render(<Products {...props} data={{...base,products:[{...product,facts:[{...product.facts[0],validUntil:'2027-01-01'}]}]}}/>);
  fireEvent.click(screen.getByRole('button',{name:/晴山水杯/}));
  fireEvent.change(screen.getByLabelText('事实 1 有效期'),{target:{value:''}});
  fireEvent.click(screen.getByRole('button',{name:'保存商品'}));
  await waitFor(()=>expect(payload).toBeDefined());
  expect(payload.facts[0].validUntil).toBeNull();
  expect(payload.expectedRevision).toBe(2);
 });
 it('shows a recoverable save error and keeps entered facts',async()=>{
  let saves=0;
  setTestTransport(async <T,>(action:string)=>{if(action==='product.save'){saves++;if(saves===1)throw Error('SKU 已存在，请使用其他 SKU');return {...product,id:'new'} as T}throw Error(action)});
  render(<Products {...props} data={{...base,products:[]}}/>);
  fireEvent.click(screen.getByRole('button',{name:'新增商品'}));
  fireEvent.change(screen.getByLabelText('商品名称'),{target:{value:'新水杯'}});
  fireEvent.change(screen.getByLabelText('SKU'),{target:{value:'CUP-01'}});
  fireEvent.click(screen.getByRole('button',{name:'保存商品'}));
  expect(await screen.findByRole('alert')).toHaveTextContent('SKU 已存在');
  expect(screen.getByLabelText('商品名称')).toHaveValue('新水杯');
  fireEvent.change(screen.getByLabelText('SKU'),{target:{value:'CUP-02'}});
  fireEvent.click(screen.getByRole('button',{name:'保存商品'}));
  await waitFor(()=>expect(saves).toBe(2));
 });
});
