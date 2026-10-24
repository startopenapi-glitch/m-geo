/** Aggregate history result. Per-item identifiers and receipt messages are paged separately. */
interface ChannelInspectionReportBase {
  format:'m-geo-channel-inspection-v1';
  profile:'mgeo-catalog-v1'|'google-merchant-basic-v1';
  source:{fileName:string;sha256:string;compressedBytes:number;uncompressedBytes:number;fileCount:4};
  brandId:string;
  packageId:string;
  status:'passed'|'blocked';
  counts:{selected:number;baselineSelected:number;rows:number;ready:number;blocked:number};
  checks:{code:'frozen_evidence'|'report'|'notice';status:'passed'|'failed';message:string}[];
  caveat:string;
  reportFingerprint:string;
}
export type ChannelInspectionReport = ChannelInspectionReportBase & (
  {operation:'comparison';baselinePackageId:string;summary:{added:number;removed:number;changed:number;unchanged:number}} |
  {operation:'simulation'|'receipt-check';baselinePackageId:null;summary:{accepted:number;rejected:number;unknown:number;missing:number}}
);
export interface ChannelInspectionSelection {inspectionId:string;report:ChannelInspectionReport}
export interface ChannelComparisonRow {sku:string;change:'added'|'removed'|'changed'|'unchanged';fields:string[]}
export interface ChannelReceiptRow {sku:string;rowHash:string;status:'accepted'|'rejected'|'unknown'|'missing';code:string;message:string}
/** The native page has no operation field: the selection's report determines the table shape. */
export interface ChannelInspectionPage {
  inspectionId:string;
  page:number;
  pageSize:20;
  total:number;
  rows:(ChannelComparisonRow|ChannelReceiptRow)[];
}
