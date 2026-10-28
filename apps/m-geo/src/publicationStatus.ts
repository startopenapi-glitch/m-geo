import {productHasCurrentFacts} from './components/draftEligibility';
import type {Publication,WorkspaceSnapshot} from './types';

export type PublicationVersionStatus='current'|'newer_draft'|'product_changed'|'historical';
export type PublicationLifecycleStatus='registered'|'pending_readback'|'online'|'content_changed'|'unreachable'|'withdrawing'|'withdrawn'|'withdrawal_unverified';
export type PublicationStatus=PublicationVersionStatus|PublicationLifecycleStatus;
export type PublicationWithdrawalStatus='withdrawing'|'withdrawn'|'withdrawal_unverified';
export interface PublicationReadback {fetchedAt:string;httpStatus:number|null;finalUrl:string;title:string;bodySha256:string|null;draftRevision:number;matchedContent:boolean;errorCategory:string|null;transport:'https'|'http'}
export interface PublicationWithdrawalEntry {status:PublicationWithdrawalStatus;reason:string;recordedAt:string}
export interface PublicationWithdrawal {status:PublicationWithdrawalStatus;reason:string;requestedAt:string;updatedAt:string;history?:PublicationWithdrawalEntry[]}
export type PublicationWithLifecycle=Publication&{lifecycle?:PublicationLifecycleStatus;readbacks?:PublicationReadback[];withdrawal?:PublicationWithdrawal};

export const statusLabels:Record<PublicationStatus,string>={
 registered:'已登记',
 pending_readback:'待回读',
 online:'在线',
 content_changed:'内容变化',
 unreachable:'不可达',
 withdrawing:'撤回中',
 withdrawn:'已撤回（人工登记）',
 withdrawal_unverified:'撤回待核查',
 current:'当前版本',
 newer_draft:'有新稿件',
 product_changed:'商品已变更／需复核',
 historical:'历史记录',
};
export const readbackErrorLabels:Record<string,string>={
 dns:'域名解析失败或目标不是公开地址',
 timeout:'读取超时',
 transport:'连接或传输失败',
 http_status:'公开页面返回非成功状态',
 response_too_large:'响应超过 512 KB',
 invalid_response:'响应或重定向无效',
};

/** Derived from retained versions; this never checks the public URL or platform. */
export function publicationStatus(data:WorkspaceSnapshot,publication:Publication,brandId:string,at=new Date()):PublicationVersionStatus{
 if(publication.brandId!==brandId)return 'historical';
 const draft=data.drafts.find(item=>item.id===publication.draftId&&item.brandId===brandId);
 if(!draft||publication.draftRevision>draft.revision)return 'historical';
 const product=data.products.find(item=>item.id===draft.productId&&item.brandId===brandId);
 if(!product)return 'historical';
 if(product.revision!==draft.sourceRevision||!productHasCurrentFacts(product,at))return 'product_changed';
 if(draft.revision>publication.draftRevision)return 'newer_draft';
 return 'current';
}

/** Withdrawal always wins; an in-flight readback is transient and never persisted. */
export function publicationLifecycle(publication:Publication,pending=false):PublicationLifecycleStatus{
 const p=publication as PublicationWithLifecycle;
 if(p.withdrawal?.status)return p.withdrawal.status;
 if(pending)return 'pending_readback';
 if(p.lifecycle)return p.lifecycle;
 return 'registered';
}

/** Legacy records keep their version filter; lifecycle-bearing records use readback state. */
export function publicationDisplayStatus(data:WorkspaceSnapshot,publication:Publication,brandId:string,pending=false,at=new Date()):PublicationStatus{
 const p=publication as PublicationWithLifecycle;
 if(pending||p.lifecycle||p.readbacks||p.withdrawal)return publicationLifecycle(publication,pending);
 return publicationStatus(data,publication,brandId,at);
}

export function latestReadback(publication:Publication):PublicationReadback|undefined{
 return (publication as PublicationWithLifecycle).readbacks?.[0];
}
export function withdrawalOf(publication:Publication):PublicationWithdrawal|undefined{
 return (publication as PublicationWithLifecycle).withdrawal;
}
export function readbackCount(publication:Publication):number{
 return (publication as PublicationWithLifecycle).readbacks?.length||0;
}
