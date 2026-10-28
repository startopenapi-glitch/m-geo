export type AssetKind = 'keyword' | 'title';
export type AssetReviewStatus = 'pending' | 'approved' | 'rejected';
export type AssetReviewDecision = Exclude<AssetReviewStatus,'pending'>;
export type AssetHistoryAction = 'created' | 'revised' | 'archived' | 'restored' | AssetReviewDecision;

export interface AssetRevision {
 revision:number;
 action:AssetHistoryAction;
 text:string;
 source:string;
 productId:string|null;
 reviewStatus:AssetReviewStatus;
 reviewNote:string;
 reviewedAt:string|null;
 changedAt:string;
}

export interface ContentAsset {
 id:string;
 brandId:string;
 kind:AssetKind;
 productId:string|null;
 text:string;
 source:string;
 reviewStatus:AssetReviewStatus;
 reviewNote:string;
 reviewedAt:string|null;
 archivedAt?:string|null;
 archiveNote?:string;
 revision:number;
 createdAt:string;
 updatedAt:string;
 history:AssetRevision[];
}

export interface AssetCreateRequest {
 brandId:string;
 kind:AssetKind;
 text:string;
 source:string;
 productId:string|null;
 requestId:string;
}

export interface AssetReviseRequest {
 brandId:string;
 id:string;
 expectedRevision:number;
 text:string;
 source:string;
 productId:string|null;
}

export interface AssetReviewRequest {
 brandId:string;
 id:string;
 expectedRevision:number;
 decision:AssetReviewDecision;
 note:string;
}

export interface AssetArchiveRequest {
 brandId:string;
 id:string;
 expectedRevision:number;
 archived:boolean;
 note:string;
}
