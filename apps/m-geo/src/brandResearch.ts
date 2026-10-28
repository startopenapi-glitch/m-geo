import {call} from './api';
import type {Brand,Question} from './types';

export interface BrandProfile {
 aliases:string[];
 officialUrl:string;
 manufacturer:string;
 category:string;
 description:string;
 updatedAt:string;
}
export type BrandWithProfile=Brand&{profile?:BrandProfile};
export type ResearchIntent=Question['intent'];
export type ResearchSourceKind='manual'|'imported'|'suggested';
export type ResearchStatus='candidate'|'confirmed'|'archived';
export interface ResearchEntry {
 id:string;
 brandId:string;
 productId:string|null;
 question:string;
 intent:ResearchIntent;
 sourceKind:ResearchSourceKind;
 sourceLabel:string;
 sourceUrl:string;
 observedAt:string;
 clusterKey:string;
 status:ResearchStatus;
 revision:number;
 createdAt:string;
 updatedAt:string;
}
export interface BrandProfileSaveRequest {
 brandId:string;
 aliases:string[];
 officialUrl:string;
 manufacturer:string;
 category:string;
 description:string;
}
export interface ResearchSaveRequest {
 brandId:string;
 id?:string;
 expectedRevision?:number;
 productId:string|null;
 question:string;
 intent:ResearchIntent;
 sourceKind:ResearchSourceKind;
 sourceLabel:string;
 sourceUrl:string;
 observedAt:string;
 clusterKey:string;
}
export interface ResearchListFilters {
 brandId:string;
 status?:ResearchStatus;
 sourceKind?:ResearchSourceKind;
 productId?:string;
 clusterKey?:string;
 query?:string;
 observedFrom?:string;
 observedTo?:string;
 limit?:number;
}
export interface ResearchCluster {clusterKey:string;count:number;researchIds:string[]}
export interface ResearchListResult {items:ResearchEntry[];clusters:ResearchCluster[];truncated:boolean}
export interface ResearchTransitionRequest {brandId:string;id:string;expectedRevision:number}
export interface ConfirmToQuestionResult {id:string;brandId:string;research:ResearchEntry;question:Question}

const payload=<T extends object>(value:T)=>Object.fromEntries(Object.entries(value).filter(([,entry])=>entry!==undefined));
export const saveBrandProfile=(input:BrandProfileSaveRequest)=>call<BrandWithProfile>('brand.profile.save',{...input});
export const saveResearch=(input:ResearchSaveRequest)=>call<ResearchEntry>('research.save',payload(input));
export const listResearch=(input:ResearchListFilters)=>call<ResearchListResult>('research.list',payload(input));
export const archiveResearch=(input:ResearchTransitionRequest)=>call<ResearchEntry>('research.archive',{...input});
export const confirmResearch=(input:ResearchTransitionRequest)=>call<ResearchEntry>('research.confirm',{...input});
export const confirmResearchToQuestion=(input:ResearchTransitionRequest)=>call<ConfirmToQuestionResult>('research.confirmToQuestion',{...input});

/** Mirrors the deliberately conservative native duplicate key, for hints only. */
export function normalizeResearchQuestion(value:string):string{
 const normalized=value.trim().toLowerCase().replace(/[！-～]/g,character=>String.fromCharCode(character.charCodeAt(0)-0xfee0)).replace(/[^\p{L}\p{N}]/gu,'').replace(/^请问/,'');
 return normalized.replace(/[吗呢]$/,'');
}
export function researchDuplicateKey(productId:string|null,question:string):string{
 return `${productId??''}\0${normalizeResearchQuestion(question)}`;
}
export function aliasesFromText(value:string):string[]{
 const seen=new Set<string>(),aliases:string[]=[];
 for(const part of value.split(/[\n,，]/)){
  const alias=part.trim();
  if(alias&&!seen.has(alias.toLowerCase())){seen.add(alias.toLowerCase());aliases.push(alias)}
 }
 return aliases;
}
export function aliasesToText(aliases:string[]):string{return aliases.join('\n')}
export function localDateTimeToIso(value:string):string{return new Date(value).toISOString()}
export function isoToLocalDateTime(value:string):string{
 const date=new Date(value),pad=(part:number)=>String(part).padStart(2,'0');
 return `${date.getFullYear()}-${pad(date.getMonth()+1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
