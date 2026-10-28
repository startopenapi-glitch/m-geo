import type {Draft,Product,WorkspaceSnapshot} from '../types';
import {factIsApproved} from './factReview';
/** Presentation guard mirrors native current_draft; every outward action still revalidates natively. */
export function productHasCurrentFacts(product:Product,at=new Date()):boolean {
 const publicFacts=product.facts.filter(f=>f.visibility==='public');
 const utcDate=at.toISOString().slice(0,10);
 return Boolean(product.confirmedAt&&publicFacts.length&&publicFacts.every(f=>{
  if(!factIsApproved(f)||!f.source.trim())return false;
  if(f.validUntil===null)return true;
  if(!/^\d{4}-\d{2}-\d{2}$/.test(f.validUntil))return false;
  const parsed=new Date(`${f.validUntil}T00:00:00Z`);
  return Number.isFinite(parsed.getTime())&&parsed.toISOString().slice(0,10)===f.validUntil&&f.validUntil>=utcDate;
 }));
}
export function draftIsCurrent(data:WorkspaceSnapshot,draft:Draft,brandId:string,at=new Date()):boolean {
 if(draft.brandId!==brandId)return false;
 const product=data.products.find(p=>p.id===draft.productId&&p.brandId===brandId);
 if(!product||product.revision!==draft.sourceRevision||!productHasCurrentFacts(product,at))return false;
 return !draft.questionId||data.questions.some(q=>q.id===draft.questionId&&q.brandId===brandId&&q.productId===draft.productId&&q.confirmed);
}
export function draftClaimsHaveEvidence(data:WorkspaceSnapshot,draft:Draft,brandId:string):boolean {
 if(!draft.claims?.length)return false;
 const product=data.products.find(p=>p.id===draft.productId&&p.brandId===brandId);
 if(!product)return false;
 return draft.claims.every(claim=>claim.text.trim()&&draft.body.includes(claim.text)&&product.facts.some(f=>f.id===claim.factId&&f.visibility==='public'&&factIsApproved(f)&&f.source.trim()));
}
export function draftAssetsHaveEvidence(draft:Draft):boolean {
 const ids=draft.assetIds??[];
 if(!ids.length)return !draft.assetSnapshots?.length;
 if(ids.length!==draft.assetSnapshots?.length)return false;
 return ids.every((id,index)=>{const snapshot=draft.assetSnapshots?.[index];return snapshot?.assetId===id&&snapshot.text.trim()&&(draft.title.includes(snapshot.text)||draft.body.includes(snapshot.text))});
}
export function draftCanExport(data:WorkspaceSnapshot,draft:Draft,brandId:string,at=new Date()):boolean {
 return Boolean(draft.reviewedAt&&draftIsCurrent(data,draft,brandId,at)&&draftClaimsHaveEvidence(data,draft,brandId)&&draftAssetsHaveEvidence(draft));
}
