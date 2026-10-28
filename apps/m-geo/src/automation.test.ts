import {createElement} from 'react';
import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,describe,expect,it} from 'vitest';
import {setTestTransport} from './api';
import {AUTOMATION_NODE_KINDS,MIN_TREND_SAMPLES,buildAutomationNodes,compareTrendVersions,deriveTrendPoints,evidenceModeLabel,formatRate,makeAutomationCreatePayload,summarizeTrend,taskIsDue,type AutomationReport,type AutomationTask,type TrendPoint} from './automation';
import AutomationWorkbench from './pages/AutomationWorkbench';
import type {Campaign,Observation,Platform,WorkspaceSnapshot} from './types';

afterEach(()=>{cleanup();setTestTransport(undefined)});

const campaign=(id:string,evidenceMode:Campaign['evidenceMode'],name=id):Campaign=>({id,brandId:'b',name,evidenceMode,questions:[{id:'q',productId:'p',text:'虚构问题'}],platforms:['doubao','yuanbao'],repetitions:1,modelLabel:'虚构模型',searchMode:'无请求',context:'虚构测试',createdAt:'2026-10-01T00:00:00Z'});
const observation=(id:string,values:Partial<Observation>={}):Observation=>({id,brandId:'b',productId:'p',questionId:'q',querySnapshot:'虚构问题',branded:false,platform:'doubao',surface:'consumer',phase:'baseline',modelLabel:'虚构模型',searchMode:'无请求',testedAt:'2026-10-02T00:00:00Z',answer:'虚构回答',mentioned:false,accuracy:'unknown',citedUrl:'',evidenceNote:'虚构测试',assessment:{campaignId:'sim',repetition:1,context:'虚构测试',recommendation:'unknown',rank:null,competitors:''},...values});
const workspace=(observations:Observation[]):WorkspaceSnapshot=>({products:[],questions:[],drafts:[],publications:[],observations,campaigns:[campaign('sim','simulated','模拟版本'),campaign('real','recorded','记录版本')]});

describe('只读巡检请求',()=>{
 it('只生成白名单 DAG，并按选择顺序串联依赖',()=>{
  const nodes=buildAutomationNodes([...AUTOMATION_NODE_KINDS]);
  expect(nodes.map(node=>node.kind)).toEqual(AUTOMATION_NODE_KINDS);
  expect(nodes.map(node=>node.dependsOn)).toEqual([[],['node-1'],['node-2']]);
  expect(buildAutomationNodes(['fact_expiry_scan','fact_expiry_scan'])).toHaveLength(1);
  expect(()=>buildAutomationNodes([])).toThrow();
  expect(()=>buildAutomationNodes(['publish' as never])).toThrow(/只读/);
 });
 it('校验名称、间隔并保留幂等请求编号',()=>{
  const payload=makeAutomationCreatePayload('b','  虚构巡检  ',15,['fact_expiry_scan'],'request-1');
  expect(payload).toMatchObject({brandId:'b',name:'虚构巡检',intervalMinutes:15,requestId:'request-1'});
  expect(()=>makeAutomationCreatePayload('b','',15,['fact_expiry_scan'],'r')).toThrow();
  expect(()=>makeAutomationCreatePayload('b','任务',14,['fact_expiry_scan'],'r')).toThrow(/15/);
  expect(()=>makeAutomationCreatePayload('b','任务',15.5,['fact_expiry_scan'],'r')).toThrow();
 });
 it('只有活动且到期的任务可由显式调用运行',()=>{
  const task={status:'active',nextRunAt:'2026-10-24T01:00:00Z'} as AutomationTask;
  expect(taskIsDue(task,new Date('2026-10-24T00:59:00Z'))).toBe(false);
  expect(taskIsDue(task,new Date('2026-10-24T01:00:00Z'))).toBe(true);
  expect(taskIsDue({...task,status:'paused'},new Date('2026-10-24T02:00:00Z'))).toBe(false);
 });
});

describe('趋势派生与版本比较',()=>{
 it('按日期、样本性质、模型、平台和计划分格，不混合模拟与真实',()=>{
  const points=deriveTrendPoints(workspace([
   observation('s1',{mentioned:true,citedUrl:'https://example.com/evidence'}),
   observation('s2',{assessment:{campaignId:'real',repetition:1,context:'虚构测试',recommendation:'unknown',rank:null,competitors:''}}),
   observation('s3',{modelLabel:'另一虚构模型'}),
   observation('s4',{platform:'yuanbao' as Platform}),
   observation('s5',{testedAt:'2026-10-03T00:00:00Z'})
  ]));
  expect(points).toHaveLength(5);
  expect(new Set(points.map(point=>point.evidenceMode))).toEqual(new Set(['simulated','recorded']));
  expect(points.find(point=>point.key===JSON.stringify(['2026-10-02','simulated','虚构模型','doubao','sim']))).toMatchObject({sampleCount:1,mentionRate:1,citationRate:1,campaignName:'模拟版本'});
 });
 it('提及和引用以全部样本为分母，准确率仅以已复核样本为分母',()=>{
  const points=deriveTrendPoints(workspace([
   observation('1',{mentioned:true,accuracy:'correct'}),
   observation('2',{mentioned:true,accuracy:'incorrect',citedUrl:'https://example.com/a'}),
   observation('3',{accuracy:'unknown'}),
   observation('4',{accuracy:'unknown'})
  ]));
  expect(points).toHaveLength(1);
  expect(points[0]).toMatchObject({sampleCount:4,mentionedCount:2,citedCount:1,accuracyReviewedCount:2,accuracyCorrectCount:1,mentionRate:0.5,citationRate:0.25,accuracyReviewed:0.5,sampleInsufficient:true});
  expect(formatRate(points[0].accuracyReviewed)).toBe('50.0%');
 });
 it('筛选条件不会把未分类历史样本升级成真实记录',()=>{
  const historical=observation('old',{assessment:undefined});
  const points=deriveTrendPoints(workspace([historical]),{evidenceMode:'unspecified'});
  expect(points).toHaveLength(1);
  expect(points[0].evidenceMode).toBe('unspecified');
  expect(evidenceModeLabel('unspecified')).toContain('不得视为真实实测');
  expect(deriveTrendPoints(workspace([historical]),{evidenceMode:'recorded'})).toEqual([]);
 });
 it('支持模型、平台、计划和日期范围筛选',()=>{
  const data=workspace([observation('1'),observation('2',{modelLabel:'模型乙'}),observation('3',{platform:'yuanbao'}),observation('4',{testedAt:'2026-10-05T00:00:00Z'})]);
  expect(deriveTrendPoints(data,{modelLabel:'模型乙'})).toHaveLength(1);
  expect(deriveTrendPoints(data,{platform:'yuanbao'})).toHaveLength(1);
  expect(deriveTrendPoints(data,{campaignId:'missing'})).toEqual([]);
  expect(deriveTrendPoints(data,{from:'2026-10-03',to:'2026-10-05'})).toHaveLength(1);
  expect(deriveTrendPoints(data,{from:'not-a-date'})).toEqual([]);
 });
 it('小样本和未复核准确率明确保留空值与不足提示',()=>{
  const point=deriveTrendPoints(workspace([observation('1')]))[0];
  expect(point.accuracyReviewed).toBeNull();
  expect(point.sampleInsufficient).toBe(true);
  expect(formatRate(point.accuracyReviewed)).toBe('—');
  const comparison=compareTrendVersions([point],[{...point,key:'next',mentionedCount:1,mentionRate:1}]);
  expect(comparison.comparable).toBe(false);
  expect(comparison.note).toContain('不作提升判断');
 });
 it('已有版本可比较描述差值，但不输出提升结论',()=>{
  const makePoints=(campaignId:string,correct:number):TrendPoint[]=>Array.from({length:MIN_TREND_SAMPLES},(_,index)=>deriveTrendPoints(workspace([observation(`${campaignId}-${index}`,{mentioned:index%2===0,accuracy:index<correct?'correct':'incorrect',assessment:{campaignId,repetition:1,context:'虚构测试',recommendation:'unknown',rank:null,competitors:''}})]))[0]);
  const baseline=makePoints('sim',10),comparison=makePoints('real',20);
  const result=compareTrendVersions(baseline,comparison);
  expect(result.comparable).toBe(true);
  expect(result.baseline.sampleCount).toBe(30);
  expect(result.comparison.sampleCount).toBe(30);
  expect(result.accuracyReviewedDelta).toBeCloseTo(1/3);
  expect(result.mentionRateDelta).toBe(0);
  expect(result.note).toContain('不证明因果');
 });
 it('汇总数重新使用原始计数，不对每日百分比取平均',()=>{
  const points=deriveTrendPoints(workspace([observation('1',{mentioned:true}),...Array.from({length:9},(_,index)=>observation(`large-${index}`,{testedAt:'2026-10-03T00:00:00Z'}))]));
  const summary=summarizeTrend(points);
  expect(summary.sampleCount).toBe(10);
  expect(summary.mentionRate).toBe(0.1);
  expect(summary.sampleInsufficient).toBe(true);
 });
});

const report:AutomationReport={id:'report',ranAt:'2026-10-24T00:00:00Z',nodes:[{nodeId:'node-1',kind:'fact_expiry_scan',reminders:[],omitted:0}],modelCalls:0,modelFeeMinor:0,networkRequests:0,publicationWrites:0};
const task=(values:Partial<AutomationTask>={}):AutomationTask=>({format:'m-geo-automation-task-v1',id:'task',brandId:'b',requestId:'request',createFingerprint:'hash',name:'虚构巡检',intervalMinutes:15,nodes:buildAutomationNodes(['fact_expiry_scan']),status:'active',revision:1,createdAt:'2026-10-01T00:00:00Z',updatedAt:'2026-10-01T00:00:00Z',nextRunAt:'2026-10-24T00:00:00Z',lastRunAt:null,totalRuns:0,missedRuns:0,reports:[],...values});

describe('中文巡检工作台',()=>{
 it('进入页面只读列表，创建不自动运行，并发送显式 DAG 与幂等编号',async()=>{
  const calls:{action:string;payload:Record<string,unknown>}[]=[];
  setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});if(action==='automation.list')return {tasks:[]} as T;if(action==='automation.create')return {...task({requestId:payload.requestId as string,name:payload.name as string,intervalMinutes:payload.intervalMinutes as number,nodes:payload.nodes as AutomationTask['nodes']}),alreadyExists:false} as T;throw new Error(`意外动作 ${action}`)});
  render(createElement(AutomationWorkbench,{brandId:'b',data:workspace([])}));
  await waitFor(()=>expect(calls.map(call=>call.action)).toEqual(['automation.list']));
  fireEvent.click(screen.getByRole('button',{name:'创建只读巡检'}));
  await screen.findByText(/只读巡检已创建/);
  expect(calls.map(call=>call.action)).toEqual(['automation.list','automation.create']);
  expect(calls[1].payload.nodes).toEqual(buildAutomationNodes(AUTOMATION_NODE_KINDS));
  expect(calls[1].payload.requestId).toBeTruthy();
 });
 it('只手动运行到期任务并展示零网络、零模型、零发布写入报告',async()=>{
  const calls:string[]=[];
  const saved=task({revision:2,lastRunAt:report.ranAt,totalRuns:1,reports:[report],nextRunAt:'2026-10-24T00:15:00Z'});
  setTestTransport(async<T,>(action:string)=>{calls.push(action);if(action==='automation.list')return {tasks:[task()]} as T;if(action==='automation.runDue')return {tasks:[saved],reports:[report],skipped:0,modelCalls:0,modelFeeMinor:0,networkRequests:0,publicationWrites:0} as T;throw new Error(`意外动作 ${action}`)});
  render(createElement(AutomationWorkbench,{brandId:'b',data:workspace([])}));
  const select=await screen.findByLabelText('已保存巡检');fireEvent.change(select,{target:{value:'task'}});
  fireEvent.click(screen.getByRole('button',{name:'手动运行到期任务'}));
  await screen.findByText(/已生成 1 份只读报告/);
  expect(calls).toEqual(['automation.list','automation.runDue']);
  expect(screen.getByText(/模型调用 0 · 模型费用 0 · 网络请求 0 · 发布写入 0/)).toBeInTheDocument();
  expect(screen.getByRole('region',{name:'最新只读巡检报告'})).toBeInTheDocument();
 });
});
