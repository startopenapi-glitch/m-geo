import type {Assessment, EvidenceMode, Observation, Platform} from './types';

export interface InspectionStats {
  samples:number; assessed:number; recommended:number; unknown:number;
  recommendationRate:number|null; ranked:number; top3:number;
}
export interface InspectionPlatform {
  platform:Platform; expectedPerPhase:number; baseline:InspectionStats; followup:InspectionStats;
  matchedPairs:number; matchedAssessedPairs:number; matchedBaselineRecommended:number;
  matchedFollowupRecommended:number; unmatchedSamples:number; deltaPercentagePoints:number|null;
}
export interface ExperimentInspectionReport {
  format:'m-geo-experiment-inspection-v1';
  source:{fileName:string; sha256:string; compressedBytes:number; uncompressedBytes:number; fileCount:number};
  profile:'legacy-v1'|'current-v1'; brandId:string; campaignId:string;
  evidenceMode:EvidenceMode; evidenceLabel:string; status:'passed'|'blocked';
  counts:{observations:number; expectedPerPhase:number; baseline:number; followup:number; missing:number; reviewedObservations:number; historyEntries:number};
  platforms:InspectionPlatform[];
  checks:{code:'evidence_schema'|'assessment_history'|'summary'|'csv'|'markdown'; status:'passed'|'failed'; message:string}[];
  caveat:string; reportFingerprint:string;
}
export interface InspectionAnnotation {
  mentioned:boolean; accuracy:Observation['accuracy']; citedUrl:string; evidenceNote:string; assessment:Assessment;
}
export interface InspectionHistoryEntry extends InspectionAnnotation {revision:number; supersededAt:string}
/** Pristine frozen records omit both history fields; do not invent a recorded correction. */
export interface InspectedObservation extends Omit<Observation,'assessment'|'assessmentHistory'> {
  assessment:Assessment; assessmentHistory?:InspectionHistoryEntry[];
}
export interface ExperimentInspectionSelection {inspectionId:string; report:ExperimentInspectionReport}
export interface ExperimentInspectionPage {inspectionId:string; page:number; pageSize:20; total:number; observations:InspectedObservation[]}
