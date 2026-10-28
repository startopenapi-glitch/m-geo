import {act, cleanup, fireEvent, render, screen, waitFor} from '@testing-library/react';
import {afterEach, describe, expect, it} from 'vitest';
import userEvent from '@testing-library/user-event';
import App from './App';
import {setTestTransport} from './api';
import type {WorkspaceSnapshot} from './types';
const empty:WorkspaceSnapshot={products:[],questions:[],drafts:[],publications:[],observations:[]};
afterEach(()=>{cleanup();setTestTransport(undefined)});
describe('M- GEO workspace',()=>{
 it('creates the first brand and opens its honest empty workspace',async()=>{
  let brands:any[]=[];
  setTestTransport(async <T,>(action:string,p:any)=>{
   if(action==='brand.list')return brands as T;
   if(action==='brand.create'){brands=[{id:'new',name:p.name,createdAt:''}];return brands[0] as T;}
   if(action==='workspace.snapshot')return empty as T;
   if(action==='ai.jobs')return [] as T;
   throw Error(action);
  });
  render(<App/>);
  fireEvent.change(await screen.findByLabelText('品牌名称'),{target:{value:'晴山'}});
  fireEvent.click(screen.getByRole('button',{name:'创建品牌'}));
  expect(await screen.findByRole('heading',{name:'让 AI 准确理解你的商品'})).toBeInTheDocument();
  expect(screen.getByText('还没有商品资料')).toBeInTheDocument();
 });
 it('ignores a delayed response from a brand after switching',async()=>{
  let resolveA!:(v:WorkspaceSnapshot)=>void;
  const delayed=new Promise<WorkspaceSnapshot>(r=>{resolveA=r});
  setTestTransport(async <T,>(action:string,p:any)=>{
   if(action==='brand.list')return [{id:'a',name:'甲品牌'},{id:'b',name:'乙品牌'}] as T;
   if(action==='workspace.snapshot')return (p.brandId==='a'?await delayed:empty) as T;
   if(action==='ai.jobs')return [] as T;
   throw Error(action);
  });
  render(<App/>);
  fireEvent.change(await screen.findByLabelText('当前品牌'),{target:{value:'b'}});
  await screen.findByText('还没有商品资料');
  resolveA({...empty,products:[{id:'stale',brandId:'a',name:'甲秘密商品'} as any]});
  await waitFor(()=>expect(screen.queryByText('甲秘密商品')).not.toBeInTheDocument());
  expect(screen.getByLabelText('当前品牌')).toHaveValue('b');
 });
 it('clears selected product and unsaved input when switching an existing workspace',async()=>{
  const item={id:'p',brandId:'a',name:'甲商品',sku:'A1',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
  setTestTransport(async <T,>(action:string,p:any)=>{
   if(action==='brand.list')return [{id:'a',name:'甲品牌'},{id:'b',name:'乙品牌'}] as T;
   if(action==='workspace.snapshot')return (p.brandId==='a'?{...empty,products:[item]}:empty) as T;
   throw Error(action);
  });
  render(<App/>);
  await screen.findByText('让 AI 准确理解你的商品');
  fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));fireEvent.click(screen.getByRole('button',{name:'商品事实'}));
  fireEvent.click(await screen.findByRole('button',{name:/甲商品/}));
  fireEvent.change(screen.getByLabelText('商品名称'),{target:{value:'未保存的甲资料'}});
  fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:'b'}});
  await screen.findByText('还没有商品资料');
  expect(screen.queryByDisplayValue('未保存的甲资料')).not.toBeInTheDocument();
  expect(screen.queryByDisplayValue('A1')).not.toBeInTheDocument();
 });
 it('retries a completed AI job snapshot after a recoverable workspace read failure',async()=>{
  let status='running',snapshots=0;
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list')return [{id:'a',name:'甲品牌',createdAt:''}] as T;
   if(action==='workspace.snapshot'){snapshots++;if(snapshots===2)throw Error('工作区快照暂不可用');return empty as T;}
   if(action==='ai.jobs')return [{id:'j',brandId:'a',productId:'p',kind:'faq',status,message:'',resultText:null,draftId:status==='succeeded'?'d':null,createdAt:''}] as T;
   throw Error(action);
  });
  render(<App/>);await screen.findByText('让 AI 准确理解你的商品');
  fireEvent.click(screen.getByRole('button',{name:'问题与内容'}));fireEvent.click(screen.getByRole('button',{name:'内容审核'}));await screen.findByText('生成中');status='succeeded';
  fireEvent.click(screen.getByRole('button',{name:'刷新'}));
  await waitFor(()=>expect(screen.getAllByText('工作区快照暂不可用').length).toBeGreaterThan(0));
  fireEvent.click(screen.getByRole('button',{name:'刷新'}));
  await waitFor(()=>expect(snapshots).toBe(3));
  await screen.findByText('已完成');
 });
 it('handles initial snapshot and retry failures without unhandled rejections',async()=>{
  let reads=0;
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list')return [{id:'a',name:'甲品牌',createdAt:''}] as T;
   if(action==='workspace.snapshot'){reads++;if(reads<3)throw Error(`资料读取失败 ${reads}`);return empty as T;}
   throw Error(action);
  });
  render(<App/>);await screen.findByText('资料读取失败 1');
  fireEvent.click(screen.getByRole('button',{name:'重试'}));await screen.findByText('资料读取失败 2');
  fireEvent.click(screen.getByRole('button',{name:'重试'}));await screen.findByText('让 AI 准确理解你的商品');
  expect(reads).toBe(3);
 });
 it('shows a native failure and allows retry without demo data',async()=>{
  let tries=0;
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list'){if(tries++===0)throw Error('本地工作区读取失败');return [] as T;}
   throw Error(action);
  });
  render(<App/>);
  expect(await screen.findByRole('alert')).toHaveTextContent('本地工作区读取失败');
  fireEvent.click(screen.getByRole('button',{name:'重试'}));
  expect(await screen.findByLabelText('品牌名称')).toBeInTheDocument();
 });
 it('reloads all restored data even when navigation and a brand switch are attempted during restore',async()=>{
  let restored=false,openedRecovery=false,finishRestore!:(value:{restored:true;recoveryPath:string})=>void;
  const pending=new Promise<{restored:true;recoveryPath:string}>(resolve=>{finishRestore=resolve});
  const product={id:'p',brandId:'a',name:'恢复后的商品',sku:'RESTORED',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
  setTestTransport(async <T,>(action:string,p:any)=>{
   if(action==='brand.list')return (restored?[{id:'a',name:'恢复后的品牌',createdAt:''}]:[{id:'a',name:'甲品牌',createdAt:''},{id:'b',name:'乙品牌',createdAt:''}]) as T;
   if(action==='workspace.snapshot')return (restored?{...empty,products:[product]}:empty) as T;
   if(action==='ai.settings.get')return {baseUrl:'https://api.deepseek.com/v1',model:'deepseek-chat',version:1,hasKey:false} as T;
   if(action==='app.info')return {version:'0.1',dataLocation:'/local',platform:'macos',releaseChannel:'internal',updateUrl:null} as T;
   if(action==='backup.restore'){expect(p).toEqual({password:'restore-secret',confirm:true});return await pending as T;}
   if(action==='app.openRecoveryFolder'){expect(p).toEqual({});openedRecovery=true;return {ok:true} as T;}
   throw Error(action);
  });
  render(<App/>);await screen.findByText('让 AI 准确理解你的商品');
  fireEvent.click(screen.getByRole('button',{name:'数据与设置'}));
  fireEvent.change(await screen.findByLabelText('备份 / 恢复口令'),{target:{value:'restore-secret'}});
  fireEvent.click(screen.getByLabelText('我确认用所选备份替换当前整个工作区'));
  fireEvent.click(screen.getByRole('button',{name:'选择备份并恢复'}));
  fireEvent.click(screen.getByRole('button',{name:'首页'}));
  fireEvent.change(screen.getByLabelText('当前品牌'),{target:{value:'b'}});
  await act(async()=>{restored=true;finishRestore({restored:true,recoveryPath:'/local/recovery/pre-restore.sqlite'});});
  await screen.findByRole('option',{name:'恢复后的品牌'});
  expect(screen.queryByRole('option',{name:'乙品牌'})).not.toBeInTheDocument();
  expect((await screen.findAllByText('恢复后的商品')).length).toBeGreaterThan(0);
  expect(screen.getByText(/pre-restore\.sqlite/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));fireEvent.click(screen.getByRole('button',{name:'商品事实'}));
  expect(screen.getByText(/pre-restore\.sqlite/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button',{name:'打开快照目录'}));
  await screen.findByText('已请求打开恢复前快照目录');
  expect(openedRecovery).toBe(true);
 });
 it('restores an empty installation without first creating a brand',async()=>{
  let restored=false;
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list')return (restored?[{id:'r',name:'备份品牌',createdAt:''}]:[]) as T;
   if(action==='backup.restore'){restored=true;return {restored:true,recoveryPath:'/local/recovery/empty.sqlite'} as T;}
   if(action==='workspace.snapshot')return empty as T;
   throw Error(action);
  });
  render(<App/>);
  const keyboard=userEvent.setup();
  await keyboard.click(await screen.findByLabelText('备份 / 恢复口令'));
  await keyboard.type(screen.getByLabelText('备份 / 恢复口令'),'restore-secret');
  await keyboard.tab();
  expect(screen.getByLabelText('我确认用所选备份替换当前整个工作区')).toHaveFocus();
  await keyboard.keyboard(' ');
  await keyboard.tab();
  expect(screen.getByRole('button',{name:'选择备份并恢复'})).toHaveFocus();
  await keyboard.keyboard('{Enter}');
  expect(await screen.findByRole('option',{name:'备份品牌'})).toBeInTheDocument();
  expect(await screen.findByText('让 AI 准确理解你的商品')).toBeInTheDocument();
  expect(screen.getByLabelText('工作区恢复状态')).toHaveFocus();
 });
 it.each(['cancel','failure'] as const)('locks the whole workspace from picker launch and unlocks after restore %s',async(outcome)=>{
  let settle!:(value:null)=>void,reject!:(reason:Error)=>void,brandReads=0;
  const pending=new Promise<null>((resolve,fail)=>{settle=resolve;reject=fail});
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list'){brandReads++;return [{id:'a',name:'甲品牌',createdAt:''},{id:'b',name:'乙品牌',createdAt:''}] as T;}
   if(action==='workspace.snapshot')return empty as T;
   if(action==='ai.settings.get')return {baseUrl:'https://api.deepseek.com/v1',model:'deepseek-chat',version:1,hasKey:false} as T;
   if(action==='app.info')return {version:'0.1',dataLocation:'/local',platform:'macos',releaseChannel:'internal',updateUrl:null} as T;
   if(action==='backup.restore')return await pending as T;
   throw Error(action);
  });
  render(<App/>);await screen.findByText('让 AI 准确理解你的商品');
  fireEvent.click(screen.getByRole('button',{name:'数据与设置'}));
  await screen.findByDisplayValue('deepseek-chat');
  fireEvent.change(screen.getByLabelText('备份 / 恢复口令'),{target:{value:'restore-secret'}});
  fireEvent.click(screen.getByLabelText('我确认用所选备份替换当前整个工作区'));
  fireEvent.click(screen.getByRole('button',{name:'选择备份并恢复'}));
  expect(screen.getByLabelText('备份 / 恢复口令')).toHaveValue('');
  expect(screen.getByLabelText('当前品牌')).toBeDisabled();
  expect(screen.getByRole('button',{name:'首页'})).toBeDisabled();
  expect(screen.getByRole('button',{name:'添加品牌'})).toBeDisabled();
  expect(screen.getByRole('button',{name:'保存模型配置'})).toBeDisabled();
  expect(screen.getByRole('button',{name:'选择 CNVP JSON 导出文件'})).toBeDisabled();
  await act(async()=>{if(outcome==='cancel')settle(null);else reject(new Error('口令不正确或备份已损坏'));});
  expect(await screen.findByText(outcome==='cancel'?'已取消恢复，工作区未替换':'口令不正确或备份已损坏')).toBeInTheDocument();
  expect(screen.getByRole('button',{name:'首页'})).toBeEnabled();
  expect(screen.getByRole('button',{name:'保存模型配置'})).toBeEnabled();
  expect(screen.getByLabelText('当前品牌')).toHaveValue('a');
  expect(brandReads).toBe(1);
 });
 it('prevents restoring while the first brand is still being created',async()=>{
  let finishCreate!:(value:any)=>void;
  setTestTransport(async <T,>(action:string)=>{
   if(action==='brand.list')return [] as T;
   if(action==='brand.create')return await new Promise<T>(resolve=>{finishCreate=resolve});
   if(action==='workspace.snapshot')return empty as T;
   throw Error(action);
  });
  render(<App/>);
  fireEvent.change(await screen.findByLabelText('品牌名称'),{target:{value:'新品牌'}});
  fireEvent.change(screen.getByLabelText('备份 / 恢复口令'),{target:{value:'restore-secret'}});
  fireEvent.click(screen.getByLabelText('我确认用所选备份替换当前整个工作区'));
  expect(screen.getByRole('button',{name:'选择备份并恢复'})).toBeEnabled();
  fireEvent.click(screen.getByRole('button',{name:'创建品牌'}));
  expect(screen.getByRole('button',{name:'选择备份并恢复'})).toBeDisabled();
  await act(async()=>{finishCreate({id:'created',name:'新品牌',createdAt:''});});
  expect(await screen.findByText('让 AI 准确理解你的商品')).toBeInTheDocument();
 });
 it.each(['brand.list','workspace.snapshot'])('can restore a backup when the initial %s read fails',async(failedAction)=>{
  let restored=false;
  setTestTransport(async <T,>(action:string)=>{
   if(action===failedAction&&!restored)throw Error('本地资料格式损坏，请从备份恢复');
   if(action==='brand.list')return [{id:'a',name:restored?'已恢复品牌':'损坏的工作区',createdAt:''}] as T;
   if(action==='workspace.snapshot')return empty as T;
   if(action==='backup.restore'){restored=true;return {restored:true,recoveryPath:'/local/recovery/damaged.sqlite'} as T;}
   throw Error(action);
  });
  render(<App/>);
  await screen.findByText('本地资料格式损坏，请从备份恢复');
  fireEvent.change(screen.getByLabelText('备份 / 恢复口令'),{target:{value:'restore-secret'}});
  fireEvent.click(screen.getByLabelText('我确认用所选备份替换当前整个工作区'));
  fireEvent.click(screen.getByRole('button',{name:'选择备份并恢复'}));
  expect(await screen.findByRole('option',{name:'已恢复品牌'})).toBeInTheDocument();
  expect(await screen.findByText('让 AI 准确理解你的商品')).toBeInTheDocument();
  expect(screen.queryByText('本地资料格式损坏，请从备份恢复')).not.toBeInTheDocument();
 });
});
