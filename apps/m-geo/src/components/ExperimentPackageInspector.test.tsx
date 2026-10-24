import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import ExperimentPackageInspector from './ExperimentPackageInspector';
import Visibility from '../pages/Visibility';
import { setTestTransport, type Transport } from '../api';
import type { PageProps } from '../App';

const brandId = '11111111-1111-4111-8111-111111111111';
const otherBrand = '22222222-2222-4222-8222-222222222222';
const inspectionId = '33333333-3333-4333-8333-333333333333';
const secondId = '44444444-4444-4444-8444-444444444444';
const fingerprint = 'a'.repeat(64);
const report = {
  format: 'm-geo-experiment-inspection-v1',
  source: {
    fileName: '虚构历史.zip',
    sha256: 'b'.repeat(64),
    compressedBytes: 100,
    uncompressedBytes: 500,
    fileCount: 3,
  },
  profile: 'current-v1',
  brandId,
  campaignId: '55555555-5555-4555-8555-555555555555',
  evidenceMode: 'simulated',
  evidenceLabel: '模拟样本：仅用于工作流演练，不代表真实平台结果',
  status: 'passed',
  counts: {
    observations: 21,
    expectedPerPhase: 12,
    baseline: 12,
    followup: 9,
    missing: 3,
    reviewedObservations: 1,
    historyEntries: 1,
  },
  platforms: [{
    platform: 'doubao',
    expectedPerPhase: 12,
    baseline: {
      samples: 12,
      assessed: 10,
      recommended: 8,
      unknown: 2,
      recommendationRate: .8,
      ranked: 5,
      top3: 4,
    },
    followup: {
      samples: 9,
      assessed: 8,
      recommended: 7,
      unknown: 1,
      recommendationRate: .875,
      ranked: 6,
      top3: 5,
    },
    matchedPairs: 9,
    matchedAssessedPairs: 7,
    matchedBaselineRecommended: 5,
    matchedFollowupRecommended: 6,
    unmatchedSamples: 3,
    deltaPercentagePoints: 100 / 7,
  }],
  checks: ['evidence_schema', 'assessment_history', 'summary', 'csv', 'markdown'].map(code => ({
    code,
    status: 'passed',
    message: `${code} 一致`,
  })),
  caveat: '一致性不证明真实性、平台认证或因果改善。',
  reportFingerprint: fingerprint,
};
const assessment = {
  campaignId: report.campaignId,
  repetition: 1,
  context: '冻结测试条件',
  recommendation: 'yes',
  rank: 1,
  competitors: '当前竞品说明',
};
const raw = '  \uFEFF<script>globalThis.rawExecuted=true</script>\n[链接](https://example.com/raw)  ';

function observation(i: number) {
  return {
    id: `obs-${i}`,
    brandId,
    productId: 'product',
    questionId: 'question',
    querySnapshot: `冻结问题${i}`,
    branded: false,
    platform: 'doubao',
    surface: 'consumer',
    phase: 'baseline',
    modelLabel: '历史模型',
    searchMode: '历史搜索',
    testedAt: '2026-10-20T00:00:00Z',
    answer: i === 0 ? raw : `历史回答${i}`,
    mentioned: true,
    accuracy: 'correct',
    citedUrl: 'https://example.com/raw',
    evidenceNote: '当前核对说明',
    assessment,
    ...(i === 0 ? {
      assessmentRevision: 2,
      assessmentHistory: [{
        revision: 1,
        supersededAt: '2026-10-21T00:00:00Z',
        mentioned: false,
        accuracy: 'unknown',
        citedUrl: 'https://example.com/old',
        evidenceNote: '旧版核对说明',
        assessment: {
          ...assessment,
          recommendation: 'unknown',
          rank: null,
          competitors: '旧版竞品说明',
        },
      }],
    } : {}),
  };
}

function page(n = 0, total = 21, id = inspectionId) {
  return {
    inspectionId: id,
    page: n,
    pageSize: 20,
    total,
    observations: Array.from(
      { length: Math.min(20, Math.max(0, total - n * 20)) },
      (_, i) => observation(n * 20 + i),
    ),
  };
}

function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (value: unknown) => void;
  return {
    promise: new Promise((res, rej) => {
      resolve = res;
      reject = rej;
    }),
    resolve,
    reject,
  };
}

function transport(handler: (action: string, payload: Record<string, unknown>) => unknown) {
  const spy = vi.fn(handler);
  setTestTransport((async (action, payload) => spy(action, payload)) as Transport);
  return spy;
}

function normalTransport(selected: unknown = { inspectionId, report }, total = 21) {
  return transport((action, payload) =>
    action === 'file.inspectExperimentReport'
      ? selected
      : action === 'file.readExperimentInspectionPage'
        ? page(
          payload.page as number,
          total,
          (selected as { inspectionId?: string })?.inspectionId,
        )
        : { saved: true },
  );
}

async function open() {
  fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
  await screen.findByText('虚构历史.zip');
}

afterEach(() => {
  cleanup();
  setTestTransport(undefined);
});

describe('historical experiment package inspector', () => {
  it('requests metadata then page zero, distinguishes annotation versions and renders raw text inertly', async () => {
    const calls = normalTransport();
    const view = render(<ExperimentPackageInspector brandId={brandId} />);

    await open();
    await screen.findByText('冻结问题0');

    expect(calls.mock.calls).toEqual([
      ['file.inspectExperimentReport', { brandId }],
      ['file.readExperimentInspectionPage', { brandId, inspectionId, page: 0 }],
    ]);
    expect(screen.getByText(report.evidenceLabel)).toBeTruthy();
    expect(screen.getByText('基线 12/12 · 复测 9/12 · 缺失 3/24 个计划单元')).toBeTruthy();
    expect(screen.getByText('基线：推荐 8/10 条已核对；未知 2/12 条；前三 4/5 条有顺位')).toBeTruthy();
    expect(screen.getByText('配对推荐率变化：14.3 个百分点（仅 7 对两阶段已核对样本）')).toBeTruthy();

    const sample = screen.getByText('冻结问题0').closest('details')!;
    expect(within(sample).getByText('当前评估 v2 · 历史版本 1 次')).toBeTruthy();
    expect(within(sample).getByText('历史评估 v1')).toBeTruthy();
    expect(within(sample).getByText('旧版核对说明')).toBeTruthy();
    expect(within(sample).getByText('当前核对说明')).toBeTruthy();
    expect(sample.querySelector('pre')!.textContent).toBe(raw);
    expect(view.container.querySelector('a,script,iframe,img')).toBeNull();
    expect(within(sample).getByText('https://example.com/raw')).toBeTruthy();
    expect(screen.getAllByText(/当前评估 v/)).toHaveLength(20);
    expect(screen.getAllByText('未保存更正历史（初始评估 v1）。')).toHaveLength(19);
  });

  it.each([
    ['legacy-v1', 'unspecified', '历史性质未分类：不能作为已确认的真实平台实测'],
    ['current-v1', 'recorded', '人工实测记录：由用户录入，需核对原始证据'],
  ])('shows %s without inventing provenance', async (profile, evidenceMode, evidenceLabel) => {
    normalTransport({
      inspectionId,
      report: {
        ...report,
        profile,
        evidenceMode,
        evidenceLabel,
        counts: { ...report.counts, observations: 0 },
      },
    }, 0);
    render(<ExperimentPackageInspector brandId={brandId} />);

    await open();
    await screen.findByText('历史包没有原始观察记录。');

    expect(screen.getByText(evidenceLabel)).toBeTruthy();
    expect(screen.getByText(profile)).toBeTruthy();
  });

  it('keeps a blocked package browsable and exports exact ticket and fingerprint', async () => {
    const calls = normalTransport({
      inspectionId,
      report: {
        ...report,
        status: 'blocked',
        checks: report.checks.map(c =>
          c.code === 'csv'
            ? { ...c, status: 'failed', message: 'CSV 与原始证据不一致' }
            : c,
        ),
      },
    });
    render(<ExperimentPackageInspector brandId={brandId} />);

    await open();
    await screen.findByText('一致性检查未通过');
    await screen.findByText('冻结问题0');
    expect(screen.getByText(/csv · 未通过/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: '导出本地核验报告 ZIP' }));
    await screen.findByText('本地核验报告 ZIP 已保存');
    expect(calls.mock.calls.at(-1)).toEqual([
      'file.exportExperimentInspection',
      { brandId, inspectionId, reportFingerprint: fingerprint },
    ]);
  });

  it('requests a single 20-row page and resets on a replacement ticket', async () => {
    let selected = 0;
    const calls = transport((action, payload) =>
      action === 'file.inspectExperimentReport'
        ? { inspectionId: ++selected === 1 ? inspectionId : secondId, report }
        : action === 'file.readExperimentInspectionPage'
          ? page(payload.page as number, 21, payload.inspectionId as string)
          : null,
    );
    render(<ExperimentPackageInspector brandId={brandId} />);
    await open();
    await screen.findByText('冻结问题0');
    const nav = screen.getByRole('navigation', { name: '历史原始记录分页' });

    fireEvent.click(within(nav).getByRole('button', { name: '下一页' }));
    await screen.findByText('冻结问题20');

    expect(screen.queryByText('冻结问题0')).toBeNull();
    expect(within(nav).getByText('第 2/2 页 · 共 21 条')).toBeTruthy();
    expect((within(nav).getByRole('button', { name: '下一页' }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
    expect(screen.queryByText('冻结问题20')).toBeNull();
    await screen.findByText('冻结问题0');

    expect(
      within(screen.getByRole('navigation', { name: '历史原始记录分页' }))
        .getByText('第 1/2 页 · 共 21 条'),
    ).toBeTruthy();
    expect(calls.mock.calls.at(-1)).toEqual([
      'file.readExperimentInspectionPage',
      { brandId, inspectionId: secondId, page: 0 },
    ]);
  });

  it.each(['cancel', 'failure'])('clears prior selection before picker and stays clear on %s', async (outcome) => {
    const pending = deferred();
    let count = 0;
    transport((action, payload) =>
      action === 'file.inspectExperimentReport'
        ? ++count === 1 ? { inspectionId, report } : pending.promise
        : page(payload.page as number),
    );
    render(<ExperimentPackageInspector brandId={brandId} />);
    await open();
    await screen.findByText('冻结问题0');

    fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
    expect(screen.queryByText('虚构历史.zip')).toBeNull();
    expect(screen.queryByText('冻结问题0')).toBeNull();

    await act(async () => {
      if (outcome === 'cancel') pending.resolve(null);
      else pending.reject(new Error('历史包无法核验'));
    });
    await screen.findByText(outcome === 'cancel' ? '已取消选择历史实验 ZIP' : '历史包无法核验');
    expect(screen.queryByText('虚构历史.zip')).toBeNull();
  });

  it('locks rapid clicks and competing selection/page/export actions', async () => {
    const selecting = deferred();
    const paging = deferred();
    const exporting = deferred();
    const calls = transport(action =>
      action === 'file.inspectExperimentReport'
        ? selecting.promise
        : action === 'file.readExperimentInspectionPage' ? paging.promise : exporting.promise,
    );
    render(<ExperimentPackageInspector brandId={brandId} />);
    const picker = screen.getByRole('button', { name: '选择历史实验 ZIP' });

    fireEvent.click(picker);
    fireEvent.click(picker);
    expect(calls).toHaveBeenCalledTimes(1);

    await act(async () => selecting.resolve({ inspectionId, report }));
    expect((picker as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: '导出本地核验报告 ZIP' }) as HTMLButtonElement).disabled).toBe(true);

    await act(async () => paging.resolve(page()));
    const exporter = screen.getByRole('button', { name: '导出本地核验报告 ZIP' });
    fireEvent.click(exporter);
    fireEvent.click(exporter);
    expect(calls.mock.calls.filter(([a]) => a === 'file.exportExperimentInspection')).toHaveLength(1);
    expect((picker as HTMLButtonElement).disabled).toBe(true);

    await act(async () => exporting.resolve({ saved: true }));
    await screen.findByText('本地核验报告 ZIP 已保存');
  });

  it('preserves report after cancelled or failed export', async () => {
    let exports = 0;
    transport((action, payload) =>
      action === 'file.inspectExperimentReport'
        ? { inspectionId, report }
        : action === 'file.readExperimentInspectionPage'
          ? page(payload.page as number)
          : ++exports === 1 ? null : Promise.reject(new Error('核验报告保存失败')),
    );
    render(<ExperimentPackageInspector brandId={brandId} />);
    await open();
    await screen.findByText('冻结问题0');

    fireEvent.click(screen.getByRole('button', { name: '导出本地核验报告 ZIP' }));
    await screen.findByText('已取消保存本地核验报告');
    expect(screen.getByText('虚构历史.zip')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: '导出本地核验报告 ZIP' }));
    await screen.findByText('核验报告保存失败');
    expect(screen.getByText('冻结问题0')).toBeTruthy();
  });

  it.each(['selection', 'page', 'export'])('suppresses late %s after brand switch and unlocks new brand', async (stage) => {
    const pending = deferred();
    transport((action, payload) =>
      payload.brandId === otherBrand
        ? null
        : action === 'file.inspectExperimentReport'
          ? stage === 'selection' ? pending.promise : { inspectionId, report }
          : action === 'file.readExperimentInspectionPage'
            ? stage === 'page' ? pending.promise : page()
            : pending.promise,
    );
    const view = render(<ExperimentPackageInspector brandId={brandId} />);
    fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
    if (stage !== 'selection') await screen.findByText('虚构历史.zip');
    if (stage === 'export') {
      await screen.findByText('冻结问题0');
      fireEvent.click(screen.getByRole('button', { name: '导出本地核验报告 ZIP' }));
    }

    view.rerender(<ExperimentPackageInspector brandId={otherBrand} />);
    await act(async () => pending.resolve(
      stage === 'selection' ? { inspectionId, report } : stage === 'page' ? page() : { saved: true },
    ));

    expect(screen.queryByText('虚构历史.zip')).toBeNull();
    expect(screen.queryByText('冻结问题0')).toBeNull();
    expect(screen.queryByText('本地核验报告 ZIP 已保存')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
    await screen.findByText('已取消选择历史实验 ZIP');
  });

  it.each(['selection', 'page', 'export'])('suppresses late %s after unmount', async (stage) => {
    const pending = deferred();
    const calls = transport(action =>
      action === 'file.inspectExperimentReport'
        ? stage === 'selection' ? pending.promise : { inspectionId, report }
        : action === 'file.readExperimentInspectionPage'
          ? stage === 'page' ? pending.promise : page()
          : pending.promise,
    );
    const view = render(<ExperimentPackageInspector brandId={brandId} />);
    fireEvent.click(screen.getByRole('button', { name: '选择历史实验 ZIP' }));
    if (stage !== 'selection') await screen.findByText('虚构历史.zip');
    if (stage === 'export') {
      await screen.findByText('冻结问题0');
      fireEvent.click(screen.getByRole('button', { name: '导出本地核验报告 ZIP' }));
    }
    const before = calls.mock.calls.length;

    view.unmount();
    await act(async () => pending.resolve(
      stage === 'selection' ? { inspectionId, report } : stage === 'page' ? page() : { saved: true },
    ));

    expect(calls).toHaveBeenCalledTimes(before);
    expect(screen.queryByText('虚构历史.zip')).toBeNull();
  });

  it('keeps metadata after page failure and permits retry', async () => {
    let pages = 0;
    transport(action =>
      action === 'file.inspectExperimentReport'
        ? { inspectionId, report }
        : action === 'file.readExperimentInspectionPage'
          ? ++pages === 1 ? Promise.reject(new Error('页面读取失败')) : page()
          : null,
    );
    render(<ExperimentPackageInspector brandId={brandId} />);
    await open();
    await screen.findByText('页面读取失败');
    expect(screen.getByText('虚构历史.zip')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: '重试读取历史记录' }));
    await screen.findByText('冻结问题0');
  });

  it('works without current campaigns and survives same-brand snapshot refresh', async () => {
    const calls = normalTransport();
    const props = {
      brand: { id: brandId, name: '虚构品牌' },
      data: { products: [], questions: [], campaigns: [], observations: [], drafts: [], publications: [] },
      refresh: vi.fn(),
    } as unknown as PageProps;
    const view = render(<Visibility {...props} />);
    await open();
    await screen.findByText('冻结问题0');

    view.rerender(<Visibility {...props} data={{ ...props.data }} />);

    expect(screen.getByText('虚构历史.zip')).toBeTruthy();
    expect(screen.getByText('冻结问题0')).toBeTruthy();
    expect(calls.mock.calls.map(([a]) => a)).toEqual([
      'file.inspectExperimentReport',
      'file.readExperimentInspectionPage',
    ]);
  });
});
