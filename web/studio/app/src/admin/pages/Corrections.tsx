// 발매 후 정보 수정 요청 — 플랫폼에 나간 발매의 크레딧·장르 수정. 승인하면 그 발매가 나간 모든 플랫폼에
// 수정 메시지를 보낸다 (보관된 발매 스냅샷은 그대로 두고, 나가는 정보에만 수정을 얹는다).
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useConfirm } from '../../components/Confirm';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type CorrectionItem } from '../api';
import { ago } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

const STATUS: Record<CorrectionItem['status'], { label: string; tone: 'blue' | 'green' | 'red' | 'gray' }> = {
  PENDING: { label: '확인 대기', tone: 'blue' },
  APPROVED: { label: '승인 · 플랫폼 전송', tone: 'green' },
  DECLINED: { label: '반려', tone: 'red' },
  CANCELLED: { label: '아티스트 취소', tone: 'gray' },
};
const ROLE_KO: Record<string, string> = {
  Composer: '작곡', Lyricist: '작사', Arranger: '편곡', Producer: '프로듀서', 'Mixing Engineer': '믹싱',
  'Mastering Engineer': '마스터링', 'Recording Engineer': '녹음', Musician: '연주',
};

export function Corrections() {
  const toast = useToast();
  const confirm = useConfirm();
  const { can } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.corrections(), []);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const items = data?.items ?? [];

  const run = async (c: CorrectionItem, fn: () => Promise<unknown>, done: string) => {
    setBusy(c.id);
    try { await fn(); toast(done, 'success'); reload(); } catch (e) { toast(errorMessage(e, '처리하지 못했어요.'), 'error'); } finally { setBusy(''); }
  };
  const approve = async (c: CorrectionItem) => {
    const ok = await confirm({
      title: '정보 수정을 승인할까요?',
      message: `‘${c.title}’이(가) 나간 모든 플랫폼에 수정 내용을 보내요.`,
      confirmLabel: '승인하고 전송',
    });
    if (ok) await run(c, () => staffApi.approveCorrection(c.id, notes[c.id] ?? ''), '플랫폼에 수정 내용을 보냈어요.');
  };
  const decline = (c: CorrectionItem) => {
    const note = (notes[c.id] ?? '').trim();
    if (!note) { toast('반려 사유를 적어 주세요. 아티스트에게 그대로 보여요.', 'error'); return; }
    void run(c, () => staffApi.declineCorrection(c.id, note), '요청을 반려했어요.');
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="발매 후 정보 수정 요청"
        sub="플랫폼에 나간 발매의 크레딧·장르 수정이에요. 제목·아티스트·음원은 바꿀 수 없어요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!can('REVIEW') && <NoDuty duty="심사" />}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="정보 수정 요청이 없어요">아티스트가 발매 상세에서 수정을 요청하면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(c => {
            const title = (tid: string) => c.tracks.find(t => t.track_id === tid)?.title ?? tid;
            return (
              <div key={c.id} className="ui-card">
                <div className="adm-check-top">
                  <span>
                    <Link to={`/admin/reviews/${c.release_id}`} className="ui-row-title">{c.title}</Link>
                    <span className="ui-row-meta"><span>{c.org_name}</span><span>요청 {ago(c.created_at)}</span></span>
                  </span>
                  <Badge tone={STATUS[c.status].tone}>{STATUS[c.status].label}</Badge>
                </div>
                <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
                  <small className="muted">수정 이유</small><p>{c.reason}</p>
                  {c.changes.genre && <><small className="muted">장르</small><p>{c.changes.genre}</p></>}
                  {(c.changes.tracks ?? []).map(t => (
                    <div key={t.track_id}>
                      <small className="muted">{title(t.track_id)} 크레딧</small>
                      <p>{t.credits.length ? t.credits.map(cr => `${cr.name} (${ROLE_KO[cr.role] ?? cr.role})`).join(', ') : '크레딧 없음'}</p>
                    </div>
                  ))}
                  {c.decision_note && <><small className="muted">처리 메모</small><p>{c.decision_note}</p></>}
                </div>
                {c.status === 'PENDING' && (
                  <>
                    <div className="ui-field" style={{ marginTop: 12 }}>
                      <label htmlFor={`mc-note-${c.id}`}>처리 메모 (반려하면 아티스트에게 보여요)</label>
                      <textarea id={`mc-note-${c.id}`} rows={2} maxLength={1000} value={notes[c.id] ?? ''} onChange={e => setNotes(n => ({ ...n, [c.id]: e.target.value }))} />
                    </div>
                    <div className="adm-form-actions">
                      <button type="button" className="ui-btn is-soft" disabled={!can('REVIEW') || busy === c.id} onClick={() => decline(c)}>반려</button>
                      <button type="button" className="ui-btn is-primary" disabled={!can('REVIEW') || busy === c.id} onClick={() => void approve(c)}>승인하고 전송</button>
                    </div>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
