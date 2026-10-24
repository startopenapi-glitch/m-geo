import {act,fireEvent,render,screen,waitFor,within} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import {setTestTransport,type Transport} from '../api';
import type {WorkspaceSnapshot} from '../types';
import type {InspectionReport,SiteInspectionEnvelope} from '../siteInspectionTypes';
import SitePackageInspector from './SitePackageInspector';

const data:WorkspaceSnapshot={products:[],questions:[],drafts:[],observations:[],publications:[]};
const selectName='选择网站 ZIP 并只读验收',recheckName='重新核对当前品牌',exportName='导出验收报告 ZIP';
const report:InspectionReport={
 format:'m-geo-site-inspection-v1',brandId:'brand-a',checkedAt:'2026-10-24T08:00:00Z',
 source:{fileName:'虚构网站.zip',sha256:'a'.repeat(64),compressedBytes:1024,uncompressedBytes:4096,fileCount:6},
 packageStatus:'pass',checks:[{code:'manifest',status:'pass',message:'本地清单一致',path:'release-manifest.json'},{code:'remote',status:'warning',message:'外部状态尚未验证'}],
 previewOnly:true,baseUrl:null,
 products:[{productId:'product-1',name:'虚构水杯',sku:'CUP-001',packageRevision:2,currentRevision:2,status:'match',detail:'商品与审核稿件版本一致',drafts:[{id:'draft-1',packageRevision:3,currentRevision:3,status:'match'}]}],
 currentStatus:'match',remoteStatus:'not-verified',publicationStatus:'not-published',reportFingerprint:'fingerprint-1'
};
const envelope:SiteInspectionEnvelope={inspectionId:'inspection-1',report};
function button(name:string){return screen.getByRole('button',{name})}
function deferred<T>(){let resolve!:(value:T)=>void;const promise=new Promise<T>(yes=>{resolve=yes});return {promise,resolve}}
function serve(handler:(action:string,payload:Record<string,unknown>)=>Promise<unknown>|unknown){setTestTransport((async(action,payload)=>handler(action,payload)) as Transport)}
async function inspect(){fireEvent.click(button(selectName));await screen.findByText('虚构网站.zip')}
afterEach(()=>setTestTransport(undefined));

it('starts empty and reentry never automatically selects, rechecks or exports',()=>{
 const calls:string[]=[];serve(action=>{calls.push(action);return envelope});
 const first=render(<SitePackageInspector brandId="brand-a" data={data}/>);
 expect(button(recheckName)).toBeDisabled();expect(button(exportName)).toBeDisabled();
 first.unmount();render(<SitePackageInspector brandId="brand-a" data={data}/>);
 expect(screen.queryByText('虚构网站.zip')).toBeNull();expect(calls).toEqual([]);
});
it('manually inspects read only and distinguishes local, current and remote evidence with versions',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];serve((action,payload)=>{calls.push({action,payload});return envelope});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();
 const region=screen.getByRole('region',{name:'已导出网站包验收'});
 expect(within(region).getByText('本地文件一致性：通过')).toBeInTheDocument();
 expect(within(region).getByText('当前品牌版本：一致')).toBeInTheDocument();
 expect(screen.getByText('a'.repeat(64))).toBeInTheDocument();
 expect(screen.getByText(/压缩 1024 字节.*解压 4096 字节.*6 个文件/)).toBeInTheDocument();
 expect(screen.getByText('仅预览（noindex）')).toBeInTheDocument();
 expect(screen.getByText('远程状态：未验证')).toBeInTheDocument();expect(screen.getByText('发布状态：尚未发布')).toBeInTheDocument();
 expect(screen.getByText(/商品 1.*通过 1.*警告 1.*阻断 0/)).toBeInTheDocument();
 expect(screen.getByText('虚构水杯')).toBeInTheDocument();expect(screen.getByText(/CUP-001/)).toBeInTheDocument();expect(screen.getByText(/product-1/)).toBeInTheDocument();
 expect(screen.getByText('商品版本：包内 2 · 当前 2 · 一致')).toBeInTheDocument();
 expect(screen.getByText('稿件 draft-1：包内 3 · 当前 3 · 一致')).toBeInTheDocument();
 expect(screen.getByText(/本地清单一致/)).toBeInTheDocument();
 expect(screen.getByText('仅核对本地文件与当前品牌，不证明文件来源可信、官网可达、收录或推荐；哈希不等于数字签名。')).toBeInTheDocument();
 expect(calls).toEqual([{action:'file.inspectSitePackage',payload:{brandId:'brand-a'}}]);expect(button(exportName)).toBeEnabled();
});
it('shows package pass separately from changed current revisions and configured domain',async()=>{
 serve(()=>({inspectionId:'changed',report:{...report,previewOnly:false,baseUrl:'https://example.invalid/shop/',currentStatus:'changed',products:[{...report.products[0],currentRevision:4,status:'changed',drafts:[{id:'draft-1',packageRevision:3,currentRevision:5,status:'changed'}]}]}}));
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();
 expect(screen.getByText('本地文件一致性：通过')).toBeInTheDocument();expect(screen.getByText('当前品牌版本：已变化')).toBeInTheDocument();
 expect(screen.getByText('已配置域名（未验证）')).toBeInTheDocument();expect(screen.getByText('https://example.invalid/shop/')).toBeInTheDocument();
 expect(screen.getByText('商品版本：包内 2 · 当前 4 · 已变化')).toBeInTheDocument();
 expect(screen.getByText('稿件 draft-1：包内 3 · 当前 5 · 已变化')).toBeInTheDocument();expect(button(exportName)).toBeEnabled();
});
it('keeps blocked and unavailable evidence readable and permits exporting its report',async()=>{
 serve(()=>({inspectionId:'blocked',report:{...report,packageStatus:'blocked',currentStatus:'unavailable',previewOnly:null,products:[{...report.products[0],currentRevision:null,status:'unavailable',drafts:[{id:'draft-1',packageRevision:3,currentRevision:null,status:'unavailable'}]}],checks:[{code:'hash',status:'blocked',message:'文件校验不一致',path:'index.html'}]}}));
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();
 expect(screen.getByText('本地文件一致性：阻断')).toBeInTheDocument();expect(screen.getByText('当前品牌版本：不可用')).toBeInTheDocument();
 expect(screen.getByText('预览配置：不可判定')).toBeInTheDocument();expect(screen.getByText(/阻断.*文件校验不一致/)).toBeInTheDocument();
 expect(screen.getByText('商品版本：包内 2 · 当前 不可用 · 不可用')).toBeInTheDocument();expect(button(exportName)).toBeEnabled();
});
it('renders untrusted report fields as text and omits additional raw content',async()=>{
 const unsafe='<img src=x onerror=alert(1)>';
 serve(()=>({inspectionId:'safe',report:{...report,source:{...report.source,fileName:unsafe+'.zip'},baseUrl:'javascript:alert(1)',products:[{...report.products[0],name:unsafe,sku:'<script>sku</script>',detail:'<iframe>detail</iframe>'}],checks:[{code:'<svg>code</svg>',status:'warning',message:'<script>check</script>',path:'<iframe>path</iframe>'}],rawHtml:'PRIVATE-RAW-SENTINEL',privateNotes:'PRIVATE-NOTE-SENTINEL'}}));
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);fireEvent.click(button(selectName));await screen.findByText(unsafe);
 expect(view.container.querySelector('img,script,iframe,svg,object,embed,a')).toBeNull();
 expect(view.container.textContent).not.toContain('PRIVATE-RAW-SENTINEL');expect(view.container.textContent).not.toContain('PRIVATE-NOTE-SENTINEL');
 expect(screen.getByText('<iframe>detail</iframe>')).toBeInTheDocument();
});
it('clears the prior report before reselection and keeps cancellation empty with manual retry',async()=>{
 const pending=deferred<SiteInspectionEnvelope|null>();let selections=0;serve(()=>++selections===1?envelope:selections===2?pending.promise:envelope);
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(selectName));
 expect(screen.queryByText('虚构网站.zip')).toBeNull();expect(button(exportName)).toBeDisabled();
 await act(async()=>pending.resolve(null));await screen.findByText(/已取消选择/);
 expect(button(recheckName)).toBeDisabled();await inspect();expect(button(exportName)).toBeEnabled();
});
it('selection errors do not restore old success or automatically retry',async()=>{
 let calls=0;serve(()=>{if(++calls===1)return envelope;throw Error('网站 ZIP 无法读取')});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(selectName));
 expect(await screen.findByRole('alert')).toHaveTextContent('网站 ZIP 无法读取');expect(screen.queryByText('虚构网站.zip')).toBeNull();
 expect(button(exportName)).toBeDisabled();expect(calls).toBe(2);expect(button(selectName)).toBeEnabled();
});
it('serializes doubleclicks across selection, recheck and export',async()=>{
 const first=deferred<SiteInspectionEnvelope|null>(),second=deferred<SiteInspectionEnvelope>(),third=deferred<{saved:true}|null>();
 const calls:string[]=[];serve(action=>{calls.push(action);return calls.length===1?first.promise:calls.length===2?second.promise:third.promise});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);
 fireEvent.click(button(selectName));fireEvent.click(button(selectName));expect(calls).toEqual(['file.inspectSitePackage']);
 await act(async()=>first.resolve(envelope));
 fireEvent.click(button(recheckName));fireEvent.click(button(recheckName));fireEvent.click(button(selectName));
 expect(calls).toEqual(['file.inspectSitePackage','file.recheckSitePackage']);
 await act(async()=>second.resolve(envelope));
 fireEvent.click(button(exportName));fireEvent.click(button(exportName));fireEvent.click(button(recheckName));
 expect(calls).toEqual(['file.inspectSitePackage','file.recheckSitePackage','file.exportSiteInspection']);
 await act(async()=>third.resolve(null));expect(button(exportName)).toBeEnabled();
});
it('invalidates currency on snapshot replacement until an explicit successful recheck',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];
 serve((action,payload)=>{calls.push({action,payload});return action==='file.exportSiteInspection'?null:{inspectionId:'inspection-2',report:{...report,reportFingerprint:'fingerprint-2'}}});
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();
 view.rerender(<SitePackageInspector brandId="brand-a" data={{...data}}/>);
 expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();expect(screen.getByText(/当前资料已变化.*重新核对/)).toBeInTheDocument();
 expect(button(exportName)).toBeDisabled();expect(calls).toHaveLength(1);
 fireEvent.click(button(recheckName));await waitFor(()=>expect(button(exportName)).toBeEnabled());
 fireEvent.click(button(exportName));await waitFor(()=>expect(calls).toHaveLength(3));
 expect(calls[1]).toEqual({action:'file.recheckSitePackage',payload:{brandId:'brand-a',inspectionId:'inspection-2'}});
 expect(calls[2]).toEqual({action:'file.exportSiteInspection',payload:{brandId:'brand-a',inspectionId:'inspection-2',reportFingerprint:'fingerprint-2'}});
});
it('preserves an outdated report after failed recheck and exposes cache loss reselection',async()=>{
 let calls=0;serve(()=>{if(++calls===1)return envelope;throw Error('验收会话已失效，请重新选择网站 ZIP')});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(recheckName));
 expect(await screen.findByRole('alert')).toHaveTextContent(/重新选择网站 ZIP/);expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();
 expect(screen.getByText(/报告已过时/)).toBeInTheDocument();expect(button(exportName)).toBeDisabled();expect(button(selectName)).toBeEnabled();expect(calls).toBe(2);
});
it('does not mark recheck current if snapshot changes while pending',async()=>{
 const pending=deferred<SiteInspectionEnvelope>();let calls=0;serve(()=>++calls===1?envelope:pending.promise);
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(recheckName));
 view.rerender(<SitePackageInspector brandId="brand-a" data={{...data}}/>);
 await act(async()=>pending.resolve({...envelope,report:{...report,reportFingerprint:'old-during-recheck'}}));
 expect(button(exportName)).toBeDisabled();expect(screen.getByText(/当前资料已变化.*重新核对/)).toBeInTheDocument();expect(calls).toBe(2);
});
it('keeps selection received after snapshot changes visible but outdated',async()=>{
 const pending=deferred<SiteInspectionEnvelope>();serve(()=>pending.promise);
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);fireEvent.click(button(selectName));
 view.rerender(<SitePackageInspector brandId="brand-a" data={{...data}}/>);
 await act(async()=>pending.resolve(envelope));expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();expect(button(exportName)).toBeDisabled();
});
it('ignores late selection replies across brand switches including switch back',async()=>{
 const pending=deferred<SiteInspectionEnvelope>();const calls:string[]=[];serve((action,payload)=>{calls.push(action+':'+payload.brandId);return pending.promise});
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);fireEvent.click(button(selectName));
 view.rerender(<SitePackageInspector brandId="brand-b" data={data}/>);view.rerender(<SitePackageInspector brandId="brand-a" data={data}/>);
 await act(async()=>pending.resolve(envelope));
 expect(screen.queryByText('虚构网站.zip')).toBeNull();expect(button(exportName)).toBeDisabled();expect(button(selectName)).toBeEnabled();
 expect(calls).toEqual(['file.inspectSitePackage:brand-a']);
});
it('ignores late recheck replies after unmount and leaves reentry empty',async()=>{
 const pending=deferred<SiteInspectionEnvelope>();let calls=0;serve(()=>++calls===1?envelope:pending.promise);
 const first=render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(recheckName));first.unmount();
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await act(async()=>pending.resolve(envelope));
 expect(screen.queryByText('虚构网站.zip')).toBeNull();expect(button(exportName)).toBeDisabled();expect(calls).toBe(2);
});
it('exports only token and fingerprint and cancellation leaves the report available',async()=>{
 const calls:{action:string;payload:Record<string,unknown>}[]=[];serve((action,payload)=>{calls.push({action,payload});return action==='file.exportSiteInspection'?null:envelope});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(exportName));
 await screen.findByText('已取消导出验收报告，原报告仍可查看。');expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();expect(button(exportName)).toBeEnabled();
 expect(calls[1]).toEqual({action:'file.exportSiteInspection',payload:{brandId:'brand-a',inspectionId:'inspection-1',reportFingerprint:'fingerprint-1'}});
});
it('announces a saved local report without implying remote verification',async()=>{
 serve(action=>action==='file.exportSiteInspection'?{saved:true}:envelope);
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(exportName));
 await screen.findByText('验收报告 ZIP 已保存到本机。');expect(screen.getByText('远程状态：未验证')).toBeInTheDocument();
 expect(screen.queryByText(/已收录|已推荐|^已发布$/)).toBeNull();
});
it('requires explicit recheck after native export refuses stale workspace evidence',async()=>{
 let rechecked=false;serve(action=>{if(action==='file.exportSiteInspection')throw Error('当前品牌资料已变化，请重新核对后导出');if(action==='file.recheckSitePackage')rechecked=true;return envelope});
 render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(exportName));
 expect(await screen.findByRole('alert')).toHaveTextContent(/重新核对后导出/);expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();expect(button(exportName)).toBeDisabled();
 expect(rechecked).toBe(false);fireEvent.click(button(recheckName));await waitFor(()=>expect(button(exportName)).toBeEnabled());expect(rechecked).toBe(true);
});
it('does not show late export success in another brand session',async()=>{
 const pending=deferred<{saved:true}>();serve(action=>action==='file.exportSiteInspection'?pending.promise:envelope);
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(exportName));
 view.rerender(<SitePackageInspector brandId="brand-b" data={data}/>);await act(async()=>pending.resolve({saved:true}));
 expect(screen.queryByText('验收报告 ZIP 已保存到本机。')).toBeNull();expect(screen.queryByText('虚构网站.zip')).toBeNull();expect(button(exportName)).toBeDisabled();
});
it.each([
 ['cancelled',null,'已取消导出验收报告，原报告仍可查看。'],
 ['saved',{saved:true} as const,'验收报告 ZIP 已保存到本机。']
] as const)('announces %s export after snapshot replacement while keeping evidence outdated',async(_outcome,result,message)=>{
 const pending=deferred<{saved:true}|null>();const calls:string[]=[];
 serve(action=>{calls.push(action);return action==='file.exportSiteInspection'?pending.promise:envelope});
 const view=render(<SitePackageInspector brandId="brand-a" data={data}/>);await inspect();fireEvent.click(button(exportName));
 view.rerender(<SitePackageInspector brandId="brand-a" data={{...data}}/>);
 await act(async()=>pending.resolve(result));
 expect(await screen.findByText(message)).toBeInTheDocument();
 expect(screen.getByText('虚构网站.zip')).toBeInTheDocument();expect(screen.getByText(/报告已过时/)).toBeInTheDocument();
 expect(button(exportName)).toBeDisabled();expect(button(recheckName)).toBeEnabled();
 expect(calls).toEqual(['file.inspectSitePackage','file.exportSiteInspection']);
});
