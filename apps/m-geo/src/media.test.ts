import {describe,expect,it,vi} from 'vitest';
import {createMediaObjectUrl,detectMediaMime,prepareMediaFile,type MediaPreview} from './media';

const bytes=(text:string)=>new TextEncoder().encode(text);
const png=new Uint8Array([0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a]);
const preview:MediaPreview={id:'m',brandId:'b',filename:'虚构.png',mime:'image/png',size:8,sha256:'fictional',alt:'虚构替代文本',caption:'',source:'虚构来源',rightsNote:'虚构授权',reviewStatus:'pending',archived:false,revision:1,createdAt:'',updatedAt:'',reviewedAt:null,data:'iVBORw0KGgo='};

describe('media content boundary',()=>{
 it('detects_supported_magic_bytes_instead_of_extensions',()=>{
  expect(detectMediaMime(png)).toBe('image/png');
  expect(detectMediaMime(new Uint8Array([0xff,0xd8,0xff,0xd9]))).toBe('image/jpeg');
  expect(detectMediaMime(bytes('RIFF\0\0\0\0WEBP'))).toBe('image/webp');
  expect(detectMediaMime(bytes('GIF89a'))).toBe('image/gif');
  expect(detectMediaMime(bytes('\0\0\0\x18ftypisom'))).toBe('video/mp4');
  expect(detectMediaMime(new Uint8Array([0x1a,0x45,0xdf,0xa3,...bytes('webm')]))).toBe('video/webm');
  expect(()=>detectMediaMime(bytes('not-a-png'))).toThrow(/真实内容|扩展名|无法识别/);
  expect(()=>detectMediaMime(new Uint8Array([0x1a,0x45,0xdf,0xa3,...bytes('matroska')]))).toThrow();
 });

 it('reads_file_in_memory_sanitizes_display_name_and_rejects_mime_mismatch',async()=>{
  const file=new File([png],'private.png',{type:'image/png'});
  const prepared=await prepareMediaFile(file);
  expect(prepared).toEqual({filename:'private.png',mime:'image/png',data:'iVBORw0KGgo=',size:8});
  await expect(prepareMediaFile(new File([png],'fake.jpg',{type:'image/jpeg'}))).rejects.toThrow(/真实内容/);
 });

 it('builds_only_short_lived_object_urls_after_validating_preview_data',()=>{
  const create=vi.fn(()=> 'blob:fictional-preview');
  Object.defineProperty(URL,'createObjectURL',{configurable:true,value:create});
  expect(createMediaObjectUrl(preview)).toBe('blob:fictional-preview');
  expect(create).toHaveBeenCalledOnce();
  expect(()=>createMediaObjectUrl({...preview,mime:'image/jpeg'})).toThrow(/元数据不一致/);
  expect(()=>createMediaObjectUrl({...preview,data:'file:///Users/private.png'})).toThrow(/base64|无效/);
 });
});
