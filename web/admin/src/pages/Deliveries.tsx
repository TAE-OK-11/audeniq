// 배급 현황 — 배급은 발매 심사 최종 승인 + 아티스트 서명 뒤 시스템이 알아서 보낸다.
// 담당자는 여기서 플랫폼별 진행과 문제만 확인하고, 필요하면 멈추거나(보류) 다시 검사한다.
import { useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { Modal, useModalClose } from '../components/Modal';
import { useToast } from '../components/Toast';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { staffApi, type DeliveryItem } from '../api/staff';
import { ago } from '../labels';
import { DSP_NAME, deliveryVerdict } from '../dspReqs';
import { Chip, Empty, ErrorBox, Filters, NoDuty, PageHead, Skeleton, SubTabs, useStaff } from '../ui';
import { Glyph } from '../components/Glyph';

const VIEWS = [
  { value: 'PENDING', label: '확인 필요' },
  { value: 'APPROVED', label: '자동 배급 중' },
  { value: 'HELD', label: '멈춤' },
] as const;

function HoldForm({ item, onDone }: { item: DeliveryItem; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || !note.trim()) return;
    setBusy(true);
    try {
      await staffApi.decideDelivery(item.package_id, item.dsp, { action: 'HOLD', note: note.trim() });
      toast(`${DSP_NAME[item.dsp] ?? item.dsp} 배급을 멈췄어요.`, 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '멈추지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">{item.title} · {DSP_NAME[item.dsp] ?? item.dsp} 배급을 멈춰요. 멈춘 플랫폼은 자동으로 보내지 않아요.</p>
      <div className="adm-field" style={{ marginTop: 14 }}>
        <label htmlFor="holdNote">멈추는 이유 <span className="required">*</span></label>
        <input id="holdNote" data-autofocus className="adm-input" maxLength={1000} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 권리 확인 중" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn warn" disabled={busy || !note.trim()}>{busy ? '처리 중…' : '배급 멈추기'}</button>
      </div>
    </form>
  );
}

export function Deliveries() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const view = params.get('approval') || 'PENDING';
  const { can, refreshCounts } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.deliveries({ approval: view }), [view]);
  const [hold, setHold] = useState<DeliveryItem | null>(null);
  const items = data?.items ?? [];

  // 발매(패키지)별로 묶는다 — 한 발매의 플랫폼들을 한 카드에서
  const groups = new Map<string, DeliveryItem[]>();
  for (const d of items) groups.set(d.package_id, [...(groups.get(d.package_id) ?? []), d]);

  const recheck = async (pkg: string) => {
    try {
      await staffApi.restage(pkg);
      toast('다시 검사를 시작했어요. 잠시 후 새로고침해 주세요.', 'success');
    } catch (e) {
      toast(errorMessage(e, '다시 검사하지 못했어요.'), 'error');
    }
  };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="DELIVERY"
        title="배급 현황"
        sub="발매 심사에서 최종 승인하고 아티스트가 계약서에 서명하면 시스템이 플랫폼마다 자동으로 보내요. 여기서는 진행 상황과 문제만 확인하면 돼요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={[{ to: '/deliveries', label: '배급 현황' }, { to: '/dsps', label: '플랫폼별 조건' }]} />
      {!can('DELIVERY') && <NoDuty duty="배급 관리" />}
      <Filters label="보기" value={view} onChange={v => setParams({ approval: v }, { replace: true })} options={VIEWS.map(v => ({ value: v.value, label: v.label }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={4} /> : groups.size === 0 ? (
        <Empty icon={<Glyph name="truck" size={22} />} title={view === 'PENDING' ? '확인할 배급이 없어요' : '해당하는 배급이 없어요'}>
          {view === 'PENDING' ? '문제가 있거나 플랫폼 연동을 기다리는 배급이 여기에 모여요.' : undefined}
        </Empty>
      ) : (
        <div className="adm-list">
          {[...groups.values()].map(list => {
            const first = list[0];
            const verdicts = list.map(d => ({ d, v: deliveryVerdict(d) }));
            const bad = verdicts.filter(x => x.v.tone === 'red').length;
            return (
              <div key={first.package_id} className={`adm-card adm-deliv${bad ? ' is-bad' : ''}`}>
                <div className="adm-check-top">
                  <span className="adm-min">
                    <Link to={`/reviews/${first.release_id}`} className="adm-row-title">{first.title} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link>
                    <span className="adm-row-meta"><span>{first.org_name}</span><span>배급 준비 {ago(first.staged_at)}</span></span>
                  </span>
                  <span className="adm-codes">
                    {bad > 0 ? <Chip tone="red">문제 {bad}곳</Chip> : <Chip tone="green">문제 없음</Chip>}
                    {can('DELIVERY') && <button type="button" className="adm-btn soft small" onClick={() => recheck(first.package_id)}>다시 검사</button>}
                  </span>
                </div>
                <ul className="adm-deliv-rows">
                  {verdicts.map(({ d, v }) => (
                    <li key={d.dsp} className={`is-${v.tone}`}>
                      <b className="adm-deliv-name">{DSP_NAME[d.dsp] ?? d.dsp}</b>
                      <span className="adm-min">
                        <span className="adm-deliv-head">{d.approval === 'HELD' ? '멈춤' : d.approval === 'APPROVED' && v.tone !== 'red' ? (v.tone === 'gray' ? 'DSP 연동 대기 · 연동되면 자동 전송' : '서명 후 자동 전송') : v.headline}</span>
                        {v.problems.map(p => <small key={p} className="adm-deliv-problem">{p}</small>)}
                        {v.notes.length > 0 && <small className="adm-deliv-note">{v.notes.join(' · ')}</small>}
                      </span>
                      {can('DELIVERY') && d.approval !== 'HELD' && (
                        <button type="button" className="adm-link-btn" onClick={() => setHold(d)}>멈추기</button>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      )}
      {hold && (
        <Modal title="배급 멈추기" onClose={() => setHold(null)} dismissible={false}>
          <HoldForm item={hold} onDone={() => { reload(); refreshCounts(); }} />
        </Modal>
      )}
    </div>
  );
}
