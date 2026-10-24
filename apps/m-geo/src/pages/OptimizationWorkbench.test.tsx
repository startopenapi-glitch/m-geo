import {act,cleanup,fireEvent,render,screen,waitFor,within} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import {setTestTransport} from '../api';
import type {WorkspaceSnapshot} from '../types';
import type {OptimizationDiagnosis,OptimizationPlan,OptimizationPlanSummary} from '../optimizationTypes';
import OptimizationWorkbench from './OptimizationWorkbench';
const campaign={id:'c',brandId:'b',name:'虚构台灯固定测量',evidenceMode:'simulated' as const,questions:[{id:'q',productId:'p',text:'虚构问题'}],platforms:['doubao' as const,'yuanbao' as const],repetitions:2,modelLabel:'虚构条件',searchMode:'模拟',context:'无历史',createdAt:'2026-10-23T00:00:00Z'};
const data:WorkspaceSnapshot={products:[],questions:[],drafts:[],publications:[],observations:[],campaigns:[campaign,{...campaign,id:'c2',name:'另一虚构测量'}]};
const metrics={samples:2,assessed:1,recommended:1,unknown:1,ranked:1,top3:1,cited:0,incorrect:0};
function diagnosis():OptimizationDiagnosis{return {format:'m-geo-optimization-diagnosis-v1',brandId:'b',campaign,evidenceMode:'simulated',phase:'baseline',asOf:'2026-10-23',products:[],observations:[],metrics:{expectedPerPhase:4,baseline:metrics,followup:{...metrics,samples:1,assessed:1,unknown:0},matchedPairs:1,matchedAssessedPairs:1,matchedBaselineRecommended:0,matchedFollowupRecommended:1,deltaPercentagePoints:100},findings:[{key:'samples_missing:p',code:'samples_missing',productId:'p',priority:'normal',title:'测量样本不完整',detail:'预期 4，已记录 2，缺少 2',section:'visibility',evidenceIds:['o1']}],fingerprint:'frozen-hash'}}
function makePlan():OptimizationPlan{return {format:'m-geo-optimization-plan-v1',id:'plan',brandId:'b',requestId:'00000000-0000-4000-8000-000000000001',createFingerprint:'create-hash',name:'虚构优化计划',campaignId:'c',evidenceMode:'simulated',revision:1,createdAt:'2026-10-23T00:00:00Z',updatedAt:'2026-10-23T00:00:00Z',checkpoints:[{id:'cp1',capturedAt:'2026-10-23T00:00:00Z',diagnosis:diagnosis()}],actions:diagnosis().findings.map(f=>({...f,status:'open',note:'',updatedAt:'2026-10-23T00:00:00Z'}))}}
function summary(value:OptimizationPlan):OptimizationPlanSummary{return {...value,checkpointCount:value.checkpoints.length,actionCount:value.actions.length,doneCount:value.actions.filter(a=>a.status==='done').length}}
let calls:{action:string;payload:Record<string,unknown>}[]=[],plan=makePlan();
afterEach(()=>{cleanup();setTestTransport(undefined);calls=[];plan=makePlan()});
function native(override?:(action:string,payload:Record<string,unknown>)=>unknown){setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{calls.push({action,payload});const result=override?.(action,payload);if(result!==undefined)return await result as T;
if(action==='optimization.list')return [summary(plan)] as T;
if(action==='optimization.get')return structuredClone(plan) as T;
if(action==='optimization.diagnose')return diagnosis() as T;
if(action==='optimization.create'){plan={...plan,requestId:payload.requestId as string};return {...structuredClone(plan),alreadyExists:false} as T}
if(action==='optimization.update'){plan={...plan,revision:plan.revision+1,actions:plan.actions.map(a=>a.key===payload.actionKey?{...a,status:payload.status as typeof a.status,note:payload.note as string}:a)};return structuredClone(plan) as T}
if(action==='optimization.check'){plan={...plan,revision:plan.revision+1,checkpoints:[...plan.checkpoints,{id:'cp2',capturedAt:'2026-10-23T01:00:00Z',diagnosis:{...diagnosis(),phase:'followup',fingerprint:'next',findings:[]}}]};return structuredClone(plan) as T}
if(action==='file.exportOptimization')return null as T;
throw new Error(`Unexpected bridge action: ${action}`)})}
const count=(action:string)=>calls.filter(c=>c.action===action).length;
async function setup(){const view=render(<OptimizationWorkbench brandId="b" data={data}/>);await screen.findByRole('option',{name:/虚构优化计划/});return view}
async function diagnose(){fireEvent.change(screen.getByLabelText('固定测量计划'),{target:{value:'c'}});fireEvent.click(screen.getByRole('button',{name:'只读诊断基线'}));await screen.findByRole('region',{name:'未保存诊断预览'})}
async function select(){fireEvent.change(screen.getByLabelText('已保存优化计划'),{target:{value:'plan'}});await screen.findByRole('region',{name:'优化计划明细'})}
async function create(){await diagnose();fireEvent.change(screen.getByLabelText('优化计划名称'),{target:{value:'虚构优化计划'}});fireEvent.click(screen.getByRole('button',{name:'保存优化计划'}))}
it('diagnose_unsaved_mode_and_missing_coverage',async()=>{native();await setup();await diagnose();const preview=within(screen.getByRole('region',{name:'未保存诊断预览'}));expect(preview.getAllByText(/模拟数据/).length).toBeGreaterThan(0);expect(preview.getByText(/基线样本：2 \/ 4/)).toBeInTheDocument();expect(preview.getByText(/已配对且已判读：1/)).toBeInTheDocument();expect(preview.getByText('测量样本不完整')).toBeInTheDocument();expect(count('optimization.create')).toBe(0);expect(calls.find(c=>c.action==='optimization.diagnose')?.payload).toEqual({brandId:'b',campaignId:'c',phase:'baseline'});expect(calls.every(c=>/^optimization\.(list|diagnose)$/.test(c.action))).toBe(true)});
it('preview_invalidates_when_campaign_changes',async()=>{native();await setup();await diagnose();fireEvent.change(screen.getByLabelText('固定测量计划'),{target:{value:'c2'}});expect(screen.queryByRole('region',{name:'未保存诊断预览'})).toBeNull();expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled()});
it('create_saved_refresh_failed_reload_only',async()=>{let lists=0;native(a=>a==='optimization.list'&&++lists===2?Promise.reject(new Error('读取失败')):undefined);await setup();await create();await screen.findByText(/计划已保存，但列表刷新失败/);expect(screen.getByRole('region',{name:'优化计划明细'})).toBeInTheDocument();expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByText('已重新加载计划列表');expect(count('optimization.create')).toBe(1)});
it('uncertain_update_requires_reload_and_never_replays',async()=>{native(a=>a==='optimization.update'?Promise.reject(new Error('响应中断')):undefined);await setup();await select();fireEvent.change(screen.getByLabelText(/^处理状态：测量样本不完整/),{target:{value:'in_progress'}});fireEvent.click(screen.getByRole('button',{name:/^保存行动：测量样本不完整/}));await screen.findByText(/操作结果未确认/);expect(screen.getByRole('button',{name:'采集复测检查点'})).toBeDisabled();expect(screen.getByRole('button',{name:/^保存行动：测量样本不完整/})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText('已加载保存版本，请核对后手动操作');expect(count('optimization.update')).toBe(1);expect(count('optimization.get')).toBe(2);expect(screen.getByRole('button',{name:'采集复测检查点'})).toBeEnabled()});
it('brandchange_unmount_ignores_late_reply',async()=>{let finish!:(value:OptimizationDiagnosis)=>void;native(a=>a==='optimization.diagnose'?new Promise<OptimizationDiagnosis>(r=>{finish=r}):undefined);const view=await setup();fireEvent.change(screen.getByLabelText('固定测量计划'),{target:{value:'c'}});fireEvent.click(screen.getByRole('button',{name:'只读诊断基线'}));view.rerender(<OptimizationWorkbench brandId="other" data={data}/>);await act(async()=>finish(diagnosis()));expect(screen.queryByRole('region',{name:'未保存诊断预览'})).toBeNull();expect(screen.getByLabelText('固定测量计划')).toHaveValue('');expect(count('optimization.create')).toBe(0);view.unmount()});
it('reentry_readonly',async()=>{native();const view=await setup();await select();view.unmount();await setup();expect(count('optimization.list')).toBe(2);expect(count('optimization.get')).toBe(1);expect(calls.every(c=>['optimization.list','optimization.get'].includes(c.action))).toBe(true)});
it('action_status_is_distinct_from_latest_detection_and_requires_done_note',async()=>{native();await setup();await select();fireEvent.change(screen.getByLabelText(/^处理状态：测量样本不完整/),{target:{value:'done'}});expect(screen.getByRole('button',{name:/^保存行动：测量样本不完整/})).toBeDisabled();fireEvent.change(screen.getByLabelText(/^处理备注：测量样本不完整/),{target:{value:'已补录，等待复核'}});fireEvent.click(screen.getByRole('button',{name:/^保存行动：测量样本不完整/}));await screen.findByText('行动状态已保存；检测结论未因此改变');expect(screen.getByText('最新检查点：仍检测到')).toBeInTheDocument();expect(screen.getByText('人工状态：已处理，等待验证')).toBeInTheDocument();expect(calls.find(c=>c.action==='optimization.update')?.payload).toEqual({brandId:'b',planId:'plan',expectedRevision:1,actionKey:'samples_missing:p',status:'done',note:'已补录，等待复核'})});
it('followup_stale_conflict_blocks_mutation_until_reload',async()=>{native(a=>a==='optimization.check'?Promise.reject(new Error('版本已变化，请重新加载')):undefined);await setup();await select();fireEvent.click(screen.getByRole('button',{name:'采集复测检查点'}));await screen.findByText(/版本已变化，请重新加载/);expect(count('optimization.check')).toBe(1);expect(screen.getByRole('button',{name:'采集复测检查点'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText('已加载保存版本，请核对后手动操作');expect(count('optimization.check')).toBe(1)});
it('followup_keeps_frozen_initial_and_current_detection_separate',async()=>{native();await setup();await select();fireEvent.click(screen.getByRole('button',{name:'采集复测检查点'}));await screen.findByText('复测检查点已保存；请对照冻结指标核对');expect(screen.getByText('最新检查点：未检测到')).toBeInTheDocument();expect(screen.getByLabelText(/^处理状态：测量样本不完整/)).toHaveValue('open');expect(screen.getByRole('region',{name:'初始冻结指标'})).toBeInTheDocument();expect(screen.getByRole('region',{name:'最新冻结指标'})).toBeInTheDocument();expect(count('optimization.diagnose')).toBe(0)});
it('export_cancel_and_save',async()=>{let exports=0;native(a=>a==='file.exportOptimization'&&++exports===2?{saved:true}:undefined);await setup();await select();fireEvent.click(screen.getByRole('button',{name:'导出优化计划 ZIP'}));await screen.findByText('已取消导出，计划记录保留');fireEvent.click(screen.getByRole('button',{name:'导出优化计划 ZIP'}));await screen.findByText('优化计划 ZIP 已保存');expect(count('file.exportOptimization')).toBe(2);expect(count('optimization.update')).toBe(0)});
it('uncertain_create_matches_requestId_then_get_without_replay',async()=>{let lists=0;native((a,p)=>{if(a==='optimization.create'){plan={...plan,requestId:p.requestId as string};return Promise.reject(new Error('响应丢失'))}if(a==='optimization.list')return ++lists===1?[]:[summary(plan)]});render(<OptimizationWorkbench brandId="b" data={data}/>);await waitFor(()=>expect(count('optimization.list')).toBe(1));await create();await screen.findByText(/创建结果未确认/);expect(screen.getByLabelText('固定测量计划')).toBeDisabled();expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByText('已找到原请求保存的计划，请核对明细');expect(screen.getByRole('region',{name:'优化计划明细'})).toBeInTheDocument();expect(count('optimization.create')).toBe(1);expect(count('optimization.get')).toBe(1)});
it('uncertain_create_no_match_retries_only_same_uuid_and_payload_explicitly',async()=>{let attempts=0;native((a,p)=>{if(a==='optimization.list')return [];if(a==='optimization.create'){if(++attempts===1)return Promise.reject(new Error('响应丢失'));plan={...plan,requestId:p.requestId as string};return {...plan,alreadyExists:false}}});render(<OptimizationWorkbench brandId="b" data={data}/>);await waitFor(()=>expect(count('optimization.list')).toBe(1));await create();await screen.findByText(/创建结果未确认/);fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByRole('button',{name:'重试原保存请求'});expect(count('optimization.create')).toBe(1);expect(screen.getByLabelText('优化计划名称')).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:'重试原保存请求'}));await screen.findByText('优化计划已保存');const writes=calls.filter(c=>c.action==='optimization.create');expect(writes).toHaveLength(2);expect(writes[0].payload).toEqual(writes[1].payload);expect(writes[0].payload.requestId).toMatch(/^[0-9a-f-]{36}$/)});
it('empty_state_directs_to_fixed_measurement_without_writing',async()=>{native();const navigate:(s:'products'|'content'|'visibility')=>void=s=>{expect(s).toBe('visibility')};render(<OptimizationWorkbench brandId="b" data={{...data,campaigns:[]}} onNavigate={navigate}/>);expect(screen.getByText(/先在可见度建立固定测量计划/)).toBeInTheDocument();fireEvent.click(screen.getByRole('button',{name:'前往可见度'}));expect(count('optimization.create')).toBe(0)});

it.each(['update','check'] as const)('uncertain_%s_blocks_new_plan_save_until_successful_current_plan_get',async action=>{
 let failGet=false;
 native(a=>a===`optimization.${action}`?Promise.reject(new Error('响应中断，结果未确认')):a==='optimization.get'&&failGet?Promise.reject(new Error('当前计划读取失败')):undefined);
 await setup();await diagnose();fireEvent.change(screen.getByLabelText('优化计划名称'),{target:{value:'另一个虚构优化计划'}});await select();
 if(action==='update'){fireEvent.change(screen.getByLabelText(/^处理状态：测量样本不完整/),{target:{value:'in_progress'}});fireEvent.click(screen.getByRole('button',{name:/^保存行动：测量样本不完整/}))}
 else fireEvent.click(screen.getByRole('button',{name:'采集复测检查点'}));
 await screen.findByText(/操作结果未确认/);
 fireEvent.click(screen.getByRole('button',{name:'保存优化计划'}));
 expect(count('optimization.create')).toBe(0);
 expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled();
 expect(screen.getByLabelText('固定测量计划')).toBeDisabled();
 expect(screen.getByLabelText('优化计划名称')).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByText('已重新加载计划列表');
 expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled();
 failGet=true;fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText(/计划加载失败/);
 fireEvent.click(screen.getByRole('button',{name:'保存优化计划'}));expect(count('optimization.create')).toBe(0);
 failGet=false;fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText('已加载保存版本，请核对后手动操作');
 expect(screen.getByRole('button',{name:'保存优化计划'})).toBeEnabled();
 fireEvent.click(screen.getByRole('button',{name:'保存优化计划'}));await screen.findByText('优化计划已保存');expect(count('optimization.create')).toBe(1);
 expect(count(`optimization.${action}`)).toBe(1);expect(count('optimization.get')).toBe(3);
});

function twoProducts(){
 const d=diagnosis();d.products=[{id:'p',name:'虚构台灯甲',sku:'SKU-A',revision:1,confirmed:false,facts:[],drafts:[]},{id:'p2',name:'虚构台灯乙',sku:'SKU-B',revision:1,confirmed:false,facts:[],drafts:[]}];
 d.findings=[{...d.findings[0],evidenceIds:[]},{...d.findings[0],key:'samples_missing:p2',productId:'p2',evidenceIds:[]}];return d;
}
function twoActionPlan(){const d=twoProducts();plan={...makePlan(),checkpoints:[{...makePlan().checkpoints[0],diagnosis:d}],actions:d.findings.map(f=>({...f,status:'open',note:'',updatedAt:'2026-10-23T00:00:00Z'}))};}
it('final_fix_preview_and_saved_actions_show_distinct_frozen_product_targets',async()=>{
 twoActionPlan();const renamed=twoProducts();renamed.products[0]={...renamed.products[0],name:'改名台灯',sku:'NEW-A',revision:2};plan.checkpoints.push({id:'cp2',capturedAt:'2026-10-23T01:00:00Z',diagnosis:renamed});plan.revision=2;
 native(a=>a==='optimization.diagnose'?twoProducts():undefined);await setup();await diagnose();
 const preview=within(screen.getByRole('region',{name:'未保存诊断预览'}));expect(preview.getByText(/虚构台灯甲.*SKU-A.*p/)).toBeInTheDocument();expect(preview.getByText(/虚构台灯乙.*SKU-B.*p2/)).toBeInTheDocument();
 await select();const detail=within(screen.getByRole('region',{name:'优化计划明细'}));expect(detail.getByLabelText(/处理备注：测量样本不完整.*SKU-A.*p/)).toBeInTheDocument();expect(detail.getByLabelText(/处理备注：测量样本不完整.*SKU-B.*p2/)).toBeInTheDocument();expect(detail.getByText(/首次冻结目标.*虚构台灯甲.*SKU-A/)).toBeInTheDocument();expect(detail.getByText(/最新检查点目标.*改名台灯.*NEW-A/)).toBeInTheDocument();
});
it('final_fix_stale_create_refreshes_only_after_absence_and_reuses_uuid',async()=>{
 let attempts=0,diagnoses=0;native((a,p)=>{if(a==='optimization.list')return [];if(a==='optimization.diagnose')return {...diagnosis(),fingerprint:++diagnoses===1?'old':'fresh'};if(a==='optimization.create'){if(++attempts===1)return Promise.reject(new Error('资料或样本已变化，请重新诊断后保存方案'));if(p.diagnosisFingerprint!=='fresh')return Promise.reject(new Error('仍为过期诊断'));plan={...plan,requestId:p.requestId as string};return {...plan,alreadyExists:false}}});
 render(<OptimizationWorkbench brandId="b" data={data}/>);await waitFor(()=>expect(count('optimization.list')).toBe(1));await create();await screen.findByText(/创建结果未确认/);expect(screen.queryByRole('button',{name:'保留原请求编号，重新诊断编辑'})).toBeNull();
 fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));fireEvent.click(await screen.findByRole('button',{name:'保留原请求编号，重新诊断编辑'}));expect(screen.queryByRole('region',{name:'未保存诊断预览'})).toBeNull();
 fireEvent.click(screen.getByRole('button',{name:'只读诊断基线'}));await screen.findByRole('region',{name:'未保存诊断预览'});fireEvent.change(screen.getByLabelText('优化计划名称'),{target:{value:'重新诊断后计划'}});fireEvent.click(screen.getByRole('button',{name:'保存优化计划'}));await screen.findByText('优化计划已保存');
 const writes=calls.filter(c=>c.action==='optimization.create');expect(writes).toHaveLength(2);expect(writes[1].payload.requestId).toBe(writes[0].payload.requestId);expect(writes[1].payload.diagnosisFingerprint).toBe('fresh');expect(writes[1].payload.name).toBe('重新诊断后计划');
});
it('final_fix_failed_reconciliation_keeps_create_and_refresh_blocked',async()=>{
 let lists=0;native(a=>a==='optimization.list'?(++lists===1?[]:Promise.reject(new Error('读取中断'))):a==='optimization.create'?Promise.reject(new Error('未知传输结果')):undefined);
 render(<OptimizationWorkbench brandId="b" data={data}/>);await waitFor(()=>expect(count('optimization.list')).toBe(1));await create();await screen.findByText(/创建结果未确认/);fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByText(/重新加载失败/);expect(screen.getByRole('button',{name:'只读诊断基线'})).toBeDisabled();expect(screen.getByRole('button',{name:'保存优化计划'})).toBeDisabled();expect(screen.queryByRole('button',{name:'保留原请求编号，重新诊断编辑'})).toBeNull();expect(count('optimization.create')).toBe(1);
});
it('final_fix_refresh_reconciliation_loads_delayed_original_commit',async()=>{
 let lists=0;native((a,p)=>{if(a==='optimization.list')return ++lists<=2?[]:[summary(plan)];if(a==='optimization.create'){plan={...plan,requestId:p.requestId as string};return Promise.reject(new Error('响应丢失'))}});
 render(<OptimizationWorkbench brandId="b" data={data}/>);await waitFor(()=>expect(count('optimization.list')).toBe(1));await create();await screen.findByText(/创建结果未确认/);fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));fireEvent.click(await screen.findByRole('button',{name:'保留原请求编号，重新诊断编辑'}));fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));await screen.findByText('已找到原请求保存的计划，请核对明细');expect(screen.getByRole('region',{name:'优化计划明细'})).toBeInTheDocument();expect(count('optimization.create')).toBe(1);
});
it('final_fix_other_action_draft_survives_save_checkpoint_reload_and_uses_current_revision',async()=>{
 twoActionPlan();native();await setup();await select();const note=()=>screen.getAllByLabelText(/^处理备注：测量样本不完整/)[1],status=()=>screen.getAllByLabelText(/^处理状态：测量样本不完整/)[1];
 fireEvent.change(note(),{target:{value:'第二条尚未保存的调查记录'}});fireEvent.change(status(),{target:{value:'in_progress'}});fireEvent.change(screen.getAllByLabelText(/^处理状态：测量样本不完整/)[0],{target:{value:'in_progress'}});fireEvent.click(screen.getAllByRole('button',{name:/^保存行动：测量样本不完整/})[0]);await screen.findByText('行动状态已保存；检测结论未因此改变');expect(note()).toHaveValue('第二条尚未保存的调查记录');expect(status()).toHaveValue('in_progress');
 fireEvent.click(screen.getByRole('button',{name:'采集复测检查点'}));await screen.findByText('复测检查点已保存；请对照冻结指标核对');expect(note()).toHaveValue('第二条尚未保存的调查记录');fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText('已加载保存版本，请核对后手动操作');expect(note()).toHaveValue('第二条尚未保存的调查记录');
 fireEvent.click(screen.getAllByRole('button',{name:/^保存行动：测量样本不完整/})[1]);await screen.findByText('行动状态已保存；检测结论未因此改变');const writes=calls.filter(c=>c.action==='optimization.update');expect(writes[1].payload).toEqual({brandId:'b',planId:'plan',expectedRevision:3,actionKey:'samples_missing:p2',status:'in_progress',note:'第二条尚未保存的调查记录'});
});
it('final_fix_changed_own_server_fields_preserve_draft_until_explicit_reconcile',async()=>{
 twoActionPlan();native();await setup();await select();const note=()=>screen.getAllByLabelText(/^处理备注：测量样本不完整/)[1];fireEvent.change(note(),{target:{value:'未保存本地记录'}});plan={...plan,revision:2,actions:plan.actions.map(a=>a.productId==='p2'?{...a,note:'另一窗口已保存记录',status:'in_progress'}:a)};
 fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText(/此行动的保存版本已变化/);expect(note()).toHaveValue('未保存本地记录');expect(screen.getAllByRole('button',{name:/^保存行动：测量样本不完整/})[1]).toBeDisabled();fireEvent.click(screen.getByRole('button',{name:/保留本地草稿并以当前版本继续.*SKU-B/}));fireEvent.click(screen.getAllByRole('button',{name:/^保存行动：测量样本不完整/})[1]);await screen.findByText('行动状态已保存；检测结论未因此改变');expect(calls.find(c=>c.action==='optimization.update')?.payload.note).toBe('未保存本地记录');expect(calls.find(c=>c.action==='optimization.update')?.payload.expectedRevision).toBe(2);
});

it.each([['absent','update'],['absent','check'],['late-match','update'],['late-match','check']] as const)('residual_fix_%s_prioritizes_current_plan_%s_read_before_create_reconciliation',async(branch,mutation)=>{
 let lists=0,failCurrentGet=false,createRequest='';
 const late={...makePlan(),id:'late-plan',name:'迟到的原创建计划'};
 native((a,p)=>{
  if(a==='optimization.create'){createRequest=p.requestId as string;late.requestId=createRequest;return Promise.reject(new Error('资料或样本已变化，请重新诊断后保存方案'))}
  if(a==='optimization.list'){lists++;return lists>=3&&branch==='late-match'?[summary(plan),summary(late)]:[summary(plan)]}
  if(a===`optimization.${mutation}`)return Promise.reject(new Error('修改响应中断'));
  if(a==='optimization.get'&&p.planId==='late-plan')return structuredClone(late);
  if(a==='optimization.get'&&failCurrentGet)return Promise.reject(new Error('当前计划读取中断'));
 });
 await setup();await create();await screen.findByText(/创建结果未确认/);fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));fireEvent.click(await screen.findByRole('button',{name:'保留原请求编号，重新诊断编辑'}));await select();
 if(mutation==='update'){fireEvent.change(screen.getByLabelText(/^处理状态：测量样本不完整/),{target:{value:'in_progress'}});fireEvent.click(screen.getByRole('button',{name:/^保存行动：测量样本不完整/}))}
 else fireEvent.click(screen.getByRole('button',{name:'采集复测检查点'}));
 await screen.findByText(/操作结果未确认/);fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));
 await waitFor(()=>expect(screen.queryByText('正在处理本地资料…')).toBeNull());
 expect(screen.getByLabelText('已保存优化计划')).toHaveValue('plan');expect(screen.getByRole('button',{name:'采集复测检查点'})).toBeDisabled();expect(screen.getByRole('button',{name:'重新加载当前计划'})).toBeEnabled();expect(count('optimization.get')).toBe(1);expect(screen.getByText(new RegExp(`原保存请求编号：${createRequest}`))).toBeInTheDocument();
 failCurrentGet=true;fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText(/计划加载失败/);expect(screen.getByRole('button',{name:'采集复测检查点'})).toBeDisabled();expect(screen.getByRole('button',{name:'重新加载当前计划'})).toBeEnabled();
 failCurrentGet=false;fireEvent.click(screen.getByRole('button',{name:'重新加载当前计划'}));await screen.findByText('已加载保存版本，请核对后手动操作');expect(screen.getByLabelText('已保存优化计划')).toHaveValue('plan');expect(count('optimization.get')).toBe(3);
 fireEvent.click(screen.getByRole('button',{name:'重新加载计划列表'}));
 if(branch==='late-match'){await screen.findByText('已找到原请求保存的计划，请核对明细');expect(screen.getByLabelText('已保存优化计划')).toHaveValue('late-plan');expect(calls.filter(c=>c.action==='optimization.get').map(c=>c.payload.planId)).toEqual(['plan','plan','plan','late-plan'])}
 else{await screen.findByRole('button',{name:'保留原请求编号，重新诊断编辑'});expect(screen.getByLabelText('已保存优化计划')).toHaveValue('plan');expect(screen.getByRole('button',{name:'保留原请求编号，重新诊断编辑'})).toBeEnabled()}
 expect(count('optimization.create')).toBe(1);expect(count(`optimization.${mutation}`)).toBe(1);
});
