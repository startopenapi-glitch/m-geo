import {useEffect,useRef,useState} from 'react';
import type {PageProps} from '../App';
import {Badge,Empty,Field,Panel,submit,time,useTask} from '../components/ui';
import {MEDIA_ACCEPT,createMediaObjectUrl,getMedia,importMedia,listMedia,prepareMediaFile,reviewMedia,reviseMedia,setMediaArchived,type MediaItem,type MediaMetadataInput,type MediaReviewDecision,type MediaReviewStatus,type PreparedMedia} from '../media';

const statusLabels:Record<MediaReviewStatus,string>={pending:'待审核',approved:'已通过',rejected:'已驳回'};
const emptyMetadata=():Omit<MediaMetadataInput,'filename'>=>({alt:'',caption:'',source:'',rightsNote:''});
const errorMessage=(error:unknown)=>error instanceof Error?error.message:'操作未完成，请重试';
const sizeLabel=(size:number)=>size<1024?`${size} B`:`${(size/1024).toFixed(1)} KB`;
type Range='active'|'archived'|'all';
interface Editing{id:string;expectedRevision:number}

export default function MediaLibrary(props:PageProps){
 return <MediaLibrarySession key={props.brand.id} {...props}/>;
}

function MediaLibrarySession({brand}:PageProps){
 const [items,setItems]=useState<MediaItem[]>([]),[selectedId,setSelectedId]=useState('');
 const [range,setRange]=useState<Range>('active'),[listBusy,setListBusy]=useState(false),[listError,setListError]=useState('');
 const [file,setFile]=useState<PreparedMedia|null>(null),[metadata,setMetadata]=useState(emptyMetadata);
 const [fileBusy,setFileBusy]=useState(false),[fileError,setFileError]=useState(''),[fileKey,setFileKey]=useState(0);
 const [editing,setEditing]=useState<Editing|null>(null),[editForm,setEditForm]=useState<MediaMetadataInput>({filename:'',...emptyMetadata()});
 const [replacement,setReplacement]=useState<PreparedMedia|null>(null),[editFileError,setEditFileError]=useState('');
 const [previewUrl,setPreviewUrl]=useState(''),[previewBusy,setPreviewBusy]=useState(false),[previewError,setPreviewError]=useState('');
 const listRequest=useRef(0),fileRequest=useRef(0);
 const importTask=useTask(),reviseTask=useTask(),reviewTask=useTask(),archiveTask=useTask();
 const selected=items.find(item=>item.id===selectedId)??null;
 const visible=items.filter(item=>range==='all'||(range==='archived')===item.archived);
 const busy=fileBusy||importTask.busy||reviseTask.busy||reviewTask.busy||archiveTask.busy;

 async function load(){
  const seq=++listRequest.current;setListBusy(true);setListError('');
  try{
   const result=await listMedia(brand.id);
   if(seq!==listRequest.current)return;
   setItems(result);setSelectedId(current=>result.some(item=>item.id===current)?current:'');
  }catch(error){if(seq===listRequest.current)setListError(errorMessage(error))}
  finally{if(seq===listRequest.current)setListBusy(false)}
 }
 useEffect(()=>{void load();return()=>{listRequest.current++}},[]);

 useEffect(()=>{
  let cancelled=false,url='';
  setPreviewUrl('');setPreviewError('');
  if(!selected){setPreviewBusy(false);return}
  setPreviewBusy(true);
  void getMedia(brand.id,selected.id).then(media=>{
   const created=createMediaObjectUrl(media);
   if(cancelled){URL.revokeObjectURL(created);return}
   url=created;setPreviewUrl(created);
  }).catch(error=>{if(!cancelled)setPreviewError(errorMessage(error))}).finally(()=>{if(!cancelled)setPreviewBusy(false)});
  return()=>{cancelled=true;if(url)URL.revokeObjectURL(url)};
 },[brand.id,selected?.id,selected?.revision]);

 function updateMetadata<K extends keyof Omit<MediaMetadataInput,'filename'>>(key:K,value:string){
  setMetadata(current=>({...current,[key]:value}));
 }
 async function chooseFile(candidate:File|undefined,forEdit=false){
  if(!candidate)return;
  const seq=++fileRequest.current;setFileBusy(true);
  forEdit?setEditFileError(''):setFileError('');
  try{
   const prepared=await prepareMediaFile(candidate);
   if(seq!==fileRequest.current)return;
   if(forEdit)setReplacement(prepared);else setFile(prepared);
  }catch(error){if(seq===fileRequest.current)(forEdit?setEditFileError:setFileError)(errorMessage(error))}
  finally{if(seq===fileRequest.current)setFileBusy(false)}
 }
 function applySaved(saved:MediaItem){
  setItems(current=>[saved,...current.filter(item=>item.id!==saved.id)]);
  setSelectedId(saved.id);
  if(editing?.id===saved.id)cancelEdit();
 }
 async function importSelected(){
  if(!file||busy)return;
  await importTask.run(()=>importMedia(brand.id,file,metadata),saved=>{
   applySaved(saved);setFile(null);setMetadata(emptyMetadata());setFileKey(key=>key+1);setFileError('');
  },'素材已保存到本地资料库；相同内容会复用，未自动公开');
 }
 function beginEdit(item:MediaItem){
  setEditing({id:item.id,expectedRevision:item.revision});
  setEditForm({filename:item.filename,alt:item.alt,caption:item.caption,source:item.source,rightsNote:item.rightsNote});
  setReplacement(null);setEditFileError('');setFileKey(key=>key+1);
 }
 function cancelEdit(){setEditing(null);setReplacement(null);setEditFileError('')}
 async function saveRevision(){
  if(!editing||busy||!editForm.filename.trim())return;
  await reviseTask.run(()=>reviseMedia(brand.id,editing.id,editing.expectedRevision,editForm,replacement??undefined),saved=>{
   applySaved(saved);setFileKey(key=>key+1);
  },'素材已修订并回到待审核，旧审核不再有效');
 }
 async function review(decision:MediaReviewDecision){
  if(!selected||selected.reviewStatus!=='pending'||selected.archived||busy)return;
  if(decision==='approved'&&(!selected.source.trim()||!selected.rightsNote.trim()))return;
  await reviewTask.run(()=>reviewMedia(brand.id,selected.id,selected.revision,decision),applySaved,decision==='approved'?'素材已人工审核通过，仍不会自动公开':'素材已人工驳回');
 }
 async function archive(){
  if(!selected||busy)return;
  const archived=!selected.archived;
  await archiveTask.run(()=>setMediaArchived(brand.id,selected.id,selected.revision,archived),applySaved,archived?'素材已归档，二进制与元数据仍保留在备份中':'素材已恢复，可继续修订或审核');
 }

 return <div className="attribution-workbench">
  <div className="page-heading"><div><h1>图片／视频素材库</h1><p>{brand.name} · 媒体字节与元数据一同保存在可加密备份、可迁移的本地资料库中。</p></div></div>
  <p className="notice">素材库不会自动公开、上传或发布任何素材。审核通过只表示内部素材状态；单文件上限 1.5MB，不保存本机文件路径。</p>

  <Panel title="导入素材" description="文件只在内存中读取并转为 base64 提交；原生端按 magic bytes 复核真实格式与 MIME。">
   <form onSubmit={submit(()=>void importSelected())}>
    <fieldset className="task-fields" disabled={busy}>
     <Field label="选择图片或视频" hint="PNG、JPEG、WebP、GIF、MP4、WebM；真实内容而非扩展名决定格式。"><input key={`import-${fileKey}`} required={!file} type="file" accept={MEDIA_ACCEPT} onChange={event=>void chooseFile(event.target.files?.[0])}/></Field>
     {file&&<p role="status">已读取：{file.filename} · {file.mime} · {sizeLabel(file.size)}</p>}
     {fileError&&<p role="alert" className="feedback error">{fileError}</p>}
     <div className="form-grid">
      <Field label="替代文本（alt）" hint="说明画面内容，供无障碍与后续人工选用。"><textarea rows={2} maxLength={500} value={metadata.alt} onChange={event=>updateMetadata('alt',event.target.value)}/></Field>
      <Field label="素材说明（caption）"><textarea rows={2} maxLength={2000} value={metadata.caption} onChange={event=>updateMetadata('caption',event.target.value)}/></Field>
      <Field label="来源" hint="审核通过前必填；不要填写本机路径或 file:// 链接。"><textarea rows={2} maxLength={2000} value={metadata.source} onChange={event=>updateMetadata('source',event.target.value)}/></Field>
      <Field label="权利说明" hint="审核通过前必填，例如自有、授权范围或许可证记录。"><textarea rows={2} maxLength={4000} value={metadata.rightsNote} onChange={event=>updateMetadata('rightsNote',event.target.value)}/></Field>
     </div>
     <div className="actions"><button className="primary" disabled={busy||!file}>导入为待审核素材</button></div>
    </fieldset>
    {importTask.feedback}
   </form>
  </Panel>

  <Panel title="素材列表" description="列表只读取元数据；选择素材后才读取其二进制用于本地预览。" action={<button disabled={listBusy||busy} onClick={()=>void load()}>重新读取</button>}>
   <Field label="显示范围"><select disabled={busy} value={range} onChange={event=>setRange(event.target.value as Range)}><option value="active">未归档</option><option value="archived">已归档</option><option value="all">全部素材</option></select></Field>
   {listBusy&&<p role="status">正在读取素材列表…</p>}
   {listError&&<p role="alert" className="feedback error">{listError}</p>}
   {!listBusy&&!listError&&!visible.length&&<Empty title="当前范围没有素材">导入素材后，可在此预览、修订、审核或归档。</Empty>}
   {!!visible.length&&<div className="compact-list">{visible.map(item=><button key={item.id} disabled={busy} aria-pressed={selectedId===item.id} onClick={()=>{setSelectedId(item.id);cancelEdit()}}><span><strong>{item.filename}</strong><small>{item.mime} · {sizeLabel(item.size)} · 修订 {item.revision}</small></span><Badge tone={item.archived?'neutral':item.reviewStatus==='approved'?'green':item.reviewStatus==='pending'?'amber':'neutral'}>{item.archived?'已归档':statusLabels[item.reviewStatus]}</Badge></button>)}</div>}
  </Panel>

  {selected&&<Panel title="预览与审核" description="预览使用临时 object URL，切换素材或离开页面即释放；不会生成持久文件路径。">
   <section className="cohort" aria-label="媒体素材详情">
    <h3>{selected.filename}</h3>
    {previewBusy&&<p role="status">正在读取本地预览…</p>}
    {previewError&&<p role="alert" className="feedback error">{previewError}</p>}
    {previewUrl&&(selected.mime.startsWith('image/')?<img src={previewUrl} alt={selected.alt||selected.filename} style={{maxWidth:'100%',maxHeight:360}}/>:<video src={previewUrl} controls preload="metadata" aria-label={`${selected.filename} 视频预览`} style={{maxWidth:'100%',maxHeight:360}}/>)}
    <dl className="attribution-metrics">
     <div><dt>格式</dt><dd>{selected.mime}</dd></div><div><dt>大小</dt><dd>{sizeLabel(selected.size)}</dd></div>
     <div><dt>审核状态</dt><dd>{statusLabels[selected.reviewStatus]}</dd></div><div><dt>修订号</dt><dd>{selected.revision}</dd></div>
     <div><dt>SHA-256</dt><dd>{selected.sha256}</dd></div><div><dt>审核时间</dt><dd>{selected.reviewedAt?time(selected.reviewedAt):'尚未审核'}</dd></div>
     <div><dt>创建时间</dt><dd>{time(selected.createdAt)}</dd></div><div><dt>更新时间</dt><dd>{time(selected.updatedAt)}</dd></div>
    </dl>
    <p>替代文本：{selected.alt||'未填写'}</p><p>素材说明：{selected.caption||'未填写'}</p>
    <p>来源：{selected.source||'未填写'}</p><p>权利说明：{selected.rightsNote||'未填写'}</p>
    {selected.archived&&<p className="notice">素材已归档，不能修订或审核；内容和元数据仍随整库备份保留。</p>}
    <div className="actions"><button disabled={busy||selected.archived} onClick={()=>beginEdit(selected)}>编辑元数据／替换内容</button></div>
    {selected.reviewStatus==='pending'&&(!selected.source.trim()||!selected.rightsNote.trim())&&<p className="notice">请先修订并补齐来源与权利说明，之后才能审核通过。</p>}
    <div className="actions">
     <button className="primary" disabled={busy||selected.archived||selected.reviewStatus!=='pending'||!selected.source.trim()||!selected.rightsNote.trim()} onClick={()=>void review('approved')}>人工审核通过</button>
     <button className="danger" disabled={busy||selected.archived||selected.reviewStatus!=='pending'} onClick={()=>void review('rejected')}>人工驳回</button>
     <button disabled={busy} onClick={()=>void archive()}>{selected.archived?'恢复素材':'归档素材'}</button>
    </div>
    {reviewTask.feedback}{archiveTask.feedback}
   </section>
  </Panel>}

  {editing&&<Panel title="修订素材" description="保存元数据修改或替换二进制都会增加修订号，并使旧审核失效。">
   <form onSubmit={submit(()=>void saveRevision())}>
    <fieldset className="task-fields" disabled={busy}>
     <div className="form-grid">
      <Field label="显示文件名" hint="只保留清洗后的显示名，不参与格式判断。"><input required maxLength={240} value={editForm.filename} onChange={event=>setEditForm(current=>({...current,filename:event.target.value}))}/></Field>
      <Field label="替换文件（可选）" hint="不选择则仅修订元数据；选择后同样按真实字节校验。"><input key={`edit-${fileKey}`} type="file" accept={MEDIA_ACCEPT} onChange={event=>void chooseFile(event.target.files?.[0],true)}/></Field>
      {replacement&&<p role="status">将替换为：{replacement.filename} · {replacement.mime} · {sizeLabel(replacement.size)}</p>}
      <Field label="替代文本（alt）"><textarea rows={2} maxLength={500} value={editForm.alt} onChange={event=>setEditForm(current=>({...current,alt:event.target.value}))}/></Field>
      <Field label="素材说明（caption）"><textarea rows={2} maxLength={2000} value={editForm.caption} onChange={event=>setEditForm(current=>({...current,caption:event.target.value}))}/></Field>
      <Field label="来源"><textarea rows={2} maxLength={2000} value={editForm.source} onChange={event=>setEditForm(current=>({...current,source:event.target.value}))}/></Field>
      <Field label="权利说明"><textarea rows={2} maxLength={4000} value={editForm.rightsNote} onChange={event=>setEditForm(current=>({...current,rightsNote:event.target.value}))}/></Field>
     </div>
     {editFileError&&<p role="alert" className="feedback error">{editFileError}</p>}
     <div className="actions"><button className="primary" disabled={busy||!editForm.filename.trim()}>保存修订并回到待审核</button><button type="button" disabled={busy} onClick={cancelEdit}>取消修订</button></div>
    </fieldset>
   </form>
  </Panel>}
  {reviseTask.feedback}
 </div>;
}
