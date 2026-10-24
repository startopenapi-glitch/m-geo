import type { AttributionEvidenceMode, AttributionMetrics, AttributionOrder, AttributionSource, Currency } from './attributionTypes';
/** Aggregate inspection only. Raw evidence and per-record identifiers stay outside this report. */
export interface AttributionInspectionReport {
    format: 'm-geo-attribution-inspection-v1';
    profile: 'attribution-v1';
    source: {
        fileName: string;
        sha256: string;
        compressedBytes: number;
        uncompressedBytes: number;
        fileCount: number;
    };
    brandId: string;
    datasetId: string;
    contentHash: string;
    evidenceMode: AttributionEvidenceMode;
    evidenceLabel: string;
    status: 'passed' | 'blocked';
    currency: Currency;
    coverage: {
        from: string;
        to: string;
    };
    period: {
        from: string;
        to: string;
        refundAsOf: string;
        lookbackDays: number;
    };
    counts: {
        visits: number;
        orders: number;
        refunds: number;
        products: number;
    };
    summary: AttributionMetrics;
    bySource: (AttributionMetrics & {
        source: AttributionSource;
    })[];
    weekly: (AttributionMetrics & {
        weekStart: string;
    })[];
    checks: {
        code: string;
        status: 'passed' | 'failed';
        message: string;
    }[];
    caveat: string;
    reportFingerprint: string;
}
export interface AttributionInspectionSelection {
    inspectionId: string;
    report: AttributionInspectionReport;
}
export type AttributionInspectionKind = 'orders' | 'products';
export type AttributionInspectionProduct = AttributionMetrics & {
    sku: string;
    name: string;
};
interface InspectionPageBase {
    inspectionId: string;
    page: number;
    pageSize: 20;
    total: number;
}
export type AttributionInspectionPage = (InspectionPageBase & {
    kind: 'orders';
    rows: AttributionOrder[];
}) | (InspectionPageBase & {
    kind: 'products';
    rows: AttributionInspectionProduct[];
});
