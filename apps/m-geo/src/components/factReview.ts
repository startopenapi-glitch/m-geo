import type {Fact,FactReviewStatus} from '../types';

export function factReviewStatus(fact:Fact):FactReviewStatus {
 return fact.reviewStatus??'approved';
}
export function factIsApproved(fact:Fact):boolean {
 return factReviewStatus(fact)==='approved';
}
export function createPendingFact(id:string):Fact {
 return {id,label:'',value:'',source:'',visibility:'public',validUntil:null,reviewStatus:'pending',reviewedAt:null,updatedAt:new Date().toISOString()};
}
export function changeFact(fact:Fact,field:keyof Fact,value:string):Fact {
 if(field==='reviewStatus')return {...fact,reviewStatus:value as FactReviewStatus,reviewedAt:null};
 const next={...fact,[field]:field==='validUntil'?(value||null):value};
 if(['label','value','source','visibility','validUntil'].includes(field)){
  return {...next,reviewStatus:'pending',reviewedAt:null,updatedAt:new Date().toISOString()};
 }
 return next;
}
