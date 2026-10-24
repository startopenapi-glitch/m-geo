export type ID = string;
export type Kind = 'description' | 'faq' | 'guide' | 'comparison';
export type Platform = 'doubao' | 'yuanbao' | 'qianwen' | 'baidu';
export interface Brand { id:ID; name:string; createdAt:string }
export interface Fact { id:string; label:string; value:string; source:string; visibility:'public'|'private'; validUntil:string|null }
export interface Product { id:ID; brandId:ID; name:string; sku:string; category:string; officialUrl:string; sourceText:string; privateNotes:string; facts:Fact[]; revision:number; confirmedAt:string|null; updatedAt:string }
export interface Question { id:ID; brandId:ID; productId:ID; text:string; intent:'selection'|'comparison'|'usage'|'after_sales'; origin:'customer'|'suggestion'; source:string; branded:boolean; confirmed:boolean }
export interface Draft { id:ID; brandId:ID; productId:ID; questionId:ID|null; kind:Kind; title:string; body:string; sourceRevision:number; revision:number; generationMode:'local'|'ai'; reviewedAt:string|null; createdAt:string }
export interface Publication { id:ID; brandId:ID; draftId:ID; draftRevision:number; channel:string; url:string; publishedAt:string; note:string; contentSnapshot:string }
export interface Assessment { campaignId:string|null; repetition:number; context:string; recommendation:'unknown'|'yes'|'no'; rank:number|null; competitors:string }
export interface Observation { assessmentRevision?:number; assessmentHistory?:Record<string,unknown>[]; assessment?:Assessment; id:ID; brandId:ID; productId:ID; questionId:ID; querySnapshot:string; branded:boolean; platform:Platform; surface:'consumer'|'api'; phase:'baseline'|'followup'; modelLabel:string; searchMode:string; testedAt:string; answer:string; mentioned:boolean; accuracy:'unknown'|'correct'|'incorrect'; citedUrl:string; evidenceNote:string }
export type EvidenceMode = 'recorded' | 'simulated' | 'unspecified';
export interface Campaign { evidenceMode?:EvidenceMode; id:string; brandId:string; name:string; questions:{id:string;productId:string;text:string}[]; platforms:Platform[]; repetitions:number; modelLabel:string; searchMode:string; context:string; createdAt:string }
export interface WorkspaceSnapshot { campaigns?:Campaign[]; products:Product[]; questions:Question[]; drafts:Draft[]; publications:Publication[]; observations:Observation[]; truncated?:boolean }
export interface ModelSettings {baseUrl:string;model:string;version:number;hasKey:boolean}
export interface AiJob {id:ID;brandId:ID;productId:ID|null;kind:string;status:'queued'|'running'|'succeeded'|'failed'|'cancelled'|'interrupted';message:string;resultText:string|null;draftId:ID|null;createdAt:string}
export interface AppInfo {name:string;version:string;dataLocation:string;platform:string;releaseChannel:'internal'|'test';updateUrl:string|null}
export type Payload = Record<string, unknown>;
