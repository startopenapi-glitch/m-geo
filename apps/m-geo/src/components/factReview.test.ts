import {afterEach,describe,expect,it,vi} from 'vitest';
import type {Fact} from '../types';
import {changeFact,createPendingFact,factIsApproved,factReviewStatus} from './factReview';

afterEach(()=>vi.useRealTimers());

describe('fact review registry',()=>{
 it('creates new facts as pending and treats only missing status as legacy approved',()=>{
  vi.useFakeTimers();vi.setSystemTime(new Date('2026-10-24T08:00:00Z'));
  const fact=createPendingFact('fictional-fact');
  expect(fact).toEqual({id:'fictional-fact',label:'',value:'',source:'',visibility:'public',validUntil:null,reviewStatus:'pending',reviewedAt:null,updatedAt:'2026-10-24T08:00:00.000Z'});
  expect(factIsApproved(fact)).toBe(false);
  expect(factReviewStatus({...fact,reviewStatus:undefined})).toBe('approved');
 });
 it.each(['label','value','source','visibility','validUntil'] as const)('resets approval when %s changes',field=>{
  const approved:Fact={...createPendingFact('f'),label:'容量',value:'500mL',source:'虚构规格表',reviewStatus:'approved',reviewedAt:'2026-10-24T08:00:00Z'};
  const changed=changeFact(approved,field,field==='validUntil'?'2099-01-01':'changed');
  expect(changed.reviewStatus).toBe('pending');expect(changed.reviewedAt).toBeNull();
 });
 it('allows an explicit review decision without changing the fact content',()=>{
  const pending=createPendingFact('f');const approved=changeFact(pending,'reviewStatus','approved');
  expect(approved).toMatchObject({reviewStatus:'approved',reviewedAt:null,updatedAt:pending.updatedAt});
 });
});
