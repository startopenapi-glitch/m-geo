import type {Campaign,EvidenceMode,Kind,Platform} from './types';
export type OptimizationSection='products'|'content'|'visibility';
export type OptimizationStatus='open'|'in_progress'|'done'|'dismissed';
export interface OptimizationPhaseMetrics {samples:number;assessed:number;recommended:number;unknown:number;ranked:number;top3:number;cited:number;incorrect:number}
export interface OptimizationMetrics {expectedPerPhase:number;baseline:OptimizationPhaseMetrics;followup:OptimizationPhaseMetrics;matchedPairs:number;matchedAssessedPairs:number;matchedBaselineRecommended:number;matchedFollowupRecommended:number;deltaPercentagePoints:number|null}
export interface OptimizationFinding {key:string;code:string;productId:string|null;priority:'high'|'normal';title:string;detail:string;section:OptimizationSection;evidenceIds:string[]}
export interface OptimizationDiagnosis {format:'m-geo-optimization-diagnosis-v1';brandId:string;campaign:Campaign;evidenceMode:EvidenceMode;phase:'baseline'|'followup';asOf:string;products:{id:string;name:string;sku:string;revision:number;confirmed:boolean;facts:{id:string;hasSource:boolean;validUntil:string|null;unknown:boolean}[];drafts:{id:string;kind:Kind;revision:number;sourceRevision:number;reviewed:boolean}[]}[];observations:{id:string;productId:string;questionId:string;platform:Platform;phase:'baseline'|'followup';repetition:number;assessmentRevision:number;mentioned:boolean;accuracy:'unknown'|'correct'|'incorrect';recommendation:'unknown'|'yes'|'no';rank:number|null;cited:boolean}[];metrics:OptimizationMetrics;findings:OptimizationFinding[];fingerprint:string}
export interface OptimizationCheckpoint {id:string;capturedAt:string;diagnosis:OptimizationDiagnosis}
export interface OptimizationAction extends OptimizationFinding {status:OptimizationStatus;note:string;updatedAt:string}
export interface OptimizationPlan {format:'m-geo-optimization-plan-v1';id:string;brandId:string;requestId:string;createFingerprint:string;name:string;campaignId:string;evidenceMode:EvidenceMode;revision:number;createdAt:string;updatedAt:string;checkpoints:OptimizationCheckpoint[];actions:OptimizationAction[]}
export interface OptimizationPlanSummary {id:string;requestId:string;name:string;campaignId:string;evidenceMode:EvidenceMode;revision:number;createdAt:string;updatedAt:string;checkpointCount:number;actionCount:number;doneCount:number}
export interface OptimizationCreatePayload {brandId:string;name:string;campaignId:string;diagnosisFingerprint:string;requestId:string}
export type OptimizationCreated=OptimizationPlan & {alreadyExists:boolean};
