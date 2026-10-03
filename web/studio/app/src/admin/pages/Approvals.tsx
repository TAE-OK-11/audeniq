// 2차 승인 — 권리·중복·보호명 등 민감 항목을 다른 심사 담당자가 한 번 더 확인한다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useConfirm } from '../../components/Confirm';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type ApprovalItem } from '../api';
import { ago, checkLabel, shortId, when } from '../labels';
import { Chip, Empty, ErrorBox, NoDuty, PageHead, Skeleton, SubTabs, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';

export function Approvals() {
  const toast = useToast();
  const confirm = useConfirm();
  const { me, can, refreshCounts, counts } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.approvals(), []);
  const [busy, setBusy] = useState('');
  const items = data?.items ?? [];

  const act = async (a: ApprovalItem, approve: boolean) => {
    const ok = await confirm(approve
      ? { title: '2차 승인할까요?', message: <>‘{a.title}’의 {a.check_codes.map(checkLabel).join(', ')} 항목을 통과 처리해요. 시스템이 다음 단계를 이어서 해요. 요청 근거를 직접 확인했는지 다시 한번 봐 주세요.</>, confirmLabel: '승인' }
      : { title: '2차 승인을 반려할까요?', message: '요청이 닫히고 발매는 심사 대기 상태로 남아요. 담당자는 보완 요청이나 거절로 다시 결정할 수 있어요.', confirmLabel: '반려', danger: true });
    if (!ok) return;
    setBusy(a.id);
    try {
      if (approve) await staffApi.approve(a.id);
      else await staffApi.decline(a.id);
      toast(approve ? '2차 승인했어요. 시스템이 다음 단계로 넘겨요.' : '2차 승인 요청을 반려했어요.', 'success');
      reload();
      refreshCounts();
    } catch (e) {
      toast(errorMessage(e, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy('');
    }
  };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="발매 심사"
        title="2차 승인"
        sub="중복 음원·권리 같은 민감 항목은 두 사람이 확인해요. 내가 올린 요청은 다른 담당자가 처리해요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={[{ to: '/admin/reviews', label: '심사 목록', count: counts?.review }, { to: '/admin/approvals', label: '2차 승인', count: counts?.second_approvals }]} />
      {!can('REVIEW') && <NoDuty duty="발매 심사" />}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? (
        <Empty title="대기 중인 2차 승인이 없어요">민감 항목이 승인되면 이곳에 확인 요청이 올라와요.</Empty>
      ) : (
        <div className="adm-list">
          {items.map(a => {
            const mine = a.requested_by === me.user_id;
            return (
              <div key={a.id} className="adm-card">
                <div className="adm-check-top">
                  <span>
                    <Link to={`/admin/reviews/${a.release_id}`} className="adm-row-title">{a.title} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link>
                    <span className="adm-row-meta"><span>요청 {ago(a.at)}</span><span>만료 {when(a.expires_at)}</span><span>요청자 {mine ? '나' : shortId(a.requested_by)}</span></span>
                  </span>
                  <span className="adm-codes">{a.check_codes.map(c => <Chip key={c} tone="red">{checkLabel(c)}</Chip>)}</span>
                </div>
                <div className="adm-note" style={{ marginTop: 12 }}>
                  <small className="muted">요청 근거</small>
                  <p>{a.reason}</p>
                </div>
                <div className="adm-form-actions">
                  {mine && <span className="small muted" style={{ alignSelf: 'center' }}>내가 올린 요청은 다른 담당자가 승인해야 해요.</span>}
                  <button type="button" className="adm-btn danger" disabled={!can('REVIEW') || busy === a.id} onClick={() => act(a, false)}>반려</button>
                  <button type="button" className="adm-btn primary" disabled={!can('REVIEW') || mine || busy === a.id} onClick={() => act(a, true)}>2차 승인</button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
