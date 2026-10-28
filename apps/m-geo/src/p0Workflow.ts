import type {Brand,Product} from './types';
export type P0Mode='natural_discovery'|'informed_qa';
export type P0Scenario='success'|'rate_limit_once'|'publish_timeout'|'readback_mismatch'|'robots_blocked';
export type P0Status='paused'|'running'|'awaiting_review'|'awaiting_publish_approval'|'awaiting_reconcile'|'retry_wait'|'budget_blocked'|'completed'|'failed'|'cancelled';
export type P0NodeKind='baseline'|'generate'|'audit'|'review'|'publish'|'readback'|'followup';
export interface P0Audit {scope:'fixture_site';checks:{id:string;label:string;status:'pass'|'warning'|'fail';detail:string}[];blocking:number;warning:number;passed:number;networkRequests:0}
export interface P0Task {id:string;brandId:string;productId:string;productRevision:number;requestId:string;name:string;revision:number;mode:P0Mode;question:string;targetUrl:string;scenario:P0Scenario;transport:'fixture';evidenceMode:'simulated';status:P0Status;createdAt:string;updatedAt:string;draftId:string|null;approval:Record<string,unknown>|null;error:string|null;budget:{maxUnits:number;maxCalls:number;usedUnits:number;usedCalls:number};nodes:{id:string;kind:P0NodeKind;status:'pending'|'completed'|'blocked'|'failed';result:Record<string,unknown>|null;error:string|null}[];ledger:Record<string,unknown>[];remote:Record<string,unknown>|null;realEffects:{networkRequests:0;modelCalls:0;publications:0}}
export interface P0Create {brandId:string;requestId:string;name:string;productId:string;question:string;mode:P0Mode;targetUrl:string;scenario:P0Scenario;maxUnits:number;maxCalls:number;transport:'fixture';evidenceMode:'simulated'}
export interface P0Policy {revision:number;dailyMaxUnits:number;dailyMaxCalls:number;halted:boolean;day:string;usedUnits:number;usedCalls:number}
export interface P0Preview {mode:P0Mode;question:string;contextIncluded:boolean;transport:'fixture';evidenceMode:'simulated';request:{prompt:string};modeLabel:string;context:Record<string,unknown>|null;requestHash:string;realEffects:{networkRequests:0;modelCalls:0;publications:0}}
export const p0StatusLabel:Record<P0Status,string>={paused:'已暂停',running:'运行中',awaiting_review:'等待人工审核',awaiting_publish_approval:'等待模拟发布批准',awaiting_reconcile:'等待对账',retry_wait:'等待显式重试',budget_blocked:'模拟额度不足',completed:'已完成',failed:'失败',cancelled:'已取消'};
export const p0NodeLabel:Record<P0NodeKind,string>={baseline:'模拟基线',generate:'生成候选稿',audit:'夹具站点审计',review:'人工内容审核',publish:'模拟 CMS 发布',readback:'模拟 CMS 回读',followup:'模拟复测'};
export const defaultP0Question='选购这类商品应关注哪些指标？';
export const defaultP0Target='https://shop.example.invalid/products/demo';
export function validateP0Create(brand:Brand,input:{name:string;product:Product|undefined;question:string;mode:P0Mode;targetUrl:string;maxUnits:number;maxCalls:number}):string{
 const {name,product,question,mode,targetUrl,maxUnits,maxCalls}=input;
 if(!product||product.brandId!==brand.id||!product.confirmedAt)return '请选择当前品牌已确认商品';
 if(!name.trim()||[...name.trim()].length>100)return '任务名称须为 1–100 字';
 if(!question.trim()||[...question.trim()].length>500)return '请输入 1–500 字的单个问题';
 const hosts=[brand.profile?.officialUrl,product.officialUrl].flatMap(value=>{try{return value?[new URL(value).hostname]:[]}catch{return []}});
 if(mode==='natural_discovery'&&[brand.name,...brand.profile?.aliases??[],product.name,product.sku,...hosts].some(identity=>identity.trim()&&question.toLocaleLowerCase().includes(identity.trim().toLocaleLowerCase())))return '自然发现问题不能包含品牌、别名、完整商品名、SKU 或官网域名';
 try{const url=new URL(targetUrl);if(url.protocol!=='https:'||!url.hostname.endsWith('.invalid')||url.username||url.password||url.port||url.hash)throw Error()}catch{return '模拟目标须为无凭据的 HTTPS .invalid URL'}
 if([maxUnits,maxCalls].some(value=>!Number.isInteger(value)||value<1||value>100))return '单任务模拟额度和模拟调用须为 1–100 的整数';
 return '';
}
export function p0SiteFixture(url:string,scenario:'success'|'robots_blocked'|'hidden_text'){
 const escape=(value:string)=>value.replace(/[&<>"']/g,character=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[character]!));
 const target=escape(url),title='本地模拟页面',body='本地模拟正文',description='Local simulated article data; not real evidence.';
 const data=JSON.stringify({'@context':'https://schema.org','@type':'Article',headline:title,description,url,articleBody:body}).replace(/</g,'\\u003c').replace(/>/g,'\\u003e').replace(/&/g,'\\u0026');
 return {url,finalUrl:url,statusCode:200,headers:{contentType:'text/html; charset=utf-8',xRobotsTag:''},html:`<!doctype html><html><head><title>${title}</title><meta name="description" content="${description}"><link rel="canonical" href="${target}"><script type="application/ld+json">${data}</script></head><body><p>${description}</p><h1>${title}</h1>${scenario==='hidden_text'?'<!--本地模拟正文--><script type="application/json">本地模拟正文</script>':'<main><p>本地模拟正文</p></main>'}</body></html>`,robotsTxt:scenario==='robots_blocked'?'User-agent: *\nDisallow: /':'User-agent: *\nAllow: /',sitemapXml:`<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>${target}</loc></url></urlset>`,expectedText:body,transport:'fixture' as const};
}
