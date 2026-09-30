import { describe, expect, mock, test } from 'bun:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ReviewContext } from '../src/api/staff';

// These rendered components do not need a browser/router or network.
mock.module('../src/lib/router', () => ({ Link: () => null, useLocation: () => ({ pathname: '/' }) }));
mock.module('../src/api/staff', () => ({ staffApi: {} }));
const { CheckCard } = await import('../src/components/ReviewCheck');
const { ReviewActions } = await import('../src/components/ReviewActions');
const { TimelineRow } = await import('../src/components/ReviewTimeline');
const render = renderToStaticMarkup;
const context = (actions: ReviewContext['allowed_actions']): ReviewContext => ({
  decision_kind: 'CHECKS', allowed_actions: actions, requires_second_approval: true,
  pending_second_approval_id: null, check_counts: {},
});

describe('server review policy in the admin UI', () => {
  test('second approval badge follows the server, including a new code', () => {
    const html = render(createElement(CheckCard, { open: true, c: { check_code: 'NEW_POLICY_HOLD', status: 'REVIEW_REQUIRED', detail: '', needs_second_approval: true } }));
    expect(html).toContain('2인 승인 필요');
    const allowed = render(createElement(CheckCard, { open: true, c: { check_code: 'NEW_POLICY_HOLD', status: 'REVIEW_REQUIRED', detail: '', needs_second_approval: false } }));
    expect(allowed).not.toContain('2인 승인 필요');
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
