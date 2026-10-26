// 부가서비스 처리 — 발매 신청서에서 고른 뮤직비디오 배급·심의, 신속 발매, AI 가사 싱크 (서버 crates/core/src/addons.rs).
// 결제가 끝난 신청이 먼저, 급행·신속 발매가 그 위에. 시작 → 결과 전달 → 완료로 옮기고, 진행할 수 없으면 사유와 함께
// 진행 불가로 닫는다(결제한 금액은 자동 환불: 정산금은 원장 반대 기록, 카드는 토스 결제 취소).
// 뮤직비디오는 등급·등급분류번호와 플랫폼별 링크를, 가사 싱크는 곡마다 만든 LRC와 등록한 가사 서비스를 남긴다.
import { useState } from 'react';
import { Link } from '../../lib/router';
import { useToast } from '../../components/Toast';
import { useConfirm } from '../../components/Confirm';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { req } from '../../api/http';
import { MOCK } from '../../lib/mode';
import { money } from '../../lib/format';
import { PLATFORM_NAMES } from '../../lib/platformNames';
import { LYRIC_SERVICE_LABEL, RATING_LABEL, SERVICE_LABEL, STATUS_LABEL, STATUS_TONE, TIER, lrcLines, type AddonOrder } from '../../lib/addons';
import { ago } from '../labels';
import { Badge, EmptyState, ErrorBox, NoDuty, PageHeader, SkeletonList, DELIVERY_TABS, SubTabs, useStaff } from '../ui';

type StaffOrder = AddonOrder & { org_name: string };
const OPEN = ['SUBMITTED', 'IN_PROGRESS', 'DELIVERED'];
const PG_LABEL: Record<string, string> = { toss: '토스페이먼츠', portone: '포트원', payple: '페이플' };

export function Addons() {
  const toast = useToast();
  const { can } = useStaff();
  const [all, setAll] = useState(false);
  const [service, setService] = useState('');
  const { data, loading, error, reload } = useAsync(
    () => (MOCK ? Promise.resolve({ items: [] as StaffOrder[] })
      : req<{ items: StaffOrder[] }>(`/api/staff/addons?status=${all ? 'all' : 'open'}&service=${service}`)),
    [all, service],
  );
  const ok = can('DELIVERY');
  const items = data?.items ?? [];

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="배급"
        title="부가서비스"
        sub="결제가 끝난 신청부터 처리해요. 진행할 수 없으면 사유를 남기고 진행 불가로 닫으면 결제한 금액이 자동으로 환불돼요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {!ok && <NoDuty duty="배급" />}
      <div className="adm-filter-row" style={{ display: 'flex', gap: 8, flexWrap: 'wrap', margin: '0 0 12px' }}>
        <select className="ui-select" aria-label="서비스" value={service} onChange={e => setService(e.target.value)}>
          <option value="">전체</option>
          {Object.entries(SERVICE_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
        </select>
        <label className="small"><input type="checkbox" checked={all} onChange={e => setAll(e.target.checked)} /> 끝난 신청도 보기</label>
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState title="처리할 신청이 없어요">아티스트가 발매 신청서에서 부가서비스를 고르고 결제하면 이곳에 올라와요.</EmptyState>
      ) : (
        <div className="ui-list">
          {items.map(o => <OrderCard key={o.id} o={o} ok={ok} onDone={() => { reload(); }} toast={toast} />)}
        </div>
      )}
    </div>
  );
}

function OrderCard({ o, ok, onDone, toast }: { o: StaffOrder; ok: boolean; onDone: () => void; toast: ReturnType<typeof useToast> }) {
  const confirm = useConfirm();
  const [note, setNote] = useState('');
  const [rating, setRating] = useState(o.result.rating ?? '');
  const [ratingNo, setRatingNo] = useState(o.result.rating_no ?? '');
  const [links, setLinks] = useState<Record<string, string>>(o.result.links ?? {});
  const [busy, setBusy] = useState(false);
  const [lyricsOpen, setLyricsOpen] = useState(false);
  const d = o.detail as Record<string, string | string[]>;
  const open = OPEN.includes(o.status);
  const mv = o.service === 'MV_DISTRIBUTION';
  const platforms = Array.isArray(d.platforms) ? d.platforms : [];

  // 올린 영상은 10분짜리 내려받기 링크로 (공개 주소가 없다)
  const download = async () => {
    try { const r = await req<{ url: string }>(`/api/staff/addons/${o.id}/video`); window.open(r.url, '_blank', 'noopener'); }
    catch (e) { toast(errorMessage(e), 'error'); }
  };
  const move = async (status: string, done: string) => {
    if (status === 'REJECTED') {
      if (!note.trim()) { toast('아티스트에게 남길 사유를 적어 주세요.', 'error'); return; }
      const sure = await confirm({ title: '진행 불가로 닫을까요?', message: o.paid_at ? `결제한 ${money(o.amount)}을 ${o.pay_method === 'CARD' ? '카드 결제 취소로' : '정산금으로'} 환불해요.` : '아직 결제 전이에요.', confirmLabel: '진행 불가', danger: true });
      if (!sure) return;
    }
    setBusy(true);
    try {
      const result = mv ? { rating, rating_no: ratingNo, links: Object.fromEntries(Object.entries(links).filter(([, u]) => u.trim())) } : undefined;
      await req(`/api/staff/addons/${o.id}/status`, { method: 'POST', body: { status, note, ...(result ? { result } : {}) } });
      toast(done, 'success'); setNote(''); onDone();
    } catch (e) { toast(errorMessage(e, '처리하지 못했어요.'), 'error'); } finally { setBusy(false); }
  };

  return (
    <div className="ui-card">
      <div className="adm-check-top">
        <span>
          <span className="ui-row-title">
            {(o.service === 'EXPRESS_RELEASE' || o.tier === 'EXPRESS') && <Badge tone="red">급행</Badge>} {o.title}
          </span>
          <span className="ui-row-meta">
            <span>{SERVICE_LABEL[o.service]}{o.tier ? ` · ${TIER[o.tier].label}` : ''}</span>
            <span>{o.org_name}</span>
            {o.release_id && <Link to={`/admin/reviews/${o.release_id}`}>{o.release_title ?? '발매'}</Link>}
            <span>{money(o.amount)} · {o.pay_method ? (o.pay_method === 'CARD' ? `카드${o.provider ? `(${PG_LABEL[o.provider] ?? o.provider})` : ''}` : '정산금') : '결제 전'}{o.refunded_at ? ' · 환불됨' : ''}</span>
            <span>신청 {ago(o.created_at)}</span>
          </span>
        </span>
        <Badge tone={STATUS_TONE[o.status]}>{STATUS_LABEL[o.status]}</Badge>
      </div>

      <div className="adm-note ui-on-soft" style={{ marginTop: 12 }}>
        {o.tracks.length > 0 && <><small className="muted">곡</small><p>{o.tracks.map(t => t.title).join(', ')}</p></>}
        {mv && (
          <>
            <small className="muted">영상 파일</small>
            {o.video ? (
              <p>{o.video.name} · {(o.video.size_bytes / 1024 ** 3).toFixed(2)}GB{' '}
                <button type="button" className="ui-btn is-soft is-xs" onClick={() => void download()}>내려받기</button></p>
            ) : <p><a href={String(d.video_url)} target="_blank" rel="noopener noreferrer">{String(d.video_url)}</a></p>}
            {o.video?.checks && (
              <>
                <small className="muted">파일 검사 · {o.video.checks.rule}</small>
                <p>{o.video.checks.mv_ready ? <Badge tone="green">배급 가능</Badge> : <Badge tone="red">배급 불가</Badge>} {o.video.checks.summary}
                  {o.video.checks.sound === 'pending' && <> <Badge tone="gray">소리 검사 중</Badge></>}</p>
                {o.video.checks.issues.length > 0 && (
                  <ul style={{ margin: '4px 0 0', paddingLeft: 18 }}>
                    {o.video.checks.issues.map(i => <li key={i.code}><code>{i.code}</code> {i.detail}</li>)}
                  </ul>
                )}
              </>
            )}
            <small className="muted">영상</small><p>{String(d.mv_title)} · {d.duration ? `길이 ${String(d.duration)}` : '길이는 내려받아 확인'}{d.width ? ` · ${String(d.width)}×${String(d.height)}` : ''}{d.director ? ` · 감독 ${String(d.director)}` : ''}{d.production ? ` · ${String(d.production)}` : ''}</p>
            {d.cast && <><small className="muted">출연</small><p>{String(d.cast)}</p></>}
            {d.synopsis && <><small className="muted">영상 내용</small><p style={{ whiteSpace: 'pre-wrap' }}>{String(d.synopsis)}</p></>}
            {o.tier === 'BASIC' && <><small className="muted">받은 등급</small><p>{RATING_LABEL[String(d.rating)] ?? String(d.rating)} · {String(d.rating_no)}</p></>}
            <small className="muted">플랫폼</small><p>{platforms.map(p => PLATFORM_NAMES[p] ?? p).join(', ')}{d.release_date ? ` · 공개 희망 ${String(d.release_date)}` : ''}</p>
          </>
        )}
        {o.service === 'EXPRESS_RELEASE' && <><small className="muted">신속 발매 사유 · 발매일</small><p>{String(d.reason || '사유 없음')} · {String(d.release_date ?? '')}</p></>}
        {d.notes && <><small className="muted">아티스트 메모</small><p style={{ whiteSpace: 'pre-wrap' }}>{String(d.notes)}</p></>}
        {o.resolution && <><small className="muted">남긴 결과</small><p>{o.resolution}</p></>}
      </div>

      {open && mv && (
        <div className="form-grid" style={{ marginTop: 12 }}>
          {o.tier !== 'BASIC' && (
            <>
              <div className="ui-field">
                <label htmlFor={`ad-rating-${o.id}`}>심의 결과 등급</label>
                <select id={`ad-rating-${o.id}`} className="ui-select" value={rating} onChange={e => setRating(e.target.value)}>
                  <option value="">아직</option>
                  {Object.entries(RATING_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
                </select>
              </div>
              <div className="ui-field">
                <label htmlFor={`ad-rno-${o.id}`}>등급분류번호</label>
                <input id={`ad-rno-${o.id}`} className="ui-input" maxLength={500} value={ratingNo} onChange={e => setRatingNo(e.target.value)} />
              </div>
            </>
          )}
          {platforms.map(p => (
            <div key={p} className="ui-field">
              <label htmlFor={`ad-link-${o.id}-${p}`}>{PLATFORM_NAMES[p] ?? p} 공개 링크</label>
              <input id={`ad-link-${o.id}-${p}`} className="ui-input" type="url" placeholder="https://" value={links[p] ?? ''} onChange={e => setLinks(l => ({ ...l, [p]: e.target.value }))} />
            </div>
          ))}
        </div>
      )}
      {o.service === 'LYRIC_SYNC' && (o.paid_at || o.status === 'COMPLETED') && (
        <div style={{ marginTop: 12 }}>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => setLyricsOpen(v => !v)}>{lyricsOpen ? '싱크 가사 닫기' : '곡별 싱크 가사 만들기·등록'}</button>
          {lyricsOpen && <LyricsEditor orderId={o.id} ok={ok} toast={toast} />}
        </div>
      )}
      {(open || o.status === 'AWAITING_PAYMENT') && (
        <>
          <div className="ui-field" style={{ marginTop: 12 }}>
            <label htmlFor={`ad-note-${o.id}`}>아티스트에게 남길 말 (진행 불가면 필수)</label>
            <input id={`ad-note-${o.id}`} className="ui-input" maxLength={1000} value={note} onChange={e => setNote(e.target.value)} />
          </div>
          <div className="adm-form-actions">
            {o.status === 'SUBMITTED' && <button type="button" className="ui-btn is-soft" disabled={!ok || busy} onClick={() => void move('IN_PROGRESS', '진행 중으로 바꿨어요.')}>시작</button>}
            {open && mv && <button type="button" className="ui-btn is-soft" disabled={!ok || busy} onClick={() => void move('UPDATE', '결과를 저장했어요.')}>결과 저장</button>}
            {(o.status === 'SUBMITTED' || o.status === 'IN_PROGRESS') && <button type="button" className="ui-btn is-soft" disabled={!ok || busy} onClick={() => void move('DELIVERED', '결과를 전달했다고 기록했어요.')}>결과 전달</button>}
            <button type="button" className="ui-btn is-soft aq-danger-text" disabled={!ok || busy} onClick={() => void move('REJECTED', '진행 불가로 닫고 환불했어요.')}>진행 불가</button>
            {open && <button type="button" className="ui-btn is-primary" disabled={!ok || busy} onClick={() => void move('COMPLETED', '완료로 기록했어요.')}>완료</button>}
          </div>
        </>
      )}
    </div>
  );
}

interface LyricTrack { id: string; title: string; lyrics: string; lrc: string | null; source: string | null; registered: string[] }

function LyricsEditor({ orderId, ok, toast }: { orderId: string; ok: boolean; toast: ReturnType<typeof useToast> }) {
  const { data, reload } = useAsync(() => req<{ items: LyricTrack[]; services: string[] }>(`/api/staff/addons/${orderId}/lyrics`), [orderId]);
  const [lrc, setLrc] = useState<Record<string, string>>({});
  const [reg, setReg] = useState<Record<string, string[]>>({});
  if (!data) return <SkeletonList rows={1} />;
  const save = async (t: LyricTrack) => {
    const text = lrc[t.id] ?? t.lrc ?? '';
    if (lrcLines(text) === null) { toast('[00:12.30]처럼 시간이 붙은 LRC를 넣어 주세요.', 'error'); return; }
    try {
      await req(`/api/staff/addons/${orderId}/lyrics/${t.id}`, { method: 'PUT', body: { lrc: text, registered: reg[t.id] ?? t.registered } });
      toast(`${t.title} 싱크 가사를 저장했어요.`, 'success'); reload();
    } catch (e) { toast(errorMessage(e), 'error'); }
  };
  return (
    <div style={{ display: 'grid', gap: 12, marginTop: 12 }}>
      {data.items.map(t => {
        const regs = reg[t.id] ?? t.registered;
        return (
          <div key={t.id} className="adm-note ui-on-soft">
            <b>{t.title}</b>
            <div className="form-grid" style={{ marginTop: 8 }}>
              <div className="ui-field">
                <label htmlFor={`ly-src-${t.id}`}>가사 (신청서)</label>
                <textarea id={`ly-src-${t.id}`} className="ui-textarea" rows={8} readOnly value={t.lyrics} />
              </div>
              <div className="ui-field">
                <label htmlFor={`ly-lrc-${t.id}`}>싱크 가사 (LRC)</label>
                <textarea id={`ly-lrc-${t.id}`} className="ui-textarea aq-addon-lrc" rows={8} placeholder="[00:12.30]첫 줄" value={lrc[t.id] ?? t.lrc ?? ''} onChange={e => setLrc(m => ({ ...m, [t.id]: e.target.value }))} />
              </div>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, margin: '4px 0 8px' }}>
              {data.services.map(s => (
                <label key={s} className="small">
                  <input type="checkbox" checked={regs.includes(s)} onChange={e => setReg(m => ({ ...m, [t.id]: e.target.checked ? [...regs, s] : regs.filter(x => x !== s) }))} /> {LYRIC_SERVICE_LABEL[s] ?? s} 등록
                </label>
              ))}
            </div>
            <button type="button" className="ui-btn is-soft is-sm" disabled={!ok} onClick={() => void save(t)}>저장</button>
          </div>
        );
      })}
    </div>
  );
}
