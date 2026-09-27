// 배급 승인 — DSP별 패키지(ERN)를 확인하고 전송을 승인하거나 보류한다.
import { useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { sha256Hex, staffApi, type DeliveryItem } from '../api';
import { APPROVAL_STATUS, READINESS, ago, checkLabel, pick } from '../labels';
import { Chip, Empty, ErrorBox, Filters, NoDuty, PageHead, Skeleton, StatusChip, useStaff } from '../ui';

const APPROVALS = ['PENDING', 'HELD', 'APPROVED'];
const DSPS = Array.from({ length: 11 }, (_, i) => `D-${i + 1}`);
const ACK = ['DSP_LOUDNESS_ADVISORY', 'DSP_CLIPPING_ADVISORY'];

function DecideForm({ item, action, onDone }: { item: DeliveryItem; action: 'APPROVE' | 'HOLD'; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [ack, setAck] = useState(false);
  const [xml, setXml] = useState<string | null>(null);
  const [loadingXml, setLoadingXml] = useState(false);
  const [busy, setBusy] = useState(false);
  const needsAck = action === 'APPROVE' && item.warnings.some(w => ACK.includes(w));

  const viewErn = async () => {
    setLoadingXml(true);
    try { setXml(await staffApi.ern(item.package_id, item.dsp)); } catch (e) { toast(errorMessage(e, 'ERN을 불러오지 못했어요.'), 'error'); } finally { setLoadingXml(false); }
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (action === 'HOLD' && !note.trim()) || (needsAck && !ack)) return;
    setBusy(true);
    try {
      // 담당자가 본 ERN과 서버의 ERN이 같은지 확인 (중간에 다시 스테이징되면 409)
      const ern_sha256 = action === 'APPROVE' && xml ? await sha256Hex(xml) : undefined;
      await staffApi.decideDelivery(item.package_id, item.dsp, { action, note: note.trim(), ern_sha256, acknowledge_warnings: needsAck ? ack : undefined });
      toast(action === 'APPROVE' ? `${item.dsp} 전송을 승인했어요. 배급 대기열에 올렸어요.` : `${item.dsp} 전송을 보류했어요.`, 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit}>
      <p className="small muted">{item.title} · {item.org_name} · {item.dsp}{item.ern_is_preview ? ' · 미리보기 ERN(수신 DPID 미등록)' : ''}</p>
      {item.warnings.length > 0 && (
        <div className="adm-alert is-warn" style={{ marginTop: 14 }}>권고: {item.warnings.map(checkLabel).join(', ')}</div>
      )}
      <div className="adm-field" style={{ marginTop: 14 }}>
        <div className="adm-check-top">
          <span className="adm-field-label">ERN 3.8.2</span>
          <button type="button" className="adm-btn soft small" onClick={viewErn} disabled={loadingXml}>{loadingXml ? '불러오는 중…' : xml ? '다시 불러오기' : 'ERN 보기'}</button>
        </div>
        {xml ? <pre className="adm-ern">{xml}</pre> : <small>승인 전에 실제로 보낼 메시지를 확인하는 걸 권장해요. 확인한 ERN의 해시를 함께 보내 중간 변경을 막아요.</small>}
      </div>
      <div className="adm-field">
        <label htmlFor="dlNote">{action === 'HOLD' ? '보류 사유' : '메모 (선택)'}{action === 'HOLD' && <span className="required"> *</span>}</label>
        <textarea id="dlNote" className="adm-textarea" rows={3} maxLength={1000} value={note} required={action === 'HOLD'} onChange={e => setNote(e.target.value)} />
      </div>
      {needsAck && (
        <label className="adm-check-line">
          <input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} />
          <span>음량·클리핑 권고를 확인했고, 이 상태로 전송해도 된다고 판단했어요.</span>
        </label>
      )}
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className={`adm-btn ${action === 'HOLD' ? 'warn' : 'primary'}`} disabled={busy || (action === 'HOLD' && !note.trim()) || (needsAck && !ack)}>
          {busy ? '처리 중…' : action === 'HOLD' ? '보류' : '전송 승인'}
        </button>
      </div>
    </form>
  );
}

export function Deliveries() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const approval = params.get('approval') || 'PENDING';
  const dsp = params.get('dsp') || '';
  const { can, refreshCounts } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.deliveries({ approval, dsp: dsp || undefined }), [approval, dsp]);
  const [pending, setPending] = useState<{ item: DeliveryItem; action: 'APPROVE' | 'HOLD' } | null>(null);
  const items = data?.items ?? [];
  const setFilter = (k: string, v: string) => {
    const next = new URLSearchParams(params);
    if (v) next.set(k, v); else next.delete(k);
    setParams(next, { replace: true });
  };

  const restage = async (d: DeliveryItem) => {
    try {
      await staffApi.restage(d.package_id);
      toast('다시 스테이징을 예약했어요. 잠시 후 새로고침해 주세요.', 'success');
    } catch (e) {
      toast(errorMessage(e, '재스테이징을 예약하지 못했어요.'), 'error');
    }
  };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="DELIVERY"
        title="배급 승인"
        sub="배급 준비가 끝난 발매의 DSP별 패키지예요. ERN을 확인하고 전송을 승인하면 배급 대기열(E-0)에 올라가요. 콘텐츠 차단 건은 승인할 수 없어요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      {!can('DELIVERY') && <NoDuty duty="배급 승인" />}
      <div className="adm-check-top" style={{ alignItems: 'flex-start' }}>
        <Filters label="승인 상태" value={approval} onChange={v => setFilter('approval', v)} options={APPROVALS.map(s => ({ value: s, label: APPROVAL_STATUS[s][0] }))} />
        <label className="sr-only" htmlFor="dspFilter">DSP</label>
        <select id="dspFilter" className="adm-select" value={dsp} onChange={e => setFilter('dsp', e.target.value)}>
          <option value="">전체 DSP</option>
          {DSPS.map(d => <option key={d}>{d}</option>)}
        </select>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={4} /> : items.length === 0 ? (
        <Empty icon="🚚" title="해당 조건의 배급 건이 없어요" />
      ) : (
        <div className="adm-list">
          {items.map(d => {
            const blocked = d.readiness === 'CONTENT_BLOCKED';
            return (
              <div key={`${d.package_id}-${d.dsp}`} className="adm-row">
                <span className="adm-row-icon plain" aria-hidden="true" style={{ fontSize: 13 }}>{d.dsp}</span>
                <span className="adm-min">
                  <Link to={`/admin/reviews/${d.release_id}`} className="adm-row-title">{d.title}</Link>
                  <span className="adm-row-meta">
                    <span>{d.org_name}</span>
                    <span>{d.route_status ?? '경로 미정'}{d.route_reason ? ` · ${d.route_reason}` : ''}</span>
                    <span>스테이징 {ago(d.staged_at)}</span>
                  </span>
                  {(d.blockers.length > 0 || d.warnings.length > 0) && (
                    <span className="adm-codes" style={{ marginTop: 8 }}>
                      {d.blockers.map(b => <Chip key={b} tone="red">{checkLabel(b)}</Chip>)}
                      {d.warnings.map(w => <Chip key={w} tone="amber">{checkLabel(w)}</Chip>)}
                      {d.ern_is_preview && <Chip tone="blue">미리보기 ERN</Chip>}
                    </span>
                  )}
                </span>
                <span className="adm-row-end">
                  <span className="adm-codes"><StatusChip value={pick(READINESS, d.readiness)} /><StatusChip value={pick(APPROVAL_STATUS, d.approval)} /></span>
                  {can('DELIVERY') && (
                    <span className="adm-codes">
                      <button type="button" className="adm-btn soft small" onClick={() => restage(d)}>재스테이징</button>
                      {d.approval !== 'HELD' && <button type="button" className="adm-btn warn small" onClick={() => setPending({ item: d, action: 'HOLD' })}>보류</button>}
                      {d.approval !== 'APPROVED' && <button type="button" className="adm-btn primary small" disabled={blocked} title={blocked ? '콘텐츠 차단 항목이 있어 승인할 수 없어요' : undefined} onClick={() => setPending({ item: d, action: 'APPROVE' })}>승인</button>}
                    </span>
                  )}
                </span>
              </div>
            );
          })}
        </div>
      )}
      {pending && (
        <Modal title={pending.action === 'APPROVE' ? `${pending.item.dsp} 전송 승인` : `${pending.item.dsp} 전송 보류`} onClose={() => setPending(null)} dismissible={false}>
          <DecideForm item={pending.item} action={pending.action} onDone={() => { reload(); refreshCounts(); }} />
        </Modal>
      )}
    </div>
  );
}
