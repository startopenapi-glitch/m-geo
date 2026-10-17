import type {Draft,Product,WorkspaceSnapshot} from '../types';
/** Presentation guard mirrors native current_draft; every outward action still revalidates natively. */
export function productHasCurrentFacts(product:Product,at=new Date()):boolean {
 const publicFacts=product.facts.filter(f=>f.visibility==='public');
 const utcDate=at.toISOString().slice(0,10);
 return Boolean(product.confirmedAt&&publicFacts.length&&publicFacts.every(f=>{
  if(!f.source.trim())return false;
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
export function draftCanExport(data:WorkspaceSnapshot,draft:Draft,brandId:string,at=new Date()):boolean {
 return Boolean(draft.reviewedAt&&draftIsCurrent(data,draft,brandId,at));
}
