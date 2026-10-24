export type InspectionVersionStatus='match'|'changed'|'unavailable';
export interface InspectionDraftVersion {id:string;packageRevision:number;currentRevision:number|null;status:InspectionVersionStatus}
export interface InspectionProductVersion {productId:string;name:string;sku:string;packageRevision:number;currentRevision:number|null;status:InspectionVersionStatus;detail:string;drafts:InspectionDraftVersion[]}
export interface InspectionCheck {code:string;status:'pass'|'blocked'|'warning';message:string;path?:string}
export interface InspectionReport {
 format:'m-geo-site-inspection-v1';brandId:string;checkedAt:string;
 source:{fileName:string;sha256:string;compressedBytes:number;uncompressedBytes:number;fileCount:number};
 packageStatus:'pass'|'blocked';checks:InspectionCheck[];previewOnly:boolean|null;baseUrl:string|null;
 products:InspectionProductVersion[];currentStatus:InspectionVersionStatus;
 remoteStatus:'not-verified';publicationStatus:'not-published';reportFingerprint:string;
}
export interface SiteInspectionEnvelope {inspectionId:string;report:InspectionReport}
