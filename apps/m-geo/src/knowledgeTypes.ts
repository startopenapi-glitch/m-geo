export type KnowledgeFormat = 'markdown' | 'text';
export type KnowledgeRisk = 'low' | 'medium' | 'high';
export type KnowledgeReviewStatus = 'pending' | 'approved' | 'rejected';
export type KnowledgeReviewDecision = Exclude<KnowledgeReviewStatus,'pending'>;

export interface KnowledgeDocumentSummary {
 id:string;
 brandId:string;
 productId:string|null;
 title:string;
 source:string;
 format:KnowledgeFormat;
 effectiveAt:string|null;
 risk:KnowledgeRisk;
 reviewStatus:KnowledgeReviewStatus;
 reviewedAt:string|null;
 revision:number;
 chunkCount:number;
 createdAt:string;
 updatedAt:string;
}

export interface KnowledgeChunk {
 id:string;
 ordinal:number;
 text:string;
 sha256:string;
}

export interface KnowledgeReview {
 id:string;
 documentId:string;
 revision:number;
 decision:KnowledgeReviewDecision;
 note:string;
 createdAt:string;
}

export interface KnowledgeDocument extends KnowledgeDocumentSummary {
 content:string;
 chunks:KnowledgeChunk[];
 reviews:KnowledgeReview[];
}

export interface KnowledgeImportRequest {
 brandId:string;
 productId:string|null;
 title:string;
 source:string;
 format:KnowledgeFormat;
 effectiveAt:string|null;
 risk:KnowledgeRisk;
 content:string;
 requestId:string;
}

export interface KnowledgeReviseRequest extends KnowledgeImportRequest {
 id:string;
 expectedRevision:number;
}

export interface KnowledgeReviewRequest {
 brandId:string;
 id:string;
 expectedRevision:number;
 decision:KnowledgeReviewDecision;
 note:string;
}

export interface KnowledgeRecallHit {
 documentId:string;
 documentRevision:number;
 documentTitle:string;
 source:string;
 productId:string|null;
 risk:KnowledgeRisk;
 effectiveAt:string|null;
 reviewedAt:string;
 chunkId:string;
 chunkOrdinal:number;
 text:string;
 sha256:string;
 score:number;
}
