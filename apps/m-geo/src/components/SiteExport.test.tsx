import {render,screen,fireEvent} from '@testing-library/react';
import {it,expect,vi} from 'vitest';
import SiteExport from './SiteExport';
import {call} from '../api';
import type {PageProps} from '../App';
vi.mock('../api',()=>({call:vi.fn()}));
const props={brand:{id:'b',name:'虚构',createdAt:''},data:{products:[{id:'p',brandId:'b',name:'虚构',sku:'X',facts:[{id:'f',label:'容量',value:'1',source:'虚构依据',visibility:'public',validUntil:null}],revision:1,confirmedAt:'2026-10-23',updatedAt:''}],questions:[],observations:[],publications:[],drafts:[{id:'d',brandId:'b',productId:'p',questionId:null,kind:'faq',title:'虚构 FAQ',body:'正文',sourceRevision:1,revision:2,reviewedAt:'2026-10-23',createdAt:''}]},refresh:async()=>{}} as unknown as PageProps;
it('invalidates readiness after input edits and a changed workspace',async()=>{
 vi.mocked(call).mockResolvedValue({previewOnly:true,productCount:1,files:[],readiness:{warnings:6,blockers:1,checks:[{code:'domain',status:'blocked',message:'缺少官网地址'}]}});
 const view=render(<SiteExport {...props}/>);fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByText('检查网站内容包'));await screen.findByLabelText('本地完整性检查');
 fireEvent.change(screen.getByRole('textbox'),{target:{value:'https://example.com/'}});expect(screen.queryByLabelText('本地完整性检查')).not.toBeInTheDocument();
 fireEvent.click(screen.getByText('检查网站内容包'));await screen.findByLabelText('本地完整性检查');
 view.rerender(<SiteExport {...props} data={{...props.data,products:props.data.products.map(p=>({...p,revision:2}))}}/>);expect(screen.queryByLabelText('本地完整性检查')).not.toBeInTheDocument();
});
it('does not resurrect a result arriving after draft selection changes',async()=>{
 let resolve!:(v:unknown)=>void;vi.mocked(call).mockReturnValueOnce(new Promise(r=>{resolve=r}));
 render(<SiteExport {...props}/>);fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByText('检查网站内容包'));fireEvent.click(screen.getByRole('checkbox'));
 resolve({previewOnly:true,productCount:1,files:[],readiness:{warnings:0,blockers:1,checks:[]}});
 await new Promise(r=>setTimeout(r,0));expect(screen.queryByLabelText('本地完整性检查')).not.toBeInTheDocument();
});
it('labels readiness fields, statuses and products without internal identifiers',async()=>{
 const checks=[{code:'domain',status:'blocked',message:'缺少官网地址'},...['brand','image','audience','scenario','limits','afterSales','offer'].map(code=>({code,status:'pass',message:'已提供公开资料',productId:'p'})),{code:'image',status:'warning',message:'缺少公开商品图片',productId:'missing-product-uuid'}];
 vi.mocked(call).mockResolvedValue({previewOnly:true,productCount:1,files:[],readiness:{warnings:1,blockers:1,checks}});
 render(<SiteExport {...props}/>);fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByText('检查网站内容包'));
 const checklist=await screen.findByLabelText('本地完整性检查');expect(checklist).toHaveTextContent('待配置 · 官网地址：缺少官网地址');
 for(const field of ['商品品牌','商品图片','适用人群','使用场景','使用限制','售后说明','购买信息']) expect(checklist).toHaveTextContent(`通过 · 虚构 · ${field}：已提供公开资料`);
 expect(checklist).toHaveTextContent('待补充 · 历史商品 · 商品图片：缺少公开商品图片');expect(checklist).not.toHaveTextContent('missing-product-uuid');expect(checklist).not.toHaveTextContent('pass');
});
it('runs local semantic audit and exports the same selection with remote checks pending',async()=>{
 vi.mocked(call).mockResolvedValue({checks:[{code:'hashes',status:'pass',message:'清单匹配'}],summary:{pass:1,warning:0,blocked:0},remoteChecks:[{check:'HTTP 状态',status:'pending'}]});
 render(<SiteExport {...props}/>);
 fireEvent.click(screen.getByRole('checkbox'));
 fireEvent.click(screen.getByText('运行本地页面检查'));
 expect(await screen.findByLabelText('本地页面检查')).toHaveTextContent('通过：清单匹配');
 expect(screen.getByLabelText('待人工部署检查')).toHaveTextContent('待检查：HTTP 状态');
 expect(call).toHaveBeenCalledWith('site.audit',{brandId:'b',baseUrl:'',drafts:[{id:'d',expectedRevision:2}]});
 fireEvent.change(screen.getByRole('textbox'),{target:{value:'https://fictional.example/'}});
 expect(screen.queryByLabelText('本地页面检查')).not.toBeInTheDocument();
 fireEvent.click(screen.getByText('导出部署检查单'));
 expect(call).toHaveBeenCalledWith('file.exportSiteAudit',{brandId:'b',baseUrl:'https://fictional.example/',drafts:[{id:'d',expectedRevision:2}]});
});
it('ignores an audit result arriving after selection changes',async()=>{
 let resolve!:(v:unknown)=>void;
 vi.mocked(call).mockReturnValueOnce(new Promise(r=>{resolve=r}));
 render(<SiteExport {...props}/>);
 fireEvent.click(screen.getByRole('checkbox'));
 fireEvent.click(screen.getByText('运行本地页面检查'));
 fireEvent.click(screen.getByRole('checkbox'));
 resolve({checks:[],summary:{pass:1,warning:0,blocked:0},remoteChecks:[]});
 await new Promise(r=>setTimeout(r,0));
 expect(screen.queryByLabelText('本地页面检查')).not.toBeInTheDocument();
});
