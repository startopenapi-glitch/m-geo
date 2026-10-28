import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {PageProps} from '../App';
import type {MediaItem} from '../media';
import MediaLibrary from './MediaLibrary';

const props:PageProps={brand:{id:'b',name:'虚构品牌',createdAt:''},data:{products:[],questions:[],drafts:[],observations:[],publications:[]},refresh:vi.fn(async()=>{})};
function item(overrides:Partial<MediaItem>={}):MediaItem{return{id:'m-b',brandId:'b',filename:'虚构素材.png',mime:'image/png',size:8,sha256:'fictional-sha256',alt:'虚构替代文本',caption:'虚构说明',source:'虚构来源记录',rightsNote:'虚构内部授权',reviewStatus:'pending',archived:false,revision:1,createdAt:'2026-10-01T08:00:00Z',updatedAt:'2026-10-01T08:00:00Z',reviewedAt:null,...overrides}}
const png=new Uint8Array([0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a]);
function preview(value:MediaItem){return{...value,data:'iVBORw0KGgo='}}
afterEach(()=>{cleanup();setTestTransport(undefined);vi.clearAllMocks()});
Object.defineProperty(URL,'createObjectURL',{configurable:true,value:vi.fn(()=> 'blob:fictional-media')});
Object.defineProperty(URL,'revokeObjectURL',{configurable:true,value:vi.fn()});

it('loads_metadata_then_reads_bytes_for_safe_local_preview_without_publishing',async()=>{
 const calls:string[]=[];
 setTestTransport(async<T,>(action:string)=>{calls.push(action);if(action==='media.list')return[item()]as T;if(action==='media.get')return preview(item())as T;throw Error(action)});
 const view=render(<MediaLibrary {...props}/>);
 fireEvent.click(await screen.findByRole('button',{name:/虚构素材.png/}));
 expect(await screen.findByAltText('虚构替代文本')).toHaveAttribute('src','blob:fictional-media');
 expect(screen.getByText(/素材库不会自动公开、上传或发布任何素材/)).toBeInTheDocument();
 expect(calls).toEqual(['media.list','media.get']);
 view.unmount();
 expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:fictional-media');
 expect(calls.filter(action=>action.startsWith('publication.')||action.startsWith('ai.'))).toHaveLength(0);
});

it('imports_selected_file_as_base64_with_rights_metadata_and_no_filepath',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});if(action==='media.list')return[]as T;
  if(action==='media.import')return item({filename:payload.filename as string,source:payload.source as string,rightsNote:payload.rightsNote as string})as T;
  if(action==='media.get')return preview(item())as T;throw Error(action);
 });
 render(<MediaLibrary {...props}/>);await screen.findByText('当前范围没有素材');
 fireEvent.change(screen.getByLabelText('选择图片或视频'),{target:{files:[new File([png],'fake.jpg',{type:'image/png'})]}});
 await screen.findByText(/已读取：fake.jpg · image\/png/);
 fireEvent.change(screen.getByLabelText('来源'),{target:{value:'虚构导入来源'}});
 fireEvent.change(screen.getByLabelText('权利说明'),{target:{value:'虚构导入授权'}});
 fireEvent.click(screen.getByRole('button',{name:'导入为待审核素材'}));
 await screen.findByText(/素材已保存到本地资料库/);
 const request=calls.find(call=>call.action==='media.import');
 expect(request?.payload).toMatchObject({brandId:'b',filename:'fake.jpg',mime:'image/png',data:'iVBORw0KGgo=',source:'虚构导入来源',rightsNote:'虚构导入授权'});
 expect(JSON.stringify(request?.payload)).not.toContain('file://');
 expect(calls.filter(call=>call.action==='media.get')).toHaveLength(1);
});

it('requires_rights_before_approval_and_revising_approved_media_returns_to_pending',async()=>{
 const approved=item({reviewStatus:'approved',reviewedAt:'2026-10-02T08:00:00Z',revision:2});
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  calls.push({action,payload});if(action==='media.list')return[approved]as T;if(action==='media.get')return preview(approved)as T;
  if(action==='media.revise')return item({...approved,...payload,reviewStatus:'pending',reviewedAt:null,revision:3})as T;
  throw Error(action);
 });
 render(<MediaLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构素材.png/}));
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeDisabled();
 fireEvent.click(screen.getByRole('button',{name:'编辑元数据／替换内容'}));
 fireEvent.change(screen.getAllByLabelText('替代文本（alt）')[1],{target:{value:'虚构修订替代文本'}});
 fireEvent.click(screen.getByRole('button',{name:'保存修订并回到待审核'}));
 await screen.findByText('素材已修订并回到待审核，旧审核不再有效');
 expect(calls.find(call=>call.action==='media.revise')?.payload).toMatchObject({brandId:'b',id:'m-b',expectedRevision:2,alt:'虚构修订替代文本'});
 expect(calls.find(call=>call.action==='media.revise')?.payload).not.toHaveProperty('data');
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeEnabled();
});

it('blocks_approval_when_rights_are_missing_and_allows_rejection',async()=>{
 let current=item({source:'',rightsNote:''});const decisions:Record<string,unknown>[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='media.list')return[current]as T;if(action==='media.get')return preview(current)as T;
  if(action==='media.review'){decisions.push(payload);current={...current,reviewStatus:'rejected',reviewedAt:'2026-10-03T08:00:00Z',revision:2};return current as T}
  throw Error(action);
 });
 render(<MediaLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构素材.png/}));
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeDisabled();
 expect(screen.getByText(/请先修订并补齐来源与权利说明/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole('button',{name:'人工驳回'}));
 await waitFor(()=>expect(decisions[0]).toMatchObject({decision:'rejected',expectedRevision:1}));
});

it('approves_and_archives_with_the_current_optimistic_revision',async()=>{
 let current=item();const writes:{action:string;payload:Record<string,unknown>}[]=[];
 setTestTransport(async<T,>(action:string,payload:Record<string,unknown>)=>{
  if(action==='media.list')return[current]as T;if(action==='media.get')return preview(current)as T;
  if(action==='media.review'){writes.push({action,payload});current={...current,reviewStatus:'approved',reviewedAt:'2026-10-03T08:00:00Z',revision:2};return current as T}
  if(action==='media.setArchived'){writes.push({action,payload});current={...current,archived:true,revision:3};return current as T}throw Error(action);
 });
 render(<MediaLibrary {...props}/>);fireEvent.click(await screen.findByRole('button',{name:/虚构素材.png/}));
 fireEvent.click(screen.getByRole('button',{name:'人工审核通过'}));
 await screen.findByText('素材已人工审核通过，仍不会自动公开');
 fireEvent.click(screen.getByRole('button',{name:'归档素材'}));
 await screen.findByText('素材已归档，二进制与元数据仍保留在备份中');
 expect(writes).toEqual([
  {action:'media.review',payload:{brandId:'b',id:'m-b',expectedRevision:1,decision:'approved'}},
  {action:'media.setArchived',payload:{brandId:'b',id:'m-b',expectedRevision:2,archived:true}},
 ]);
 expect(screen.getByRole('button',{name:'编辑元数据／替换内容'})).toBeDisabled();
 expect(screen.getByRole('button',{name:'人工审核通过'})).toBeDisabled();
});
