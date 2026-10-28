import type {ChannelId} from './channels';
import type {ContentAsset} from './assetTypes';

export type ID = string;
export type Kind = 'description' | 'faq' | 'guide' | 'comparison';
export type Platform = 'doubao' | 'yuanbao' | 'qianwen' | 'baidu';
export interface BrandProfile {aliases:string[];officialUrl:string;manufacturer:string;category:string;description:string;updatedAt:string}
export interface Brand { id:ID; name:string; createdAt:string; profile?:BrandProfile }
export type FactReviewStatus = 'pending'|'approved'|'rejected';
// Optional review fields keep pre-0.16 records readable; new saves always write them.
export interface Fact { id:string; label:string; value:string; source:string; visibility:'public'|'private'; validUntil:string|null; reviewStatus?:FactReviewStatus; reviewedAt?:string|null; updatedAt?:string }
export interface Product { id:ID; brandId:ID; name:string; sku:string; category:string; officialUrl:string; sourceText:string; privateNotes:string; facts:Fact[]; revision:number; confirmedAt:string|null; updatedAt:string }
export interface Question { id:ID; brandId:ID; productId:ID; text:string; intent:'selection'|'comparison'|'usage'|'after_sales'; origin:'customer'|'suggestion'; source:string; branded:boolean; confirmed:boolean }
export interface ClaimEvidenceSnapshot { snapshotVersion:1; factId:string; label:string; value:string; source:string; validUntil:string|null; factReviewedAt:string|null; factUpdatedAt:string|null }
export interface DraftClaim { id:string; text:string; factId:string; evidence?:ClaimEvidenceSnapshot }
export interface DraftAssetSnapshot { snapshotVersion:1; assetId:ID; kind:'keyword'|'title'; text:string; source:string; productId:ID|null; assetRevision:number; assetReviewedAt:string; assetUpdatedAt:string }
export interface Draft { id:ID; brandId:ID; productId:ID; questionId:ID|null; kind:Kind; channel?:ChannelId|null; channelTemplateVersion?:number|null; title:string; body:string; claims?:DraftClaim[]; assetIds?:ID[]; assetSnapshots?:DraftAssetSnapshot[]; sourceRevision:number; revision:number; generationMode:'local'|'ai'; reviewedAt:string|null; createdAt:string }
export interface PublicationReadback {fetchedAt:string;httpStatus:number|null;finalUrl:string;title:string;bodySha256:string|null;draftRevision:number;matchedContent:boolean;errorCategory:string|null;transport:'https'|'http'}
export interface Publication { id:ID; brandId:ID; draftId:ID; draftRevision:number; channel:string; url:string; publishedAt:string; note:string; contentSnapshot:string; lifecycle?:'registered'|'pending_readback'|'online'|'content_changed'|'unreachable'|'withdrawing'|'withdrawn'|'withdrawal_unverified'; readbacks?:PublicationReadback[]; withdrawal?:{status:'withdrawing'|'withdrawn'|'withdrawal_unverified';reason:string;requestedAt:string;updatedAt:string;history?:{status:'withdrawing'|'withdrawn'|'withdrawal_unverified';reason:string;recordedAt:string}[]} }
export interface Assessment { campaignId:string|null; repetition:number; context:string; recommendation:'unknown'|'yes'|'no'; rank:number|null; competitors:string }
export interface Observation { assessmentRevision?:number; assessmentHistory?:Record<string,unknown>[]; assessment?:Assessment; id:ID; brandId:ID; productId:ID; questionId:ID; querySnapshot:string; branded:boolean; platform:Platform; surface:'consumer'|'api'; phase:'baseline'|'followup'; modelLabel:string; searchMode:string; testedAt:string; answer:string; mentioned:boolean; accuracy:'unknown'|'correct'|'incorrect'; citedUrl:string; evidenceNote:string; evidenceMode?:'recorded'; sourceKind?:'real_api'; transport?:'real_api'; responseModel?:string; providerRequestId?:string|null; citations?:string[]; preparationHash?:string; resultIdentity?:string }
export type EvidenceMode = 'recorded' | 'simulated' | 'unspecified';
export interface Campaign { evidenceMode?:EvidenceMode; id:string; brandId:string; name:string; questions:{id:string;productId:string;text:string}[]; platforms:Platform[]; repetitions:number; modelLabel:string; searchMode:string; context:string; createdAt:string }
export interface WorkspaceSnapshot { campaigns?:Campaign[]; products:Product[]; questions:Question[]; drafts:Draft[]; publications:Publication[]; observations:Observation[]; assets?:ContentAsset[]; truncated?:boolean }
export interface ModelSettings {baseUrl:string;model:string;version:number;hasKey:boolean}
export interface AiJob {id:ID;brandId:ID;productId:ID|null;kind:string;status:'queued'|'running'|'succeeded'|'failed'|'cancelled'|'interrupted';message:string;resultText:string|null;draftId:ID|null;createdAt:string}
export interface AppInfo {name:string;version:string;dataLocation:string;platform:string;releaseChannel:'internal'|'test';updateUrl:string|null}
export type Payload = Record<string, unknown>;
