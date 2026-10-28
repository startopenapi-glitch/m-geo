import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {ResearchEntry} from '../brandResearch';
import type {BrandResearchProps} from './BrandResearch';
import BrandResearch from './BrandResearch';

const product={id:'00000000-0000-4000-8000-000000000001',brandId:'b',name:'虚构星云杯',sku:'TEST-CUP',category:'饮具',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:null,updatedAt:''};
const props:BrandResearchProps={brand:{id:'b',name:'虚构星云品牌',createdAt:'2026-01-01T00:00:00Z'},data:{products:[product],questions:[],drafts:[],observations:[],publications:[]},refresh:vi.fn(async()=>{})};
function research(overrides:Partial<ResearchEntry>={}):ResearchEntry{return {id:'r1',brandId:'b',productId:product.id,question:'虚构星云杯如何清洗？',intent:'usage',sourceKind:'manual',sourceLabel:'虚构客服访谈',sourceUrl:'https://example.com/research',observedAt:'2026-01-02T03:04:00Z',clusterKey:'清洁保养',status:'candidate',revision:1,createdAt:'2026-01-02T03:04:00Z',updatedAt:'2026-01-02T03:04:00Z',...overrides}}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});

it('saves_brand_profile_with_deduplicated_aliases_and_displays_returned_timestamp',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='research.list')return {items:[],clusters:[],truncated:false} as T;
  if(action==='brand.profile.save')return {...props.brand,profile:{aliases:payload.aliases,officialUrl:payload.officialUrl,manufacturer:payload.manufacturer,category:payload.category,description:payload.description,updatedAt:'2026-01-03T00:00:00Z'}} as T;
  throw Error(action);
 });
 render(<BrandResearch {...props}/>);
 await screen.findByText('没有匹配的研究问题');
 fireEvent.change(screen.getByLabelText('品牌官网'),{target:{value:'https://example.com/brand'}});
 fireEvent.change(screen.getByLabelText('制造商 / 运营主体'),{target:{value:'虚构制造商'}});
 fireEvent.change(screen.getByLabelText('品牌品类'),{target:{value:'虚构饮具'}});
 fireEvent.change(screen.getByLabelText('品牌简介'),{target:{value:'虚构品牌介绍'}});
 fireEvent.change(screen.getByLabelText('品牌别名（每行一个）'),{target:{value:'星云\nSTAR\n星云\nstar'}});
 fireEvent.click(screen.getByRole('button',{name:'保存品牌档案'}));
 await screen.findByText('品牌级档案已保存');
 expect(calls.find(call=>call.action==='brand.profile.save')?.payload).toEqual({brandId:'b',aliases:['星云','STAR'],officialUrl:'https://example.com/brand',manufacturer:'虚构制造商',category:'虚构饮具',description:'虚构品牌介绍'});
 expect(screen.getByLabelText('品牌别名（每行一个）')).toHaveValue('星云\nSTAR');
 expect(screen.getByLabelText('档案更新时间')).not.toHaveValue('尚未保存');
});

it('creates_local_suggested_candidate_with_source_and_observation_date',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='research.list')return {items:[],clusters:[],truncated:false} as T;
  if(action==='research.save')return research({id:'created',question:payload.question as string,sourceKind:'suggested',sourceLabel:payload.sourceLabel as string,observedAt:payload.observedAt as string,revision:1}) as T;
  throw Error(action);
 });
 render(<BrandResearch {...props}/>);
 await screen.findByText('没有匹配的研究问题');
 fireEvent.change(screen.getByLabelText('关联商品'),{target:{value:product.id}});
 fireEvent.change(screen.getByLabelText('来源类型'),{target:{value:'suggested'}});
 expect(screen.getByLabelText('来源名称')).toHaveValue('M- GEO 本地模板建议；非真实搜索数据');
 fireEvent.change(screen.getByLabelText('观察时间'),{target:{value:'2026-01-02T03:04'}});
 fireEvent.change(screen.getByLabelText('聚类名称（可选）'),{target:{value:'清洁保养'}});
 fireEvent.change(screen.getByLabelText('研究问题'),{target:{value:'虚构本地建议问题'}});
 fireEvent.click(screen.getByRole('button',{name:'保存候选问题'}));
 await screen.findByText('研究问题已保存为候选');
 const request=calls.find(call=>call.action==='research.save');
 expect(request?.payload).toEqual({brandId:'b',productId:product.id,question:'虚构本地建议问题',intent:'selection',sourceKind:'suggested',sourceLabel:'M- GEO 本地模板建议；非真实搜索数据',sourceUrl:'',observedAt:new Date('2026-01-02T03:04').toISOString(),clusterKey:'清洁保养'});
 expect(screen.getByText('虚构本地建议问题')).toBeInTheDocument();
 expect(calls.filter(call=>call.action==='question.save')).toHaveLength(0);
});

it('filters_clusters_confirms_then_adds_to_fixed_questions_with_refresh',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 const first=research(),second=research({id:'r2',question:'虚构杯身如何保养？'});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='research.list')return {items:[first,second],clusters:[{clusterKey:'清洁保养',count:2,researchIds:['r1','r2']}],truncated:false} as T;
  if(action==='research.confirm')return research({status:'confirmed',revision:2}) as T;
  if(action==='research.confirmToQuestion')return {research:research({status:'confirmed',revision:2}),question:{id:'q1',brandId:'b',productId:product.id,text:first.question,intent:'usage',origin:'customer',source:first.sourceLabel,branded:true,confirmed:true}} as T;
  throw Error(action);
 });
 render(<BrandResearch {...props}/>);
 await screen.findByText('虚构星云杯如何清洗？');
 expect(screen.getByRole('button',{name:'清洁保养（2）'})).toBeInTheDocument();
 fireEvent.change(screen.getAllByLabelText('来源')[0],{target:{value:'imported'}});
 expect(screen.getByText('没有匹配的研究问题')).toBeInTheDocument();
 fireEvent.change(screen.getAllByLabelText('来源')[0],{target:{value:'manual'}});
 fireEvent.change(screen.getByLabelText('关键词'),{target:{value:'杯盖不存在'}});
 expect(screen.getByText('没有匹配的研究问题')).toBeInTheDocument();
 fireEvent.change(screen.getByLabelText('关键词'),{target:{value:'星云杯'}});
 fireEvent.click(screen.getByRole('button',{name:'确认研究问题'}));
 await screen.findByText('研究问题已确认，可加入固定问题集');
 expect(calls.find(call=>call.action==='research.confirm')?.payload).toEqual({brandId:'b',id:'r1',expectedRevision:1});
 fireEvent.click(screen.getByRole('button',{name:'确认加入固定问题集'}));
 await screen.findByText('已加入固定问题集');
 expect(calls.find(call=>call.action==='research.confirmToQuestion')?.payload).toEqual({brandId:'b',id:'r1',expectedRevision:2});
 expect(props.refresh).toHaveBeenCalledTimes(1);
 expect(calls.filter(call=>call.action==='question.save')).toHaveLength(0);
});

it('blocks_duplicate_normalized_question_before_transport_and_preserves_input',async()=>{
 const calls:string[]=[];
 setTestTransport(async<T,>(action:string)=>{calls.push(action);if(action==='research.list')return {items:[research()],clusters:[],truncated:false} as T;throw Error(action)});
 render(<BrandResearch {...props}/>);
 await screen.findByText('虚构星云杯如何清洗？');
 fireEvent.change(screen.getByLabelText('关联商品'),{target:{value:product.id}});
 fireEvent.change(screen.getByLabelText('来源名称'),{target:{value:'虚构第二次访谈'}});
 fireEvent.change(screen.getByLabelText('研究问题'),{target:{value:' 请问 虚构星云杯如何清洗吗 '}});
 await screen.findByText(/同一商品范围内已有重复问法/);
 expect(screen.getByRole('button',{name:'保存候选问题'})).toBeDisabled();
 expect(screen.getByLabelText('研究问题')).toHaveValue(' 请问 虚构星云杯如何清洗吗 ');
 expect(calls.filter(action=>action==='research.save')).toHaveLength(0);
});

it('uses_optimistic_revision_when_archiving_and_disables_generic_fixed_question_action',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 const generic=research({id:'generic',productId:null,status:'confirmed',revision:3,question:'虚构品牌级问题'});
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});
  if(action==='research.list')return {items:[generic],clusters:[],truncated:false} as T;
  if(action==='research.archive')return research({id:'generic',productId:null,status:'archived',revision:4,question:'虚构品牌级问题'}) as T;
  throw Error(action);
 });
 render(<BrandResearch {...props}/>);
 await screen.findByText('虚构品牌级问题');
 expect(screen.getByRole('button',{name:'确认加入固定问题集'})).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'归档'}));
 await screen.findByText('研究问题已归档');
 expect(calls.find(call=>call.action==='research.archive')?.payload).toEqual({brandId:'b',id:'generic',expectedRevision:3});
 await waitFor(()=>expect(screen.queryByRole('button',{name:'归档'})).toBeNull());
});
