// 권리침해 신고 — 누구나 접수한 신고를 확인하고 처리한다.
// 접수 → 반려(아티스트에게 알리지 않음) 또는 배급 중단(해당 발매가 나간 모든 플랫폼에 중단 요청, 아티스트가 30일 안에 소명)
// → 소명을 보고 복구(신고 불인정, 배급 화면에서 다시 송출) 또는 유지(신고 인정).
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useConfirm } from '../../components/Confirm';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type InfringementClaim } from '../api';
import { ago, dspLabel } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

const STATUS: Record<InfringementClaim['status'], { label: string; tone: 'blue' | 'green' | 'red' | 'gray' | 'amber' }> = {
  RECEIVED: { label: '접수', tone: 'blue' },
  REJECTED: { label: '반려', tone: 'gray' },
  SUSPENDED: { label: '배급 중단 · 소명 대기', tone: 'red' },
  COUNTER_NOTICED: { label: '소명 도착', tone: 'amber' },
  RESTORED: { label: '복구', tone: 'green' },
  UPHELD: { label: '신고 인정', tone: 'red' },
};
const RIGHT: Record<string, string> = {
  COPYRIGHT: '저작권', NEIGHBORING: '저작인접권', TRADEMARK: '상표권', PUBLICITY: '초상·성명', OTHER: '기타',
};
const LIVE: Record<string, string> = {
  LIVE: '서비스 중', INGESTING: '등록 중', UNKNOWN: '전송 완료', TAKEDOWN_REQUESTED: '중단 요청함', TAKEN_DOWN: '내려감',
};
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;
const day = (v: string | null) => (v ? new Date(v).toLocaleDateString('ko-KR', { timeZone: 'Asia/Seoul' }) : '');

export function Claims() {
  const toast = useToast();
  const confirm = useConfirm();
  const { can } = useStaff();
  const [all, setAll] = useState(false);
  const { data, loading, error, reload } = useAsync(() => staffApi.claims(all), [all]);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [releases, setReleases] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState('');
  const items = data?.items ?? [];
  const note = (c: InfringementClaim) => (notes[c.id] ?? '').trim();

  const run = async (c: InfringementClaim, fn: () => Promise<unknown>, done: string) => {
    setBusy(c.id);
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
  const reject = (c: InfringementClaim) => {
    if (!note(c)) { toast('반려 사유를 적어 주세요. 신고자에게 연락할 때 근거로 남아요.', 'error'); return; }
    void run(c, () => staffApi.rejectClaim(c.id, note(c)), '신고를 반려했어요.');
  };
  const suspend = async (c: InfringementClaim) => {
    const release = UUID.exec(releases[c.id] ?? '')?.[0];
    if (!release) { toast('중단할 발매의 ID(검토 화면 주소)를 넣어 주세요.', 'error'); return; }
    const ok = await confirm({
      title: '이 발매의 배급을 중단할까요?',
      message: '발매가 나간 모든 플랫폼에 중단을 요청하고, 아티스트에게 신고 내용과 소명 기한(30일)을 알려요. 신고자 연락처는 아티스트에게 보이지 않아요.',
      confirmLabel: '배급 중단', danger: true,
    });
    if (ok) await run(c, () => staffApi.suspendClaim(c.id, release, note(c)), '배급 중단을 요청했어요.');
  };
  const restore = async (c: InfringementClaim) => {
    const ok = await confirm({
      title: '신고를 받아들이지 않고 복구할까요?',
      message: '아티스트에게 알려요. 플랫폼에 다시 보내는 것은 배급 현황에서 다시 송출로 진행해 주세요.',
      confirmLabel: '복구',
    });
    if (ok) await run(c, () => staffApi.restoreClaim(c.id, note(c)), '복구로 처리했어요.');
  };
  const uphold = (c: InfringementClaim) => {
    if (!note(c)) { toast('처리 사유를 적어 주세요. 아티스트에게 그대로 보여요.', 'error'); return; }
    void run(c, () => staffApi.upholdClaim(c.id, note(c)), '신고를 인정했어요. 배급 중단이 유지돼요.');
  };

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="권리침해 신고"
        sub="누구나 접수할 수 있는 신고예요 (스튜디오 /rights-report). 신고자 연락처는 여기서만 보여요."
        actions={<>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => setAll(v => !v)}>{all ? '처리 중인 것만' : '지난 신고까지'}</button>
          <button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>
        </>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!can('DELIVERY') && <NoDuty duty="배급" />}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="처리할 권리침해 신고가 없어요">신고가 접수되면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(c => (
            <div key={c.id} className="ui-card">
              <div className="adm-check-top">
                <span>
                  <span className="ui-row-title">{c.receipt} · {RIGHT[c.right_type] ?? c.right_type}</span>
                  <span className="ui-row-meta">
                    <span>{c.claimant.name} ({c.claimant.role === 'AGENT' ? '대리인' : '권리자'})</span>
                    <span>{c.claimant.email}{c.claimant.phone ? ` · ${c.claimant.phone}` : ''}</span>
                    <span>접수 {ago(c.created_at)}</span>
                  </span>
                </span>
                <Badge tone={STATUS[c.status].tone}>{STATUS[c.status].label}</Badge>
              </div>
              <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
                <small className="muted">권리 대상 저작물</small><p>{c.work}</p>
                <small className="muted">신고 대상</small><p>{c.target}</p>
                {c.evidence_urls.length > 0 && <>
                  <small className="muted">증빙 링크</small>
                  <p>{c.evidence_urls.map(u => <a key={u} href={u} target="_blank" rel="noopener noreferrer nofollow" style={{ display: 'block' }}>{u}</a>)}</p>
                </>}
                {c.release_id && <>
                  <small className="muted">대상 발매</small>
                  <p><Link to={`/admin/reviews/${c.release_id}`}>{c.release_title ?? c.release_id}</Link>{c.org_name ? ` · ${c.org_name}` : ''}</p>
                </>}
                {c.staff_note && <><small className="muted">처리 메모</small><p>{c.staff_note}</p></>}
                {c.counter_deadline && c.status === 'SUSPENDED' && <><small className="muted">소명 기한</small><p>{day(c.counter_deadline)}</p></>}
                {c.counter_notice && <><small className="muted">아티스트 소명 ({day(c.counter_at)})</small><p>{c.counter_notice}</p></>}
              </div>
              {c.platforms && c.platforms.length > 0 && (
                <ul className="aq-linked-docs" style={{ marginTop: 12 }}>
                  {c.platforms.map(p => (
                    <li key={p.partner_id}><span>{p.name || dspLabel(p.slug)}</span><em>{LIVE[p.live_status ?? 'UNKNOWN'] ?? '전송 완료'}</em></li>
                  ))}
                </ul>
              )}
              {c.status === 'RECEIVED' && (
                <div className="ui-field" style={{ marginTop: 12 }}>
                  <label htmlFor={`claim-rel-${c.id}`}>중단할 발매</label>
                  <input id={`claim-rel-${c.id}`} className="ui-input" value={releases[c.id] ?? ''} onChange={e => setReleases(r => ({ ...r, [c.id]: e.target.value }))} placeholder="심사 화면 주소 또는 발매 ID" />
                </div>
              )}
              {['RECEIVED', 'SUSPENDED', 'COUNTER_NOTICED'].includes(c.status) && (
                <>
                  <div className="ui-field" style={{ marginTop: 12 }}>
                    <label htmlFor={`claim-note-${c.id}`}>처리 메모{c.status === 'RECEIVED' ? <span className="aq-optional">반려하면 필수</span> : <span className="aq-optional">아티스트에게 보여요</span>}</label>
                    <textarea id={`claim-note-${c.id}`} rows={2} maxLength={1000} value={notes[c.id] ?? ''} onChange={e => setNotes(n => ({ ...n, [c.id]: e.target.value }))} />
                  </div>
                  <div className="adm-form-actions">
                    {c.status === 'RECEIVED' ? <>
                      <button type="button" className="ui-btn is-soft" disabled={!can('DELIVERY') || busy === c.id} onClick={() => reject(c)}>반려</button>
                      <button type="button" className="ui-btn is-danger" disabled={!can('DELIVERY') || busy === c.id} onClick={() => void suspend(c)}>배급 중단</button>
                    </> : <>
                      <button type="button" className="ui-btn is-soft" disabled={!can('DELIVERY') || busy === c.id} onClick={() => void restore(c)}>복구 (신고 불인정)</button>
                      <button type="button" className="ui-btn is-danger" disabled={!can('DELIVERY') || busy === c.id} onClick={() => uphold(c)}>신고 인정</button>
                    </>}
                  </div>
                </>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
