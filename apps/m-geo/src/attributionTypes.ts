export type AttributionEvidenceMode = 'simulated' | 'recorded';
export type AttributionSource = 'doubao' | 'yuanbao' | 'qianwen' | 'baidu' | 'other' | 'unknown';
export type SourceEvidence = 'tagged' | 'referrer' | 'unknown';
export type Currency = 'CNY' | 'USD' | 'EUR';
export interface AttributionProduct {id:string;sku:string;name:string;revision:number}
export interface AttributionCounts {visits:number;orders:number;refunds:number}
export interface AttributionDatasetSummary {id:string;name:string;createdAt:string;evidenceMode:AttributionEvidenceMode;sourceLabel:string;currency:Currency;coverageStart:string;coverageEnd:string;counts:AttributionCounts;contentHash:string}
export interface AttributionPreview {contentHash:string;evidenceMode:AttributionEvidenceMode;sourceLabel:string;currency:Currency;coverageStart:string;coverageEnd:string;counts:AttributionCounts & {duplicates:number};products:AttributionProduct[];alreadyImported:boolean}
export interface AttributionApplyResult {datasetId:string;alreadyImported:boolean;evidenceMode:AttributionEvidenceMode}
export interface AttributionMetrics {visits:number;visitingSessions:number;paidOrders:number;cancelledOrders:number;attributedOrders:number;unknownOrders:number;convertedSessions:number;grossMinor:number;refundMinor:number;netMinor:number;attributedNetMinor:number;unknownNetMinor:number;sessionConversionRate:number|null;unknownOrderRate:number|null;lookbackCoverageIncomplete:boolean}
export interface AttributionOrder {orderId:string;sku:string;occurredAt:string;status:'paid'|'cancelled';amountMinor:number;refundMinor:number;netMinor:number;source:AttributionSource;sourceEvidence:SourceEvidence;visitId:string|null;reason:string;visitInPeriod:boolean}
export interface AttributionReport {datasetId:string;contentHash:string;evidenceMode:AttributionEvidenceMode;currency:Currency;period:{from:string;to:string;refundAsOf:string;lookbackDays:number};summary:AttributionMetrics;bySource:(AttributionMetrics & {source:AttributionSource})[];byProduct:(AttributionMetrics & {sku:string;name:string})[];weekly:(AttributionMetrics & {weekStart:string})[];orders:AttributionOrder[];limitations:string[];files:{path:string;content:string}[]}
export interface AttributionRequest {brandId:string;datasetId:string;from:string;to:string;lookbackDays:number}
export interface AttributionExchange {format:'m-geo-attribution-v1';evidenceMode:AttributionEvidenceMode;sourceLabel:string;coverageStart:string;coverageEnd:string;currency:Currency;visits:{id:string;sessionId:string;sku:string;occurredAt:string;source:AttributionSource;sourceEvidence:SourceEvidence}[];orders:{id:string;sessionId:string|null;sku:string;occurredAt:string;status:'paid'|'cancelled';amountMinor:number}[];refunds:{id:string;orderId:string;occurredAt:string;amountMinor:number}[]}
export interface AttributionTemplate {exchange:AttributionExchange;files:{path:string;content:string}[]}
