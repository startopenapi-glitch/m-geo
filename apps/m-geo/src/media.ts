import {call} from './api';

export const MAX_MEDIA_BYTES=1_500_000;
export const MAX_MEDIA_BASE64_CHARS=2_000_000;
export const MEDIA_ACCEPT='image/png,image/jpeg,image/webp,image/gif,video/mp4,video/webm,.png,.jpg,.jpeg,.webp,.gif,.mp4,.webm';
export type MediaMime='image/png'|'image/jpeg'|'image/webp'|'image/gif'|'video/mp4'|'video/webm';
export type MediaReviewStatus='pending'|'approved'|'rejected';
export type MediaReviewDecision=Exclude<MediaReviewStatus,'pending'>;

export interface MediaItem {
 id:string;
 brandId:string;
 filename:string;
 mime:MediaMime;
 size:number;
 sha256:string;
 alt:string;
 caption:string;
 source:string;
 rightsNote:string;
 reviewStatus:MediaReviewStatus;
 archived:boolean;
 revision:number;
 createdAt:string;
 updatedAt:string;
 reviewedAt:string|null;
}
export interface MediaPreview extends MediaItem {data:string}
export interface MediaMetadataInput {
 filename:string;
 alt:string;
 caption:string;
 source:string;
 rightsNote:string;
}
export interface PreparedMedia {
 filename:string;
 mime:MediaMime;
 data:string;
 size:number;
}
export interface MediaListFilter {archived?:boolean;reviewStatus?:MediaReviewStatus}

export function listMedia(brandId:string,filter:MediaListFilter={}){
 return call<MediaItem[]>('media.list',{brandId,...filter});
}
export function getMedia(brandId:string,id:string){
 return call<MediaPreview>('media.get',{brandId,id});
}
export function importMedia(brandId:string,file:PreparedMedia,metadata:Omit<MediaMetadataInput,'filename'>){
 return call<MediaItem>('media.import',{brandId,filename:file.filename,mime:file.mime,data:file.data,...metadata});
}
export function reviseMedia(brandId:string,id:string,expectedRevision:number,metadata:MediaMetadataInput,replacement?:PreparedMedia){
 return call<MediaItem>('media.revise',{brandId,id,expectedRevision,...metadata,...(replacement?{mime:replacement.mime,data:replacement.data}:{})});
}
export function reviewMedia(brandId:string,id:string,expectedRevision:number,decision:MediaReviewDecision){
 return call<MediaItem>('media.review',{brandId,id,expectedRevision,decision});
}
export function setMediaArchived(brandId:string,id:string,expectedRevision:number,archived:boolean){
 return call<MediaItem>('media.setArchived',{brandId,id,expectedRevision,archived});
}

function starts(bytes:Uint8Array,signature:number[],offset=0){
 return signature.every((byte,index)=>bytes[offset+index]===byte);
}
function ascii(bytes:Uint8Array,start:number,length:number){
 return String.fromCharCode(...bytes.subarray(start,start+length));
}

/** Content sniffing is only an early UI check; the native boundary validates again. */
export function detectMediaMime(bytes:Uint8Array):MediaMime{
 if(starts(bytes,[0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a]))return 'image/png';
 if(bytes.length>=4&&starts(bytes,[0xff,0xd8,0xff])&&starts(bytes,[0xff,0xd9],bytes.length-2))return 'image/jpeg';
 if(bytes.length>=12&&ascii(bytes,0,4)==='RIFF'&&ascii(bytes,8,4)==='WEBP')return 'image/webp';
 if(ascii(bytes,0,6)==='GIF87a'||ascii(bytes,0,6)==='GIF89a')return 'image/gif';
 if(bytes.length>=12&&ascii(bytes,4,4)==='ftyp')return 'video/mp4';
 if(starts(bytes,[0x1a,0x45,0xdf,0xa3])&&ascii(bytes,0,Math.min(bytes.length,4096)).toLowerCase().includes('webm'))return 'video/webm';
 throw new Error('无法识别媒体内容；仅支持 PNG、JPEG、WebP、GIF、MP4 和 WebM，不能只看扩展名');
}

function encodeBase64(bytes:Uint8Array){
 let binary='';
 for(let offset=0;offset<bytes.length;offset+=0x8000){
  binary+=String.fromCharCode(...bytes.subarray(offset,offset+0x8000));
 }
 return btoa(binary);
}
function decodeBase64(data:string){
 if(!data||data.length>MAX_MEDIA_BASE64_CHARS||data.length%4!==0)throw new Error('媒体预览数据无效或超过大小上限');
 let binary:string;
 try{binary=atob(data)}catch{throw new Error('媒体预览数据不是有效的 base64')}
 const bytes=Uint8Array.from(binary,char=>char.charCodeAt(0));
 if(!bytes.length||bytes.length>MAX_MEDIA_BYTES)throw new Error('媒体预览内容为空或超过 1.5MB 上限');
 return bytes;
}
function readArrayBuffer(file:File):Promise<ArrayBuffer>{
 return new Promise((resolve,reject)=>{
  const reader=new FileReader();
  reader.onerror=()=>reject(new Error('无法读取所选媒体，请重新选择'));
  reader.onload=()=>reader.result instanceof ArrayBuffer?resolve(reader.result):reject(new Error('所选媒体读取结果无效'));
  reader.readAsArrayBuffer(file);
 });
}

/** Reads the user-selected File in memory only. It never sends a path or reads a path later. */
export async function prepareMediaFile(file:File):Promise<PreparedMedia>{
 if(!file.size||file.size>MAX_MEDIA_BYTES)throw new Error('请选择非空且不超过 1.5MB 的图片或视频');
 const bytes=new Uint8Array(await readArrayBuffer(file));
 if(bytes.length!==file.size||bytes.length>MAX_MEDIA_BYTES)throw new Error('媒体读取大小不一致或超过 1.5MB 上限');
 const mime=detectMediaMime(bytes);
 if(file.type&&file.type!==mime)throw new Error(`文件声明类型 ${file.type} 与真实内容 ${mime} 不一致`);
 const data=encodeBase64(bytes);
 if(data.length>MAX_MEDIA_BASE64_CHARS)throw new Error('媒体 base64 数据超过 2MB 上限');
 return {filename:file.name.split(/[\\/]/).pop()?.trim()||'media',mime,data,size:bytes.length};
}

/** Creates a short-lived local object URL; callers must revoke it when replacing/unmounting. */
export function createMediaObjectUrl(media:MediaPreview){
 const bytes=decodeBase64(media.data);
 const detected=detectMediaMime(bytes);
 if(detected!==media.mime||bytes.length!==media.size)throw new Error('媒体预览内容与元数据不一致');
 return URL.createObjectURL(new Blob([bytes.buffer as ArrayBuffer],{type:media.mime}));
}
