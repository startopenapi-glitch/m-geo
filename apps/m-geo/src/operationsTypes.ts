import type {AttributionEvidenceMode,AttributionPreview,AttributionProduct,Currency} from './attributionTypes';
export const csvMappingKeys=['kind','id','sessionId','sku','occurredAt','source','sourceEvidence','status','amount','orderId'] as const;
export type CsvMappingKey=typeof csvMappingKeys[number];
export type CsvMapping=Record<CsvMappingKey,string|null>;
export type CsvDelimiter='comma'|'tab';
export interface CsvMetadata {evidenceMode:AttributionEvidenceMode;sourceLabel:string;coverageStart:string;coverageEnd:string;currency:Currency;amountUnit:'minor'|'major'}
export interface CsvTemplate {csv:string;delimiter:CsvDelimiter;mapping:CsvMapping;metadata:CsvMetadata;name:string}
export interface CsvPrepared {content:string;preview:AttributionPreview;mapping:CsvMapping;metadata:CsvMetadata}
export interface CsvIssue {scope:'csv'|'mapping'|'metadata'|'row'|'reference'|'catalog'|'dataset';record:number|null;field:string|null;code:string;message:string;relatedRecords:number[]}
export interface CsvDiagnostic {format:'m-geo-csv-diagnostics-v1';stage:'structure'|'mapped';status:'ready'|'blocked';headers:string[];rowCount:number|null;issues:CsvIssue[];totalIssues:number;issuesTruncated:boolean;prepared:CsvPrepared|null}
export interface StagedEntry {name:string;content:string;expectedHash:string;preview:AttributionPreview}
export type LocalJobStatus='paused'|'ready'|'completed'|'cancelled';
export interface LocalJobEntry {id:string;name:string;content:string;expectedHash:string;products:AttributionProduct[];status:'pending'|'succeeded'|'failed'|'cancelled';attempts:number;datasetId:string|null;alreadyImported:boolean;error:string|null}
export interface LocalJob {id:string;brandId:string;name:string;contentHash:string;createdAt:string;updatedAt:string;revision:number;status:LocalJobStatus;entries:LocalJobEntry[]}
export interface LocalJobSummary {id:string;name:string;status:LocalJobStatus;revision:number;createdAt:string;updatedAt:string;total:number;succeeded:number;failed:number;pending:number;cancelled:number}
export interface LocalJobCreated extends LocalJob {alreadyExists:boolean}
