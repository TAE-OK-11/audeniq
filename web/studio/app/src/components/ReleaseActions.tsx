// 발매 후 배급 중단·계약 종료·유통사 이전 요청 — 배급·권리 탭 (공개된 발매에서만)
// 요청 → 담당자 확인(권한·계약) → 플랫폼별 내림 → 플랫폼 확인 또는 담당자 증빙으로 완료
import { useState } from 'react';
import { useAsync } from '../hooks/useAsync';
import { useToast } from './Toast';
import { useConfirm } from './Confirm';
import { Segmented } from './Segmented';
import { DateField } from './Picker';
import { errorMessage } from '../api/errors';
import { dspLabel } from '../lib/catalog';
import { localStamp } from '../lib/format';
import { Badge } from '../ui';
import {
  cancelReleaseAction, listReleaseActions, requestReleaseAction, transferPacket,
  type ReleaseAction, type ReleaseActionKind, type TransferPacket,
} from '../api/releaseActions';

const KIND_LABEL: Record<ReleaseActionKind, string> = {
  TAKEDOWN: '배급 중단',
  TERMINATION: '계약 종료',
  TRANSFER: '유통사 이전',
};
const KIND_NOTE: Record<ReleaseActionKind, string> = {
  TAKEDOWN: '모든 플랫폼에서 발매를 내려요. 계약은 그대로 유지돼요.',
  TERMINATION: '이 발매의 배급 계약을 끝내고 모든 플랫폼에서 내려요. 끝난 뒤에는 다시 보낼 수 없어요.',
  TRANSFER: '다른 유통사로 옮겨요. 정한 날짜에 플랫폼에서 내리고, 새 유통사에 넘길 UPC·ISRC 목록을 드려요.',
};
const STATUS_LABEL: Record<string, string> = {
  REQUESTED: '담당자 확인 중',
  IN_PROGRESS: '플랫폼에서 내리는 중',
  COMPLETED: '완료',
  REJECTED: '반려',
  CANCELLED: '취소함',
};
const TASK_LABEL: Record<string, string> = {
  QUEUED: '대기',
  SENT: '플랫폼에 요청함',
  MANUAL_REQUIRED: '담당자가 직접 처리 중',
  FAILED: '담당자 확인 중',
  CONFIRMED: '내려감',
};
const OPTIONS = (Object.keys(KIND_LABEL) as ReleaseActionKind[]).map(k => ({ value: k, label: KIND_LABEL[k] }));

function todayKst(): string {
  return new Date(Date.now() + 9 * 3_600_000).toISOString().slice(0, 10);
}

export function ReleaseActions({ releaseId }: { releaseId: string }) {
  const toast = useToast();
  const confirm = useConfirm();
  const list = useAsync<ReleaseAction[]>(() => listReleaseActions(releaseId), [releaseId]);
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<ReleaseActionKind>('TAKEDOWN');
  const [reason, setReason] = useState('');
  const [transferTo, setTransferTo] = useState('');
  const [date, setDate] = useState('');
  const [busy, setBusy] = useState(false);
  const [packet, setPacket] = useState<TransferPacket | null>(null);

  const actions = list.data ?? [];
  const pending = actions.find(a => a.status === 'REQUESTED' || a.status === 'IN_PROGRESS');
  const ended = actions.some(a => a.status === 'COMPLETED' && a.kind !== 'TAKEDOWN');

  const submit = async () => {
    if (!reason.trim()) { toast('요청 사유를 적어 주세요.'); return; }
    if (kind === 'TRANSFER' && (!transferTo.trim() || !date)) { toast('이전할 유통사와 날짜를 정해 주세요.'); return; }
    const ok = await confirm({
      title: `${KIND_LABEL[kind]}을 요청할까요?`,
      message: `${KIND_NOTE[kind]} 담당자가 권한과 계약을 확인한 뒤 진행하고, 결과는 알림으로 알려 드려요.`,
      confirmLabel: '요청하기',
      danger: kind !== 'TRANSFER',
    });
    if (!ok) return;
    setBusy(true);
    try {
      await requestReleaseAction(releaseId, {
        kind, reason: reason.trim(),
        ...(kind === 'TRANSFER' ? { transfer_to: transferTo.trim(), effective_date: date } : {}),
      });
      toast('요청을 접수했어요. 담당자가 확인한 뒤 알려 드려요.', 'success');
      setOpen(false);
      setReason('');
      list.reload();
    } catch (e) {
      toast(errorMessage(e, '요청하지 못했어요. 잠시 후 다시 시도해 주세요.'));
    } finally {
      setBusy(false);
    }
  };

  const cancel = async (a: ReleaseAction) => {
    const ok = await confirm({ title: '요청을 취소할까요?', message: `${KIND_LABEL[a.kind]} 요청을 취소해요.`, confirmLabel: '요청 취소' });
    if (!ok) return;
    try {
      await cancelReleaseAction(releaseId, a.id);
      list.reload();
    } catch (e) {
      toast(errorMessage(e, '취소하지 못했어요.'));
    }
  };

  const showPacket = async (a: ReleaseAction) => {
    try {
      setPacket(await transferPacket(releaseId, a.id));
    } catch (e) {
      toast(errorMessage(e, '이전 자료를 불러오지 못했어요.'));
    }
  };

  return (
    <section className="aq-release-actions" aria-labelledby="aqReleaseActionsHead">
      <h2 className="subhead" id="aqReleaseActionsHead">배급 중단·이전</h2>
      {actions.length > 0 && (
        <ul className="aq-linked-docs">
          {actions.map(a => (
            <li key={a.id}>
              <div className="min-0">
                <span>{KIND_LABEL[a.kind]}{a.transfer_to ? ` → ${a.transfer_to}` : ''}{a.effective_date ? ` · ${a.effective_date}` : ''}</span>
                <p className="small muted">{localStamp(a.created_at)} 요청{a.source === 'STAFF' ? ' · 담당자 접수' : ''}</p>
                {a.status === 'REJECTED' && a.decision_note && <p className="small muted break">{a.decision_note}</p>}
                {a.tasks.length > 0 && (
                  <p className="small muted">
                    {a.tasks.map(t => `${dspLabel(t.partner_id)} ${TASK_LABEL[t.status] ?? t.status}`).join(' · ')}
                  </p>
                )}
                <div className="row-actions">
                  {a.status === 'REQUESTED' && a.source === 'ARTIST' && (
                    <button type="button" className="ui-btn is-text is-sm" onClick={() => cancel(a)}>요청 취소</button>
                  )}
                  {a.kind === 'TRANSFER' && (a.status === 'IN_PROGRESS' || a.status === 'COMPLETED') && (
                    <button type="button" className="ui-btn is-white is-sm" onClick={() => showPacket(a)}>이전 자료 보기</button>
                  )}
                </div>
              </div>
              <em><Badge tone={a.status === 'COMPLETED' ? 'green' : a.status === 'REJECTED' ? 'red' : undefined}>{STATUS_LABEL[a.status] ?? a.status}</Badge></em>
            </li>
          ))}
        </ul>
      )}

      {packet && (
        <div className="ui-card aq-transfer-packet">
          <h3 className="aq-bundle-sub">새 유통사에 넘길 자료</h3>
          <p className="small muted">UPC·ISRC를 그대로 쓰면 플랫폼의 재생 기록과 순위가 이어져요.</p>
          <dl className="ui-kv is-2">
            <div><dt>앨범</dt><dd>{packet.title}</dd></div>
            <div><dt>UPC</dt><dd>{packet.upc || '없음'}</dd></div>
            <div><dt>이전할 곳</dt><dd>{packet.transfer_to}</dd></div>
            <div><dt>이전 날짜</dt><dd>{packet.effective_date}</dd></div>
          </dl>
          <ul className="small">
            {packet.tracks.map(t => <li key={`${t.disc_number}-${t.track_number}`}>{t.track_number}. {t.title}{t.version ? ` (${t.version})` : ''} — ISRC {t.isrc || '없음'}</li>)}
          </ul>
          <ul className="small muted">
            {packet.platforms.map(p => <li key={p.partner_id}>{dspLabel(p.partner_id)}{p.partner_release_id ? ` · ${p.partner_release_id}` : ''}</li>)}
          </ul>
          <button type="button" className="ui-btn is-text is-sm" onClick={() => setPacket(null)}>닫기</button>
        </div>
      )}

      {ended ? (
        <p className="small muted">계약 종료·이전으로 이 발매의 배급이 끝났어요.</p>
      ) : pending ? (
        <p className="small muted">진행 중인 요청이 끝나면 새로 요청할 수 있어요.</p>
      ) : !open ? (
        <button type="button" className="ui-btn is-text aq-danger-text" onClick={() => setOpen(true)}>배급 중단·계약 종료·이전 요청</button>
      ) : (
        <div className="ui-card">
          <Segmented value={kind} options={OPTIONS} onChange={setKind} label="요청 종류" />
          <p className="small muted">{KIND_NOTE[kind]}</p>
          {kind === 'TRANSFER' && (
            <>
              <div className="ui-field">
                <label htmlFor="raTransferTo">이전할 유통사 <span className="required">*</span></label>
                <input id="raTransferTo" maxLength={200} value={transferTo} onChange={e => setTransferTo(e.target.value)} />
              </div>
              <div className="ui-field">
                <label htmlFor="raDate">이전 날짜 <span className="required">*</span></label>
                <DateField id="raDate" required min={todayKst()} value={date} onChange={setDate} />
                <p className="small muted">새 유통사에서 발매가 올라가는 날로 정하면 공백 없이 옮길 수 있어요.</p>
              </div>
            </>
          )}
          <div className="ui-field">
            <label htmlFor="raReason">요청 사유 <span className="required">*</span></label>
            <textarea id="raReason" rows={3} maxLength={2000} value={reason} onChange={e => setReason(e.target.value)} />
          </div>
          <div className="row-actions">
            <button type="button" className="ui-btn is-soft" onClick={() => setOpen(false)} disabled={busy}>닫기</button>
            <button type="button" className="ui-btn is-primary" onClick={submit} disabled={busy}>{busy ? '요청하는 중' : '요청하기'}</button>
          </div>
        </div>
      )}
    </section>
  );
}
