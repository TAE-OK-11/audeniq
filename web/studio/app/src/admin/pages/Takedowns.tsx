// 발매 중단 요청 — 이미 플랫폼에 나간 발매를 아티스트가 내려 달라고 한 건.
// 승인하면 그 발매가 나간 모든 플랫폼에 중단 메시지를 보낸다. 중단 메시지를 받지 않는 플랫폼은
// 플랫폼 관리 화면에서 내려간 것을 확인한 뒤 '내려감 확인'으로 기록한다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useConfirm } from '../../components/Confirm';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type TakedownItem } from '../api';
import { ago, dspLabel } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';

const STATUS: Record<TakedownItem['status'], { label: string; tone: 'blue' | 'green' | 'red' | 'gray' }> = {
  PENDING: { label: '확인 대기', tone: 'blue' },
  APPROVED: { label: '승인', tone: 'green' },
  DECLINED: { label: '반려', tone: 'red' },
  CANCELLED: { label: '아티스트 취소', tone: 'gray' },
};

const LIVE: Record<string, string> = {
  LIVE: '서비스 중', INGESTING: '등록 중', UNKNOWN: '전송 완료', TAKEDOWN_REQUESTED: '중단 요청함', TAKEN_DOWN: '내려감',
};

export function Takedowns() {
  const toast = useToast();
  const confirm = useConfirm();
  const { can } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.takedowns(), []);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const items = data?.items ?? [];

  const run = async (key: string, fn: () => Promise<unknown>, done: string) => {
    setBusy(key);
    try {
      await fn();
      toast(done, 'success');
      reload();
    } catch (e) {
      toast(errorMessage(e, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy('');
    }
  };

  const approve = async (t: TakedownItem) => {
    const up = t.platforms.filter(p => p.live_status !== 'TAKEN_DOWN');
    const ok = await confirm({
      title: '발매 중단을 승인할까요?',
      message: `‘${t.title}’을(를) ${up.map(p => p.name || dspLabel(p.slug)).join(', ') || '모든 플랫폼'}에서 내리도록 요청해요. 되돌리려면 다시 발매 신청을 받아야 해요.`,
      confirmLabel: '승인하고 중단 요청',
      danger: true,
    });
    if (ok) await run(t.id, () => staffApi.approveTakedown(t.id, notes[t.id] ?? ''), '플랫폼에 중단을 요청했어요.');
  };
  const decline = (t: TakedownItem) => {
    const note = (notes[t.id] ?? '').trim();
    if (!note) { toast('반려 사유를 적어 주세요. 아티스트에게 그대로 보여요.', 'error'); return; }
    void run(t.id, () => staffApi.declineTakedown(t.id, note), '요청을 반려했어요.');
  };
  const markDown = async (t: TakedownItem, partner: string, name: string) => {
    const ok = await confirm({
      title: `${name}에서 내려간 것을 확인했나요?`,
      message: '플랫폼 관리 화면에서 직접 확인한 경우에만 기록해 주세요. 확인 근거는 메모로 남아요.',
      confirmLabel: '내려감으로 기록',
    });
    if (!ok) return;
    await run(`${t.id}:${partner}`, () => staffApi.markTakenDown(t.package_id, partner, notes[t.id]?.trim() || `${name} 관리 화면에서 내려간 것을 확인`), '내려감으로 기록했어요.');
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="발매 중단 요청"
        sub="이미 플랫폼에 나간 발매를 내려 달라는 요청이에요. 승인하면 나간 모든 플랫폼에 중단을 요청해요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!can('DELIVERY') && <NoDuty duty="배급" />}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="발매 중단 요청이 없어요">아티스트가 발매 상세에서 중단을 요청하면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(t => (
            <div key={t.id} className="ui-card">
              <div className="adm-check-top">
                <span>
                  <Link to={`/admin/reviews/${t.release_id}`} className="ui-row-title">{t.title} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link>
                  <span className="ui-row-meta"><span>{t.org_name}</span><span>요청 {ago(t.created_at)}</span></span>
                </span>
                <Badge tone={STATUS[t.status].tone}>{STATUS[t.status].label}</Badge>
              </div>
              <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
                <small className="muted">중단 사유</small>
                <p>{t.reason}</p>
                {t.decision_note && <><small className="muted">처리 메모</small><p>{t.decision_note}</p></>}
              </div>
              <ul className="aq-linked-docs" style={{ marginTop: 12 }}>
                {t.platforms.map(p => {
                  const name = p.name || dspLabel(p.slug);
                  return (
                    <li key={p.partner_id}>
                      <span>{name}</span>
                      <span className="row-actions">
                        <em>{LIVE[p.live_status ?? 'UNKNOWN'] ?? '전송 완료'}</em>
                        {t.status === 'APPROVED' && p.live_status !== 'TAKEN_DOWN' && (
                          <button type="button" className="ui-btn is-soft is-sm" disabled={!can('DELIVERY') || busy === `${t.id}:${p.partner_id}`} onClick={() => markDown(t, p.partner_id, name)}>내려감 확인</button>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
              {(t.status === 'PENDING' || t.status === 'APPROVED') && (
                <div className="ui-field" style={{ marginTop: 12 }}>
                  <label htmlFor={`td-note-${t.id}`}>{t.status === 'PENDING' ? '처리 메모 (반려하면 아티스트에게 보여요)' : '확인 메모 (내려감 기록 근거)'}</label>
                  <textarea id={`td-note-${t.id}`} rows={2} maxLength={1000} value={notes[t.id] ?? ''} onChange={e => setNotes(n => ({ ...n, [t.id]: e.target.value }))} />
                </div>
              )}
              {t.status === 'PENDING' && (
                <div className="adm-form-actions">
                  <button type="button" className="ui-btn is-soft" disabled={!can('DELIVERY') || busy === t.id} onClick={() => decline(t)}>반려</button>
                  <button type="button" className="ui-btn is-danger" disabled={!can('DELIVERY') || busy === t.id} onClick={() => approve(t)}>승인하고 중단 요청</button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
