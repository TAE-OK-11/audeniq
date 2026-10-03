import { describe, expect, mock, test } from 'bun:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ReviewContext } from '../src/api/staff';

// These rendered components do not need a browser/router or network.
mock.module('../src/lib/router', () => ({ Link: () => null, useLocation: () => ({ pathname: '/' }) }));
mock.module('../src/api/staff', () => ({ staffApi: {} }));
const { CheckCard } = await import('../src/components/ReviewCheck');
const { contextOrReadOnly, ReviewActions } = await import('../src/components/ReviewActions');
const { TimelineRow } = await import('../src/components/ReviewTimeline');
const render = renderToStaticMarkup;
const context = (actions: ReviewContext['allowed_actions']): ReviewContext => ({
  decision_kind: 'CHECKS', allowed_actions: actions, requires_second_approval: true,
  pending_second_approval_id: null, check_counts: {},
});

describe('server review policy in the admin UI', () => {
  test('second approval badge follows the server, including a new code', () => {
    const html = render(createElement(CheckCard, { open: true, c: { check_code: 'NEW_POLICY_HOLD', status: 'REVIEW_REQUIRED', detail: '', needs_second_approval: true } }));
    expect(html).toContain('2인 승인<');
    const allowed = render(createElement(CheckCard, { open: true, c: { check_code: 'NEW_POLICY_HOLD', status: 'REVIEW_REQUIRED', detail: '', needs_second_approval: false } }));
    expect(allowed).not.toContain('2인 승인<');
  });
  test('pending second approval blocks approval while allowing correction/rejection', () => {
    const html = render(createElement(ReviewActions, { context: context(['REQUEST_CORRECTION', 'REJECT']), loading: false, onAction: () => {} }));
    expect(html).toMatch(/disabled="">승인/);
    expect(html).not.toMatch(/disabled="">보완 요청/);
    expect(html).not.toMatch(/disabled="">거절/);
  });
  test('read-only and refreshing sheets disable all decisions', () => {
    for (const [actions, loading] of [[[], false], [['APPROVE', 'REQUEST_CORRECTION', 'REJECT'], true]] as const) {
      const html = render(createElement(ReviewActions, { context: context([...actions]), loading, onAction: () => {} }));
      expect((html.match(/disabled=""/g) ?? []).length).toBe(3);
    }
  });
  test('applied override is distinguished from the original machine result', () => {
    const html = render(createElement(CheckCard, { c: { check_code: 'S2_SPECIAL_FLAGS', stage: 2, status: 'PASS', original_status: 'REVIEW_REQUIRED', detail: '<script>alert(1)</script>' } }));
    expect(html).toContain('담당자 결정 반영');
    expect(html).toContain('2차 검사');
    expect(html).not.toContain('<script>');
  });
});

describe('backend processing timeline', () => {
  test('job status and retries are displayed as current state, with error details', () => {
    const html = render(createElement(TimelineRow, { item: { at: '2026-09-30T00:00:00Z', source: 'job', kind: 'asset.analyze', detail: { status: 'QUEUED', attempts: 2, last_error: 'temporary decoder error' } } }));
    expect(html).toContain('음원 분석 등록');
    expect(html).toContain('현재 상태: 대기 · 실행 2회');
    expect(html).toContain('처리 오류 기록 있음');
    expect(html).toContain('temporary decoder error');
  });
  test('revision decisions and DSP acknowledgements get readable titles', () => {
    const audit = render(createElement(TimelineRow, { item: { at: '2026-09-30T00:00:00Z', source: 'audit', kind: 'stage2.decision', detail: {} } }));
    const ack = render(createElement(TimelineRow, { item: { at: '2026-09-30T00:00:00Z', source: 'dsp_ack', kind: 'D-5', detail: { outcome: 'LIVE' } } }));
    expect(audit).toContain('2차 검사 결과');
    expect(ack).toContain('Spotify 응답 수신');
    expect(ack).toContain('LIVE');
  });
});


const { audioSpecs } = await import('../src/lib/audio');
test('track technical specs reuse real measurements and preserve unknowns', () => {
  expect(audioSpecs({ duration_secs: 195.4, sample_rate: 44100, channels: 2, bits_per_sample: 24 })).toBe('3:15 · 44.1 kHz · 24 bit · 2채널');
  expect(audioSpecs({ duration_secs: 60, sample_rate: null, channels: null, bits_per_sample: null })).toBe('1:00');
  expect(audioSpecs(undefined)).toBe('분석 정보 없음');
  expect(audioSpecs({ duration_secs: Infinity, sample_rate: 0, channels: null, bits_per_sample: null })).toBe('분석 정보 없음');
});

test('a dead-letter job is reported as a processing failure', () => {
  const html = render(createElement(TimelineRow, { item: { at: '2026-09-30T00:00:00Z', source: 'job', kind: 'stage2', detail: { status: 'DEAD_LETTER', attempts: 5 } } }));
  expect(html).toContain('현재 상태: 처리 실패');
});

test('an older backend response stays readable with all decisions disabled', () => {
  const context = contextOrReadOnly(undefined);
  const html = render(createElement(ReviewActions, { context, loading: false, onAction: () => {} }));
  expect((html.match(/disabled=""/g) ?? []).length).toBe(3);
  expect(context.decision_kind).toBeNull();
});

describe('review timeline grouping and readable reasons', () => {
  test('audit reason codes become a readable line, never the raw code', async () => {
    const { reasonSummary, timelineKind } = await import('../src/components/ReviewTimeline');
    expect(reasonSummary('staff.correction_requested', 'FIX_AUDIO@t1,FIX_AUDIO@t2,S2_META_CREDITS')).toBe('보완 항목: 음원 파일, 크레딧');
    expect(reasonSummary('staff.approved', 'PASS:2')).toBe('확인 필요 2건 통과 처리');
    expect(reasonSummary('stage2.decision', 'REVIEW:S2_META_CREDITS')).toBe('확인 필요: 크레딧');
    expect(reasonSummary('release.submitted', 'SUBMIT')).toBe('');
    expect(timelineKind({ at: '', source: 'audit', kind: 'staff.approved', detail: {} })).toBe('staff');
    expect(timelineKind({ at: '', source: 'audit', kind: 'stage1.decision', detail: {} })).toBe('check');
    expect(timelineKind({ at: '', source: 'dsp_ack', kind: 'D-5', detail: {} })).toBe('dsp');
  });
  test('a failed job is marked as a failure', () => {
    const html = render(createElement(TimelineRow, { item: { at: '2026-09-30T00:00:00Z', source: 'job', kind: 'stage2', detail: { status: 'FAILED', attempts: 3 } } }));
    expect(html).toContain('is-bad');
  });
});

describe('platform delivery status', () => {
  const row = (dsp: string, readiness: string, checks: { code: string; severity?: string; message?: string }[] = []) => ({
    package_id: 'p', dsp, readiness, approval: 'PENDING', checks, route_status: null, route_reason: null, ern_message_id: null,
    ern_sha256: null, ern_is_preview: false, approval_by: null, approval_note: null, approval_at: null, staged_at: '2026-09-30T00:00:00Z',
  });
  test('blocked platforms come first and INFO notes do not count as problems', async () => {
    const { DeliveryStatus } = await import('../src/components/DeliveryStatus');
    const html = render(createElement(DeliveryStatus, { rows: [
      row('D-1', 'READY', [{ code: 'DSP_LOUDNESS_ADVISORY', severity: 'INFO', message: '음량 참고' }]),
      row('D-5', 'CONTENT_BLOCKED', [{ code: 'S2_DSP_ARTWORK_QR', severity: 'BLOCKER', message: 'QR 코드 발견' }]),
    ] }));
    expect(html.indexOf('Spotify')).toBeLessThan(html.indexOf('Melon') === -1 ? html.length : html.indexOf('Melon'));
    expect(html).toContain('막힘 1');
    expect(html).toContain('전송 가능 1');
    expect(html).toContain('문제 없는 플랫폼 1곳');
    expect(html).toMatch(/class="is-bad">QR 코드 발견/);
  });
});

test('review brief flags missing Content ID confirmations and blocked platforms', async () => {
  const { reviewBrief } = await import('../src/components/ReviewBrief');
  const sheet = {
    release: { title: 'T', cover: null, upc: null }, application: { platforms: ['D-27'], options: { contentIdExclusiveRightsAck: true }, release_date: '' },
    open_checks: [], documents: [], draft: null,
    review_context: { decision_kind: 'CHECKS', allowed_actions: [], requires_second_approval: false, pending_second_approval_id: 'ap-1', check_counts: {} },
    delivery_staging: [{ dsp: 'D-5', readiness: 'CONTENT_BLOCKED', route_reason: null, checks: [{ code: 'S2_DSP_ARTWORK_QR', severity: 'BLOCKER', message: 'QR 코드 발견' }] }],
  } as unknown as Parameters<typeof reviewBrief>[0];
  const items = reviewBrief(sheet, 'unsigned');
  const cid = items.find(i => i.key === 'cid')!;
  expect(cid.tone).toBe('bad');
  expect(cid.fix?.code).toBe('FIX_CONTENT_ID');
  expect(cid.detail).toContain('직접 제작한 고유한 녹음');
  expect(cid.detail).not.toContain('독점 권리');
  expect(items.find(i => i.key === 'dsp-D-5')?.detail).toBe('QR 코드 발견');
  expect(items.find(i => i.key === 'second')?.title).toBe('2차 승인 대기 중');
});

describe('review claim', () => {
  const ctx = (claim: ReviewContext['claim'], extra: Partial<ReviewContext> = {}): ReviewContext => ({
    decision_kind: 'CHECKS', allowed_actions: [], requires_second_approval: false, pending_second_approval_id: null, check_counts: {}, claim, ...extra,
  });
  test('unclaimed review offers to take it; another reviewer is shown and only ADMIN can take over', async () => {
    const { ReviewClaimPanel } = await import('../src/components/ReviewClaim');
    const noop = async () => {};
    const free = render(createElement(ReviewClaimPanel, { context: ctx(null, { can_claim: true }), canReview: true, onClaim: noop, onRelease: noop }));
    expect(free).toContain('이 심사 담당하기');
    const other = render(createElement(ReviewClaimPanel, { context: ctx({ user_id: 'u2', email: 'kim@audeniq.com', at: '2026-10-02T00:00:00Z', mine: false }), canReview: true, onClaim: noop, onRelease: noop }));
    expect(other).toContain('kim@audeniq.com');
    expect(other).not.toContain('넘겨받기');
    const admin = render(createElement(ReviewClaimPanel, { context: ctx({ user_id: 'u2', email: 'kim@audeniq.com', at: '2026-10-02T00:00:00Z', mine: false }, { can_take_over: true }), canReview: true, onClaim: noop, onRelease: noop }));
    expect(admin).toContain('넘겨받기');
    const mine = render(createElement(ReviewClaimPanel, { context: ctx({ user_id: 'u1', email: 'me@audeniq.com', at: '2026-10-02T00:00:00Z', mine: true }), canReview: true, onClaim: noop, onRelease: noop }));
    expect(mine).toContain('내 담당');
    expect(mine).toContain('담당 해제');
  });
});
