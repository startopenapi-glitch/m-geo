import type {Campaign,EvidenceMode,Observation,Platform} from './types';
export interface TransitionMetrics {
 total:number;baselineSamples:number;followupSamples:number;paired:number;assessedPairs:number;
 gained:number;lost:number;retainedYes:number;retainedNo:number;unknownPair:number;
 baselineOnly:number;followupOnly:number;missingBoth:number;baselineRecommended:number;
 followupRecommended:number;deltaPercentagePoints:number|null;
}
export type Transition='gained'|'lost'|'retained_yes'|'retained_no'|'unknown_pair'|'baseline_only'|'followup_only'|'missing_both';
export interface TransitionSide {observationId:string;assessmentRevision:number;recommendation:'yes'|'no'|'unknown';rank:number|null;accuracy:Observation['accuracy'];mentioned:boolean;cited:boolean}
export interface TransitionPair {productId:string;questionId:string;platform:Platform;repetition:number;transition:Transition;baseline:TransitionSide|null;followup:TransitionSide|null}
export interface TransitionProduct {productId:string;name:string;sku:string;metrics:TransitionMetrics}
export interface TransitionQuestion {productId:string;questionId:string;questionText:string;metrics:TransitionMetrics}
export interface TransitionPlatform {platform:Platform;metrics:TransitionMetrics}
export interface TransitionReport {
 format:'m-geo-recommendation-transitions-v1';brandId:string;campaign:Campaign;evidenceMode:EvidenceMode;
 evidenceFingerprint:string;summary:TransitionMetrics;products:TransitionProduct[];questions:TransitionQuestion[];
 platforms:TransitionPlatform[];rows:TransitionPair[];caveat:string;reportFingerprint:string;
}
export interface TransitionEvidence {reportFingerprint:string;row:TransitionPair;baseline:Observation|null;followup:Observation|null}
