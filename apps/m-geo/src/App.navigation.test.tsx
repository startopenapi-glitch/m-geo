import {act,cleanup,fireEvent,render,screen,within} from '@testing-library/react';
import {afterEach,describe,expect,it} from 'vitest';
import App from './App';
import {setTestTransport} from './api';
import type {WorkspaceSnapshot} from './types';

const empty:WorkspaceSnapshot={products:[],questions:[],drafts:[],publications:[],observations:[]};
const brand={id:'a',name:'虚构品牌',createdAt:''};
function install(data:WorkspaceSnapshot=empty){
 setTestTransport(async<T,>(action:string)=>{
  if(action==='brand.list')return [brand] as T;
  if(action==='workspace.snapshot')return data as T;
  if(action==='knowledge.list'||action==='asset.list')return [] as T;
  if(action==='ai.jobs')return [] as T;
  if(action==='p0Workflow.list'||action==='offlineWorkflow.list')return {tasks:[]} as T;
  if(action==='p0Workflow.policyGet')return {revision:1,dailyMaxUnits:100,dailyMaxCalls:100,halted:false,day:'2026-10-28',usedUnits:0,usedCalls:0} as T;
  if(action==='ai.settings.get')return {baseUrl:'https://example.invalid/v1',model:'fictional-model',version:1,hasKey:false} as T;
  if(action==='app.info')return {version:'0.17.0',dataLocation:'/fictional',platform:'macos',releaseChannel:'internal',updateUrl:null} as T;
  throw Error(action);
 });
}
afterEach(()=>{cleanup();setTestTransport(undefined)});

it('retains a saved brand profile after navigating away and returning',async()=>{
 let stored={...brand,profile:{aliases:[],officialUrl:'',manufacturer:'old fictional subject',category:'',description:'',updatedAt:''}};
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='brand.list')return [stored] as T;
  if(action==='workspace.snapshot')return empty as T;
  if(action==='research.list')return {items:[],clusters:[],truncated:false} as T;
  if(action==='brand.profile.save'){
   stored={...stored,profile:{...stored.profile,...payload,manufacturer:payload.manufacturer as string,updatedAt:'2026-10-27T01:00:00Z'}};
   return stored as T;
  }
  throw Error(action);
 });
 render(<App/>);
 await screen.findByRole('heading',{name:'让 AI 准确理解你的商品'});
 fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));
 fireEvent.click(screen.getByRole('button',{name:'品牌与问题研究'}));
 fireEvent.change(screen.getByLabelText('制造商 / 运营主体'),{target:{value:'saved fictional subject'}});
 fireEvent.click(screen.getByRole('button',{name:'保存品牌档案'}));
 await screen.findByText('品牌级档案已保存');
 fireEvent.click(screen.getByRole('button',{name:'首页'}));
 fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));
 fireEvent.click(screen.getByRole('button',{name:'品牌与问题研究'}));
 expect(screen.getByLabelText('制造商 / 运营主体')).toHaveValue('saved fictional subject');
});

it('holds navigation and brand selection until a deferred profile save finishes',async()=>{
 let finish!:(value:unknown)=>void;
 const pending=new Promise(resolve=>{finish=resolve});
 const saved={...brand,profile:{aliases:[],officialUrl:'',manufacturer:'deferred fictional subject',category:'',description:'',updatedAt:'2026-10-27T01:00:00Z'}};
 setTestTransport(async<T,>(action:string)=>{
  if(action==='brand.list')return [brand,{id:'other',name:'另一个虚构品牌',createdAt:''}] as T;
  if(action==='workspace.snapshot')return empty as T;
  if(action==='research.list')return {items:[],clusters:[],truncated:false} as T;
  if(action==='brand.profile.save')return await pending as T;
  throw Error(action);
 });
 render(<App/>);
 await screen.findByRole('heading',{name:'让 AI 准确理解你的商品'});
 fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));
 fireEvent.click(screen.getByRole('button',{name:'品牌与问题研究'}));
 fireEvent.change(screen.getByLabelText('制造商 / 运营主体'),{target:{value:'deferred fictional subject'}});
 fireEvent.click(screen.getByRole('button',{name:'保存品牌档案'}));
 expect(screen.getByRole('button',{name:'首页'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'商品事实'})).toBeDisabled();
 expect(screen.getByRole('combobox',{name:'当前品牌'})).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'首页'}));
 expect(screen.getByRole('heading',{name:'品牌档案与问题研究台'})).toBeInTheDocument();
 await act(async()=>{finish(saved)});
 await screen.findByText('品牌级档案已保存');
 expect(screen.getByRole('button',{name:'首页'})).toBeEnabled();
 expect(screen.getByRole('combobox',{name:'当前品牌'})).toBeEnabled();
 fireEvent.click(screen.getByRole('button',{name:'首页'}));
 fireEvent.click(screen.getByRole('button',{name:'事实与证据'}));
 fireEvent.click(screen.getByRole('button',{name:'品牌与问题研究'}));
 expect(screen.getByLabelText('制造商 / 运营主体')).toHaveValue('deferred fictional subject');
});

describe('GEO workflow navigation',()=>{
 it('organizes the left tree around facts, questions, content and testing',async()=>{
  install();render(<App/>);
  await screen.findByRole('heading',{name:'让 AI 准确理解你的商品'});
  const primary=screen.getByRole('navigation',{name:'主要导航'});
  expect(within(primary).getAllByRole('button').map(button=>button.textContent)).toEqual(['首页','事实与证据','问题与内容','AI 测试','数据与设置']);
  expect(screen.getByRole('heading',{name:'从第一件商品开始'})).toBeInTheDocument();
  expect(within(screen.getByRole('list',{name:'GEO 工作流阶段'})).getAllByRole('button')).toHaveLength(6);

  fireEvent.click(within(primary).getByRole('button',{name:'事实与证据'}));
  const productNav=screen.getByRole('group',{name:'事实与证据子导航'});
  expect(within(productNav).getAllByRole('button').map(button=>button.textContent)).toEqual(['商品事实','品牌与问题研究','知识库','内容资产','图片与视频','渠道资料']);
  expect(within(primary).getByRole('button',{name:'事实与证据'})).toHaveAttribute('aria-expanded','true');
  expect(within(productNav).getByRole('button',{name:'商品事实'})).toHaveAttribute('aria-current','page');
  expect(screen.queryByRole('navigation',{name:'任务导航'})).not.toBeInTheDocument();

  fireEvent.click(within(productNav).getByRole('button',{name:'知识库'}));
  expect(await screen.findByRole('heading',{name:'知识库'})).toBeInTheDocument();
  expect(within(productNav).getByRole('button',{name:'知识库'})).toHaveAttribute('aria-current','page');
  expect(await screen.findByText('还没有知识文档')).toBeInTheDocument();

  fireEvent.click(within(productNav).getByRole('button',{name:'内容资产'}));
  expect(await screen.findByRole('heading',{name:'内容资产'})).toBeInTheDocument();
  expect(within(productNav).getByRole('button',{name:'内容资产'})).toHaveAttribute('aria-current','page');
  expect(await screen.findByText('还没有内容资产')).toBeInTheDocument();
  expect(screen.getByText(/仅供人工复用，不是已审核商品事实/)).toBeInTheDocument();

  fireEvent.click(within(primary).getByRole('button',{name:'问题与内容'}));
  const contentNav=screen.getByRole('group',{name:'问题与内容子导航'});
  expect(within(contentNav).getAllByRole('button').map(button=>button.textContent)).toEqual(['问题与诊断','内容审核','发布与撤回']);
  expect(screen.getByRole('heading',{name:'问题与诊断'})).toBeInTheDocument();
  expect(screen.queryByRole('group',{name:'事实与证据子导航'})).not.toBeInTheDocument();
  fireEvent.click(within(contentNav).getByRole('button',{name:'发布与撤回'}));
  expect(within(contentNav).getByRole('button',{name:'发布与撤回'})).toHaveAttribute('aria-current','page');
  expect(within(screen.getByLabelText('当前位置')).getByText('发布与撤回')).toBeInTheDocument();
  expect(within(primary).getAllByRole('button',{current:'page'})).toHaveLength(1);

  fireEvent.click(within(primary).getByRole('button',{name:'AI 测试'}));
  expect(within(screen.getByRole('group',{name:'AI 测试子导航'})).getAllByRole('button').map(button=>button.textContent)).toEqual(['可见度测试','本地自动化','接入演练','巡检与趋势','优化闭环']);

  fireEvent.click(within(primary).getByRole('button',{name:'数据与设置'}));
  expect(within(screen.getByRole('group',{name:'数据与设置子导航'})).getAllByRole('button').map(button=>button.textContent)).toEqual(['备份与设置','批量运营','访问与订单']);
  expect(within(primary).queryByRole('button',{name:'批量运营'})).toBeInTheDocument();
 });

 it('turns the next step into a direct path to incomplete product facts',async()=>{
  install({...empty,products:[{id:'p',brandId:'a',name:'虚构商品',sku:'DEMO-1',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''}]});
  render(<App/>);
  expect(await screen.findByRole('heading',{name:'还有 1 件商品待确认'})).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button',{name:'继续完善商品事实'}));
  expect(await screen.findByRole('button',{name:/虚构商品/})).toBeInTheDocument();
  expect(screen.getByRole('button',{name:'事实与证据'})).toHaveAttribute('aria-expanded','true');
  expect(screen.getByRole('button',{name:'商品事实'})).toHaveAttribute('aria-current','page');
 });

 it('requires a confirmed question set before recommending content generation',async()=>{
  install({...empty,
   products:[{id:'p',brandId:'a',name:'虚构商品',sku:'DEMO-1',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[{id:'f',label:'材质',value:'虚构材质',source:'虚构说明书',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-01-01T00:00:00Z'}],revision:1,confirmedAt:'2026-01-01T00:00:00Z',updatedAt:''}],
   questions:[{id:'q',brandId:'a',productId:'p',text:'虚构商品适合什么场景？',intent:'selection',origin:'suggestion',source:'',branded:false,confirmed:false}],
  });
  render(<App/>);
  expect(await screen.findByRole('heading',{name:'建立第一版固定问题集'})).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button',{name:'整理并确认问题'}));
  expect(await screen.findByRole('heading',{name:'问题与诊断'})).toBeInTheDocument();
  expect(screen.getByText('虚构商品适合什么场景？')).toBeInTheDocument();
  expect(screen.getByRole('button',{name:'问题与诊断'})).toHaveAttribute('aria-current','page');
 });
});

it('opens fixture integration rehearsal while preserving the offline automation page',async()=>{
 install();render(<App/>);await screen.findByRole('heading',{name:'让 AI 准确理解你的商品'});fireEvent.click(screen.getByRole('button',{name:'AI 测试'}));fireEvent.click(screen.getByRole('button',{name:'接入演练'}));expect(await screen.findByRole('heading',{name:'接入演练'})).toBeInTheDocument();expect(screen.getByRole('button',{name:'接入演练'})).toHaveAttribute('aria-current','page');fireEvent.click(screen.getByRole('button',{name:'本地自动化'}));expect(await screen.findByRole('heading',{name:'本地自动化'})).toBeInTheDocument();
});
