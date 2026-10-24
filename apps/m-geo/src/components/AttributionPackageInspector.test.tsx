import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { setTestTransport } from '../api';
import type { AttributionMetrics, AttributionOrder } from '../attributionTypes';
import type { AttributionInspectionPage, AttributionInspectionSelection, AttributionInspectionReport } from '../attributionInspectionTypes';
import AttributionPackageInspector from './AttributionPackageInspector';
const brandId = '00000000-0000-4000-8000-000000000001', inspectionId = '00000000-0000-4000-8000-000000000002';
const metrics: AttributionMetrics = {
    visits: 9,
    visitingSessions: 7,
    paidOrders: 8,
    cancelledOrders: 1,
    attributedOrders: 3,
    unknownOrders: 5,
    convertedSessions: 5,
    grossMinor: 81000,
    refundMinor: 22000,
    netMinor: 59000,
    attributedNetMinor: 10000,
    unknownNetMinor: 49000,
    sessionConversionRate: 5 / 7,
    unknownOrderRate: 5 / 8,
    lookbackCoverageIncomplete: true,
};
const report: AttributionInspectionReport = {
    format: 'm-geo-attribution-inspection-v1',
    profile: 'attribution-v1',
    source: {
        fileName: '历史归因.zip',
        sha256: 'a'.repeat(64),
        compressedBytes: 1200,
        uncompressedBytes: 3600,
        fileCount: 9,
    },
    brandId,
    datasetId: '00000000-0000-4000-8000-000000000003',
    contentHash: 'b'.repeat(64),
    evidenceMode: 'simulated',
    evidenceLabel: '模拟数据',
    status: 'passed',
    currency: 'CNY',
    coverage: {from: '2026-10-01T00:00:00Z', to: '2026-10-20T00:00:00Z'},
    period: {
        from: '2026-10-05T00:00:00Z',
        to: '2026-10-12T00:00:00Z',
        refundAsOf: '2026-10-20T00:00:00Z',
        lookbackDays: 7,
    },
    counts: {visits: 9, orders: 21, refunds: 2, products: 21},
    summary: metrics,
    bySource: [{...metrics, source: 'unknown'}],
    weekly: [{...metrics, weekStart: '2026-10-05T00:00:00Z'}],
    checks: [{code: 'frozen_dataset', status: 'passed', message: '冻结资料一致'}],
    caveat: '仅核验文件与声明来源一致性；哈希不是签名，不证明 AI 因果销量。',
    reportFingerprint: 'c'.repeat(64),
};
const selection: AttributionInspectionSelection = { inspectionId, report };
const hostile = '<script>alert(1)</script> https://private.invalid/ =SKU\n\t';
function order(index: number): AttributionOrder { return { orderId: `order-${index}`, sku: index === 0 ? hostile : `SKU-${index}`, occurredAt: report.period.from, status: 'paid', amountMinor: 1000, refundMinor: 1000, netMinor: 0, source: 'unknown', sourceEvidence: 'unknown', visitId: null, reason: 'missing_session', visitInPeriod: false }; }
function page(kind: 'orders' | 'products', target = 0, ticket = inspectionId): AttributionInspectionPage {
    const indexes = Array.from({ length: target === 0 ? 20 : 1 }, (_, i) => i + target * 20);
    return kind === 'orders' ? { inspectionId: ticket, kind, page: target, pageSize: 20, total: 21, rows: indexes.map(order) } : { inspectionId: ticket, kind, page: target, pageSize: 20, total: 21, rows: indexes.map(i => ({ ...metrics, sku: `product-${i}`, name: i === 0 ? hostile : `冻结商品-${i}` })) };
}
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
function transport(override?: (action: string, payload: Record<string, unknown>) => unknown) {
    const calls = vi.fn(async (action: string, payload: Record<string, unknown>) => {
        const custom = override?.(action, payload);
        if (custom !== undefined) return await custom;
        if (action === 'file.inspectAttributionReport') return selection;
        if (action === 'file.readAttributionInspectionPage') {
            return page(payload.kind as 'orders' | 'products', payload.page as number, payload.inspectionId as string);
        }
        if (action === 'file.exportAttributionInspection') return null;
        throw new Error(`Unexpected action: ${action}`);
    });
    setTestTransport(async <T,>(action:string, payload:Record<string,unknown>) => await calls(action, payload) as T);
    return calls;
}
async function open() { fireEvent.click(screen.getByRole('button', { name: '选择历史归因 ZIP' })); await screen.findByRole('table', { name: '历史订单行' }); }
afterEach(() => { cleanup(); setTestTransport(undefined); });
it('starts empty and reads the native ticket, rows and kind contracts with identifiers only', async () => {
    const calls = transport();
    render(<AttributionPackageInspector brandId={brandId}/>);
    expect(screen.getByText('尚未选择历史归因 ZIP。')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '导出归因核验汇总 ZIP' })).toBeDisabled();
    await open();
    expect(calls.mock.calls[0]).toEqual(['file.inspectAttributionReport', { brandId }]);
    expect(calls.mock.calls[1]).toEqual(['file.readAttributionInspectionPage', { brandId, inspectionId, kind: 'orders', page: 0 }]);
    expect(screen.getByText('一致性检查通过')).toBeInTheDocument();
    expect(screen.getByText('模拟数据')).toBeInTheDocument();
    expect(screen.getByText(/展开 3600 字节/)).toBeInTheDocument();
    expect(within(screen.getByRole('table', { name: '历史订单行' })).getAllByRole('row')).toHaveLength(21);
});
it('shows aggregate money, cutoff, unknown rates, source/week data and cohort limitations', async () => {
    transport();
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    const summary = within(screen.getByRole('region', { name: '历史汇总指标' }));
    for (const amount of ['CNY 810.00', 'CNY 220.00', 'CNY 590.00', 'CNY 490.00', '71.43%', '62.50%'])
        expect(summary.getByText(amount)).toBeInTheDocument();
    expect(screen.getByText(/退款统计截至 2026-10-20/)).toBeInTheDocument();
    expect(screen.getByText(/来源与周分组的会话数不可相加/)).toBeInTheDocument();
    expect(screen.getByText(/全额退款仍保留历史支付转化/)).toBeInTheDocument();
    expect(screen.getByText(/汇总金额.*商品名称.*SKU.*订单.*会话/)).toBeInTheDocument();
    expect(screen.getByText(report.caveat)).toBeInTheDocument();
    expect(screen.getByRole('table', { name: '历史按来源' })).toBeInTheDocument();
    expect(screen.getByRole('table', { name: '历史按 UTC 周一' })).toBeInTheDocument();
});
it('distinguishes recorded declarations and null denominators', async () => {
    transport(action => action === 'file.inspectAttributionReport' ? { ...selection, report: { ...report, evidenceMode: 'recorded', evidenceLabel: '记录数据（导入者声明）', summary: { ...metrics, sessionConversionRate: null, unknownOrderRate: null } } } : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    expect(screen.getByText('记录数据（导入者声明）')).toBeInTheDocument();
    expect(within(screen.getByRole('region', { name: '历史汇总指标' })).getAllByText('不适用（分母为 0）')).toHaveLength(2);
});
it('keeps blocked companion checks browseable and exportable', async () => {
    transport(action => action === 'file.inspectAttributionReport' ? { ...selection, report: { ...report, status: 'blocked', checks: [{ code: 'orders_csv', status: 'failed', message: '订单文件不一致' }] } } : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    expect(screen.getByText('一致性检查未通过')).toBeInTheDocument();
    expect(screen.getByText(/orders_csv · 未通过/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '导出归因核验汇总 ZIP' })).toBeEnabled();
});
it('pages orders/products with independent cursors and clears old rows while loading', async () => {
    const next = deferred<AttributionInspectionPage>();
    transport((a, p) => a === 'file.readAttributionInspectionPage' && p.kind === 'orders' && p.page === 1 ? next.promise : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    fireEvent.click(screen.getByRole('button', { name: '下一页' }));
    expect(screen.queryByText('order-0')).toBeNull();
    expect(screen.getByText('历史归因.zip')).toBeInTheDocument();
    await act(async () => next.resolve(page('orders', 1)));
    await screen.findByText('order-20');
    fireEvent.change(screen.getByLabelText('历史明细类型'), { target: { value: 'products' } });
    await screen.findByText('冻结商品-1 · product-1');
    expect(screen.getByText('第 1/2 页 · 共 21 条')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('历史明细类型'), { target: { value: 'orders' } });
    await screen.findByText('order-20');
    expect(screen.getByText('第 2/2 页 · 共 21 条')).toBeInTheDocument();
});
it('resets both page cursors and the selected view for each new ticket', async () => {
    let selected = 0;
    const ticket2 = '00000000-0000-4000-8000-000000000004';
    const calls = transport(a => a === 'file.inspectAttributionReport' ? { ...selection, inspectionId: ++selected === 1 ? inspectionId : ticket2 } : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    fireEvent.click(screen.getByRole('button', { name: '下一页' }));
    await screen.findByText('order-20');
    fireEvent.change(screen.getByLabelText('历史明细类型'), { target: { value: 'products' } });
    await screen.findByText('冻结商品-1 · product-1');
    fireEvent.click(screen.getByRole('button', { name: '下一页' }));
    await screen.findByText('冻结商品-20 · product-20');
    await open();
    expect(screen.getByLabelText('历史明细类型')).toHaveValue('orders');
    expect(calls.mock.calls.at(-1)).toEqual(['file.readAttributionInspectionPage', { brandId, inspectionId: ticket2, kind: 'orders', page: 0 }]);
    fireEvent.change(screen.getByLabelText('历史明细类型'), { target: { value: 'products' } });
    await screen.findByText('冻结商品-1 · product-1');
    expect(screen.getByText('第 1/2 页 · 共 21 条')).toBeInTheDocument();
});
it('renders raw HTML, URLs and formula-looking SKU as inert text', async () => {
    transport();
    const view = render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    expect(view.container.textContent).toContain(hostile);
    expect(view.container.querySelector('script,a,iframe')).toBeNull();
    fireEvent.change(screen.getByLabelText('历史明细类型'), { target: { value: 'products' } });
    await screen.findByText('冻结商品-1 · product-1');
    expect(view.container.textContent).toContain(hostile);
    expect(view.container.querySelector('script,a,iframe')).toBeNull();
});
it('exports identifier-only payloads and preserves result on export cancellation', async () => {
    const calls = transport();
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    fireEvent.click(screen.getByRole('button', { name: '导出归因核验汇总 ZIP' }));
    await screen.findByText('已取消保存归因核验汇总');
    expect(screen.getByText('历史归因.zip')).toBeInTheDocument();
    expect(calls.mock.calls.at(-1)).toEqual(['file.exportAttributionInspection', { brandId, inspectionId, reportFingerprint: report.reportFingerprint }]);
});
it.each(['cancel', 'fail'])('clears old result when new selection ends in %s', async (outcome) => {
    let selected = 0;
    transport(a => a === 'file.inspectAttributionReport' && ++selected > 1 ? (outcome === 'cancel' ? null : Promise.reject(new Error('归因包证据无效、归属不符或超过处理上限'))) : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    fireEvent.click(screen.getByRole('button', { name: '选择历史归因 ZIP' }));
    await screen.findByText(outcome === 'cancel' ? '已取消选择历史归因 ZIP' : '归因包证据无效、归属不符或超过处理上限');
    expect(screen.queryByText('历史归因.zip')).toBeNull();
    expect(screen.queryByText('order-0')).toBeNull();
    expect(screen.getByRole('button', { name: '导出归因核验汇总 ZIP' })).toBeDisabled();
});
it('suppresses synchronous duplicate selection, page and export clicks', async () => {
    const select = deferred<AttributionInspectionSelection | null>(), read = deferred<AttributionInspectionPage>(), save = deferred<{
        saved: true;
    } | null>();
    const calls = transport((a, p) => a === 'file.inspectAttributionReport' ? select.promise : a === 'file.readAttributionInspectionPage' && p.page === 1 ? read.promise : a === 'file.exportAttributionInspection' ? save.promise : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    const button = screen.getByRole('button', { name: '选择历史归因 ZIP' });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(calls.mock.calls.filter(([a]) => a === 'file.inspectAttributionReport')).toHaveLength(1);
    await act(async () => select.resolve(selection));
    await screen.findByText('order-0');
    const next = screen.getByRole('button', { name: '下一页' });
    fireEvent.click(next);
    fireEvent.click(next);
    expect(calls.mock.calls.filter(([, p]) => p.page === 1)).toHaveLength(1);
    expect(button).toBeDisabled();
    await act(async () => read.resolve(page('orders', 1)));
    const exp = screen.getByRole('button', { name: '导出归因核验汇总 ZIP' });
    fireEvent.click(exp);
    fireEvent.click(exp);
    expect(calls.mock.calls.filter(([a]) => a === 'file.exportAttributionInspection')).toHaveLength(1);
    await act(async () => save.resolve({ saved: true }));
    await screen.findByText('归因核验汇总 ZIP 已保存');
});
it.each(['selection', 'page', 'export'])('ignores pending %s after brand change', async (pending) => {
    const late = deferred<unknown>();
    transport((a, p) => pending === 'selection' && a === 'file.inspectAttributionReport' ? late.promise : pending === 'page' && a === 'file.readAttributionInspectionPage' && p.page === 1 ? late.promise : pending === 'export' && a === 'file.exportAttributionInspection' ? late.promise : undefined);
    const view = render(<AttributionPackageInspector brandId={brandId}/>);
    if (pending === 'selection')
        fireEvent.click(screen.getByRole('button', { name: '选择历史归因 ZIP' }));
    else {
        await open();
        fireEvent.click(screen.getByRole('button', { name: pending === 'page' ? '下一页' : '导出归因核验汇总 ZIP' }));
    }
    view.rerender(<AttributionPackageInspector brandId="00000000-0000-4000-8000-000000000009"/>);
    await act(async () => late.resolve(pending === 'selection' ? selection : pending === 'page' ? page('orders', 1) : { saved: true }));
    expect(screen.getByText('尚未选择历史归因 ZIP。')).toBeInTheDocument();
    expect(screen.queryByText('归因核验汇总 ZIP 已保存')).toBeNull();
});
it.each(['selection', 'page', 'export'])('ignores pending %s after unmount and re-entry', async (pending) => {
    const late = deferred<unknown>();
    transport((a, p) => pending === 'selection' && a === 'file.inspectAttributionReport' ? late.promise : pending === 'page' && a === 'file.readAttributionInspectionPage' && p.page === 1 ? late.promise : pending === 'export' && a === 'file.exportAttributionInspection' ? late.promise : undefined);
    const view = render(<AttributionPackageInspector brandId={brandId}/>);
    if (pending === 'selection')
        fireEvent.click(screen.getByRole('button', { name: '选择历史归因 ZIP' }));
    else {
        await open();
        fireEvent.click(screen.getByRole('button', { name: pending === 'page' ? '下一页' : '导出归因核验汇总 ZIP' }));
    }
    view.unmount();
    render(<AttributionPackageInspector brandId={brandId}/>);
    await act(async () => late.resolve(pending === 'selection' ? selection : pending === 'page' ? page('orders', 1) : { saved: true }));
    expect(screen.getByText('尚未选择历史归因 ZIP。')).toBeInTheDocument();
    expect(screen.queryByText('归因核验汇总 ZIP 已保存')).toBeNull();
});
it('retains report and retries failed local pages without selecting again', async () => {
    let reads = 0;
    transport(a => a === 'file.readAttributionInspectionPage' && ++reads === 1 ? Promise.reject(new Error('无法读取本地历史页')) : undefined);
    render(<AttributionPackageInspector brandId={brandId}/>);
    fireEvent.click(screen.getByRole('button', { name: '选择历史归因 ZIP' }));
    await screen.findByText('无法读取本地历史页');
    expect(screen.getByText('历史归因.zip')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '重试读取历史明细' }));
    await screen.findByText('order-0');
});

it('shows an empty local page without a spurious next page', async () => {
    transport((action, payload) => {
        if (action === 'file.inspectAttributionReport') {
            return {...selection, report: {...report, counts: {...report.counts, orders: 0, products: 0}}};
        }
        if (action === 'file.readAttributionInspectionPage') {
            return {inspectionId, kind: payload.kind, page: 0, pageSize: 20, total: 0, rows: []};
        }
    });
    render(<AttributionPackageInspector brandId={brandId}/>);
    await open();
    expect(screen.getByText('历史包没有此类明细。')).toBeInTheDocument();
    expect(screen.getByText('第 1/1 页 · 共 0 条')).toBeInTheDocument();
    expect(screen.getByRole('button', {name: '上一页'})).toBeDisabled();
    expect(screen.getByRole('button', {name: '下一页'})).toBeDisabled();
});
