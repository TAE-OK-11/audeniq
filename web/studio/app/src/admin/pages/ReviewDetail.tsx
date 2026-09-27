// 발매 심사 시트 — 검사 결과·신청 정보·서류·이력을 보고 승인 / 보완 요청 / 거절을 결정한다.
import { useState } from 'react';
import { Link, useNavigate, useParams } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type Check, type DecisionAction, type ReleaseSheet } from '../api';
import {
  APPROVAL_STATUS, CHECK_STATUS, DECISION_LABEL, DOC_KIND, DOC_STATUS, MAX_REASON, READINESS, RELEASE_STATUS, RELEASE_TYPE,
  checkLabel, day, needsSecond, pick, shortId, when,
} from '../labels';
import { Chip, Empty, ErrorBox, Initial, NoDuty, Section, Skeleton, StatusChip, useStaff } from '../ui';

const DECL_LABEL: Record<string, string> = {
  rights_confirmed: '권리 보유 확인', adult_confirmed: '성인 확인', is_cover: '커버곡', is_remix: '리믹스',
  contains_samples: '샘플 사용', ai_involved: 'AI 활용', explicit_content: '19금 표현',
};
const ROLE_KO: Record<string, string> = { COMPOSER: '작곡', LYRICIST: '작사', ARRANGER: '편곡', PRODUCER: '프로듀서', PERFORMER: '연주', MAIN_ARTIST: '아티스트' };
const ACTION_KO: Record<string, string> = {
  'release.submitted': '발매 접수', 'stage1.decision': '1차 검사 결과', 'stage2.decision': '2차 검사 결과',
  'staff.approved': '담당자 승인', 'staff.correction_requested': '담당자 보완 요청', 'staff.rejected': '담당자 거절',
  'staff.approval_requested': '2차 승인 요청', 'staff.approval_granted': '2차 승인 완료', 'staff.approval_declined': '2차 승인 반려',
  'staff.proof_requested': '권리 증빙 요청', 'staff.identifiers_reissue': '식별자 재발급 요청',
};

function CheckCard({ c, open, note, onNote }: { c: Check; open?: boolean; note?: string; onNote?: (v: string) => void }) {
  const sensitive = open && needsSecond(c);
  const cls = ['adm-check', open ? 'is-open' : '', sensitive ? 'is-sensitive' : ''].filter(Boolean).join(' ');
  return (
    <div className={cls}>
      <div className="adm-check-top">
        <span><b>{checkLabel(c.check_code)}</b> <span className="adm-code">{c.check_code}</span></span>
        <span className="adm-codes">
          {sensitive && <Chip tone="red">2인 승인 필요</Chip>}
          <StatusChip value={pick(CHECK_STATUS, c.status)} />
        </span>
      </div>
      {c.detail && <p>{c.detail}</p>}
      {onNote && (
        <textarea
          className="adm-textarea" rows={2} maxLength={2000} value={note ?? ''}
          aria-label={`${checkLabel(c.check_code)} 항목 메모`}
          placeholder="이 항목에 대해 아티스트에게 보여줄 안내 (보완 요청·거절 시 전달돼요)"
          onChange={e => onNote(e.target.value)}
        />
      )}
    </div>
  );
}

const ACTION_COPY: Record<DecisionAction, { title: string; lead: string; placeholder: string; submit: string; btn: string }> = {
  APPROVE: {
    title: '심사 승인', lead: '미해결 검사 항목을 통과(PASS)로 처리하고 2차 재평가를 예약해요. 파이프라인이 다음 단계(배포 준비)로 넘겨요.',
    placeholder: '승인 근거 (예: 크레딧 표기는 동일인의 다른 활동명으로 확인)', submit: '승인하기', btn: 'primary',
  },
  REQUEST_CORRECTION: {
    title: '보완 요청', lead: '미해결 검사 항목을 모두 ‘보완 필요’로 바꾸고 아티스트에게 돌려보내요. 항목별 메모는 아티스트 화면에 그대로 보여요.',
    placeholder: '아티스트에게 전달할 보완 요청 요약', submit: '보완 요청 보내기', btn: 'primary',
  },
  REJECT: {
    title: '발매 거절', lead: '발매를 거절(WITHDRAWN)하고 작업 공간에 알려요. 되돌릴 수 없어요 — 아티스트는 새로 접수해야 해요.',
    placeholder: '거절 사유 (아티스트에게 전달돼요)', submit: '거절 확정', btn: 'solid-danger',
  },
};

function DecisionForm({ sheet, action, onDone }: { sheet: ReleaseSheet; action: DecisionAction; onDone: (msg: string) => void }) {
  const close = useModalClose();
  const toast = useToast();
  const copy = ACTION_COPY[action];
  const open = sheet.open_checks;
  const [reason, setReason] = useState('');
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [sure, setSure] = useState(false);
  const [busy, setBusy] = useState(false);
  const withNotes = action !== 'APPROVE' && open.length > 0;
  const sensitive = action === 'APPROVE' && open.some(needsSecond);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!reason.trim() || busy || !sheet.release.revision_id) return;
    setBusy(true);
    try {
      const res = await staffApi.decide(sheet.release.id, {
        action,
        revision_id: sheet.release.revision_id,
        reason: reason.trim(),
        notes: withNotes ? Object.entries(notes).filter(([, v]) => v.trim()).map(([check_code, note]) => ({ check_code, note: note.trim() })) : [],
      });
      const msg = res.result === 'PENDING_SECOND_APPROVAL'
        ? '민감 항목이 있어 2차 승인 요청을 올렸어요. 다른 심사 담당자가 승인하면 반영돼요.'
        : res.result === 'REJECTED' ? '발매를 거절했어요. 아티스트에게 알림이 갔어요.'
          : action === 'APPROVE' ? '승인했어요. 2차 재평가 후 배포 준비로 넘어가요.' : '보완 요청을 보냈어요. 아티스트에게 알림이 갔어요.';
      onDone(msg);
      close();
    } catch (err) {
      toast(errorMessage(err, '결정을 저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit}>
      <p className="small muted">{copy.lead}</p>
      {sensitive && (
        <div className="adm-alert is-warn" style={{ marginTop: 14 }}>
          권리·중복·보호명 등 <b>민감 항목</b>이 있어 바로 통과되지 않아요. 승인하면 <b>2차 승인 요청</b>이 만들어지고, 다른 심사 담당자가 승인해야 반영돼요.
        </div>
      )}
      {action === 'REQUEST_CORRECTION' && open.length === 0 && (
        <div className="adm-alert is-error" style={{ marginTop: 14 }}>보완 요청할 미해결 검사 항목이 없어요.</div>
      )}
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="dReason">{action === 'APPROVE' ? '승인 근거' : action === 'REJECT' ? '거절 사유' : '요청 요약'} <span className="required">*</span></label>
        <textarea id="dReason" data-autofocus className="adm-textarea" rows={4} maxLength={MAX_REASON} required value={reason} placeholder={copy.placeholder} onChange={e => setReason(e.target.value)} />
        <small className="adm-counter">{reason.length} / {MAX_REASON} · 감사 기록에 담당자 이름과 함께 남아요</small>
      </div>
      {withNotes && (
        <div className="adm-field">
          <span className="adm-field-label">항목별 안내 (선택)</span>
          <div className="adm-checks">
            {open.map(c => (
              <CheckCard key={c.check_code} c={c} open note={notes[c.check_code]} onNote={v => setNotes(n => ({ ...n, [c.check_code]: v }))} />
            ))}
          </div>
        </div>
      )}
      {action === 'REJECT' && (
        <label className="adm-check-line">
          <input type="checkbox" checked={sure} onChange={e => setSure(e.target.checked)} />
          <span><b>{sheet.release.title}</b> 발매를 거절하면 되돌릴 수 없다는 것을 확인했어요.</span>
        </label>
      )}
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button
          type="submit" className={`adm-btn ${copy.btn}`}
          disabled={busy || !reason.trim() || (action === 'REJECT' && !sure) || (action === 'REQUEST_CORRECTION' && open.length === 0)}
        >
          {busy ? '저장 중…' : copy.submit}
        </button>
      </div>
    </form>
  );
}

function ProofForm({ sheet, onDone }: { sheet: ReleaseSheet; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim() || busy) return;
    setBusy(true);
    try {
      await staffApi.requestProof(sheet.release.org_id, { release_id: sheet.release.id, title: title.trim(), body: body.trim() });
      toast('권리 증빙을 요청했어요. 아티스트에게 알림이 갔어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '증빙 요청을 보내지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">아티스트의 ‘권리·보완 서류’ 화면에 제출 요청이 생기고 알림이 가요.</p>
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="pTitle">요청 서류 이름 <span className="required">*</span></label>
        <input id="pTitle" data-autofocus className="adm-input" maxLength={200} required value={title} placeholder="예: 샘플 사용 허락서" onChange={e => setTitle(e.target.value)} />
      </div>
      <div className="adm-field">
        <label htmlFor="pBody">안내 내용</label>
        <textarea id="pBody" className="adm-textarea" rows={5} maxLength={20000} value={body} placeholder="어떤 서류가 왜 필요한지, 어떤 형식으로 내면 되는지 적어 주세요." onChange={e => setBody(e.target.value)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy || !title.trim()}>{busy ? '보내는 중…' : '증빙 요청 보내기'}</button>
      </div>
    </form>
  );
}

function ReissueForm({ sheet, onDone }: { sheet: ReleaseSheet; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!reason.trim() || busy) return;
    setBusy(true);
    try {
      await staffApi.reissue(sheet.release.id, reason.trim());
      toast('식별자 재발급을 위해 아티스트에게 돌려보냈어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '재발급 요청을 처리하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">테스트 범위(임시) UPC·ISRC로 만들어진 패키지를 정식 식별자로 다시 만들어요. 발매는 ‘배포 보완 요청’ 상태가 되고, 재접수 시 정식 코드가 발급돼요.</p>
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="rReason">사유 <span className="required">*</span></label>
        <textarea id="rReason" data-autofocus className="adm-textarea" rows={3} maxLength={2000} required value={reason} onChange={e => setReason(e.target.value)} placeholder="예: 정식 ISRC 발급 범위 등록 완료" />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy || !reason.trim()}>{busy ? '처리 중…' : '재발급 요청'}</button>
      </div>
    </form>
  );
}

export function ReviewDetail() {
  const { id = '' } = useParams<{ id: string }>();
  const nav = useNavigate();
  const { can, refreshCounts, me } = useStaff();
  const { data: sheet, loading, error, reload } = useAsync(() => staffApi.release(id), [id]);
  const [action, setAction] = useState<DecisionAction | null>(null);
  const [modal, setModal] = useState<'proof' | 'reissue' | null>(null);
  const [done, setDone] = useState('');
  const [showAll, setShowAll] = useState(false);

  if (loading && !sheet) return <Skeleton rows={6} />;
  if (error && !sheet) return <ErrorBox message={error} onRetry={reload} />;
  if (!sheet) return null;

  const r = sheet.release;
  const app = sheet.application;
  const inReview = r.status === 'STAGE2_REVIEW';
  const pending = sheet.second_approvals.find(a => a.status === 'PENDING');
  const canReview = can('REVIEW');
  const tracks = app.tracks ?? [];
  const decl = app.declarations ?? {};
  const after = (msg: string) => { setDone(msg); reload(); refreshCounts(); };

  return (
    <div className="view-enter">
      <Link to="/admin/reviews" className="adm-back">← 심사 목록</Link>

      <div className="adm-detail">
        <div>
          <div className="adm-release-hero">
            <Initial text={r.title} />
            <div className="adm-min">
              <div className="adm-codes" style={{ marginBottom: 6 }}>
                <StatusChip value={pick(RELEASE_STATUS, r.status)} />
                <Chip tone="gray">{RELEASE_TYPE[r.release_type] ?? r.release_type}</Chip>
              </div>
              <h1>{r.title}</h1>
              <span className="adm-row-meta">
                <span>{app.artist || '아티스트 미기재'}</span>
                <span>{r.org_name}</span>
                <span>접수 {when(r.submitted_at)}</span>
              </span>
            </div>
          </div>

          {done && <div className="adm-alert is-ok" role="status">{done}</div>}
          {pending && (
            <div className="adm-alert is-warn adm-alert-row">
              <span>2차 승인 대기 중 — {pending.check_codes.map(checkLabel).join(', ')} · 요청 {when(pending.at)}{pending.requested_by === me.user_id ? ' (내 요청)' : ''}</span>
              <button type="button" className="adm-btn warn small" onClick={() => nav('/admin/approvals')}>2차 승인으로</button>
            </div>
          )}

          <Section title="미해결 검사 항목" meta={`${sheet.open_checks.length}건 · 2차 검사를 멈춘 항목`}>
            {sheet.open_checks.length ? (
              <div className="adm-checks">{sheet.open_checks.map(c => <CheckCard key={c.check_code} c={c} open />)}</div>
            ) : (
              <Empty title="담당자 판단이 필요한 항목이 없어요">{inReview ? '승인하면 바로 재평가돼요.' : '심사 대기 상태가 아니에요.'}</Empty>
            )}
          </Section>

          {sheet.advisories.length > 0 && (
            <Section title="오디오 권고" meta="차단하지 않지만 확인이 필요한 1차 경고">
              <div className="adm-checks">{sheet.advisories.map(c => <CheckCard key={c.check_code} c={c} />)}</div>
            </Section>
          )}

          <Section title="신청 정보">
            <div className="adm-card">
              <dl className="adm-kv">
                <div><dt>아티스트</dt><dd>{app.artist || '—'}</dd></div>
                <div><dt>장르 · 언어</dt><dd>{[app.genre, app.language].filter(Boolean).join(' · ') || '—'}</dd></div>
                <div><dt>발매 예정일</dt><dd>{day(app.release_date)}</dd></div>
                <div><dt>레이블</dt><dd>{app.label || '—'}</dd></div>
                <div><dt>℗ / ©</dt><dd>{[app.p_line, app.c_line].filter(Boolean).join(' / ') || '—'}</dd></div>
                <div><dt>UPC</dt><dd>{r.upc ?? '발급 전'}</dd></div>
                <div style={{ gridColumn: '1 / -1' }}>
                  <dt>배급 플랫폼</dt>
                  <dd className="adm-dsps">{app.platforms.length ? app.platforms.map(p => <span key={p}>{p}</span>) : '—'}</dd>
                </div>
                <div style={{ gridColumn: '1 / -1' }}>
                  <dt>신고 항목</dt>
                  <dd className="adm-decl">
                    {Object.entries(DECL_LABEL).map(([k, label]) => {
                      const v = (decl as Record<string, boolean>)[k];
                      const warnWhenTrue = !['rights_confirmed', 'adult_confirmed'].includes(k);
                      return <Chip key={k} tone={v ? (warnWhenTrue ? 'amber' : 'green') : 'gray'}>{label} {v ? '예' : '아니오'}</Chip>;
                    })}
                  </dd>
                </div>
              </dl>
              {sheet.signed_application && (
                <p className="small muted" style={{ marginTop: 16 }}>
                  서명 신청서 <b>{sheet.signed_application.application_no}</b> · {sheet.signed_application.signer_name}({sheet.signed_application.signer_role}) · {when(sheet.signed_application.received_at)} · 문서 해시 <span className="adm-code">{sheet.signed_application.content_hash.slice(0, 16)}…</span>
                </p>
              )}
            </div>
          </Section>

          <Section title="트랙" meta={`${tracks.length}곡`}>
            {tracks.length ? (
              <div className="adm-card white adm-table-wrap">
                <table className="adm-table">
                  <thead><tr><th>#</th><th>곡명</th><th>ISRC</th><th>크레딧</th><th>음원</th></tr></thead>
                  <tbody>
                    {tracks.map(t => (
                      <tr key={t.id}>
                        <td>{t.disc_number > 1 ? `${t.disc_number}-` : ''}{t.track_number}</td>
                        <td><b>{t.title}</b>{t.version ? ` (${t.version})` : ''}{t.parental_advisory && <> <Chip tone="red">19</Chip></>}</td>
                        <td>{t.isrc ? <span className="adm-code">{t.isrc}</span> : '발급 전'}</td>
                        <td>{t.credits.map(c => ROLE_KO[c.role] ?? c.role).join(', ') || '—'}</td>
                        <td>{t.asset_kind ?? '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : <Empty icon="♪" title="트랙 정보가 없어요" />}
          </Section>

          <Section
            title="서류" meta={`${sheet.documents.length}건`}
            action={can('DOCUMENTS') && <button type="button" className="adm-btn soft small" onClick={() => setModal('proof')}>권리 증빙 요청</button>}
          >
            {sheet.documents.length ? (
              <div className="adm-list">
                {sheet.documents.map(d => (
                  <div key={d.id} className="adm-row">
                    <Initial text={DOC_KIND[d.kind] ?? d.kind} plain />
                    <span className="adm-min">
                      <span className="adm-row-title">{d.title}</span>
                      <span className="adm-row-meta"><span>{DOC_KIND[d.kind] ?? d.kind}</span>{d.file_name && <span>{d.file_name}</span>}<span>{when(d.updated_at)}</span></span>
                      {d.review_note && <span className="adm-row-meta"><span>메모: {d.review_note}</span></span>}
                    </span>
                    <span className="adm-row-end"><StatusChip value={pick(DOC_STATUS, d.status)} /></span>
                  </div>
                ))}
              </div>
            ) : <p className="small muted">연결된 서류가 없어요.</p>}
          </Section>

          {sheet.delivery_staging.length > 0 && (
            <Section title="배급 스테이징" meta="DSP별 패키지">
              <div className="adm-card white adm-table-wrap">
                <table className="adm-table">
                  <thead><tr><th>DSP</th><th>준비</th><th>승인</th><th>경로</th><th>스테이징</th></tr></thead>
                  <tbody>
                    {sheet.delivery_staging.map(s => (
                      <tr key={`${s.package_id}-${s.dsp}`}>
                        <td><b>{s.dsp}</b></td>
                        <td><StatusChip value={pick(READINESS, s.readiness)} /></td>
                        <td><StatusChip value={pick(APPROVAL_STATUS, s.approval)} /></td>
                        <td className="small">{s.route_status ?? '—'}{s.route_reason ? ` · ${s.route_reason}` : ''}</td>
                        <td className="small">{when(s.staged_at)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Section>
          )}

          {(sheet.notes.length > 0 || sheet.overrides.length > 0) && (
            <Section title="담당자 결정 기록">
              {sheet.notes.map(n => (
                <div key={n.id} className="adm-note">
                  <div className="adm-check-top">
                    <span><Chip tone={n.decision === 'REJECT' ? 'red' : n.decision === 'APPROVE' ? 'green' : 'amber'}>{DECISION_LABEL[n.decision] ?? n.decision}</Chip>{n.check_code && <> <span className="adm-code">{n.check_code}</span></>}</span>
                    <small className="muted">{when(n.at)} · {shortId(n.author_user_id)}</small>
                  </div>
                  <p>{n.note}</p>
                </div>
              ))}
              {sheet.overrides.length > 0 && (
                <div className="adm-card white adm-table-wrap" style={{ marginTop: 10 }}>
                  <table className="adm-table">
                    <thead><tr><th>검사</th><th>변경</th><th>담당 / 2차</th><th>시각</th></tr></thead>
                    <tbody>
                      {sheet.overrides.map(o => (
                        <tr key={o.id}>
                          <td><span className="adm-code">{o.check_code}</span></td>
                          <td>{pick(CHECK_STATUS, o.original_status)[0]} → <b>{pick(CHECK_STATUS, o.proposed_status)[0]}</b></td>
                          <td className="small">{shortId(o.actor_user_id)}{o.second_approver_user_id ? ` / ${shortId(o.second_approver_user_id)}` : ''}</td>
                          <td className="small">{when(o.at)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Section>
          )}

          <Section
            title="전체 검사 결과" meta={`${sheet.checks.length}건`}
            action={<button type="button" className="adm-btn soft small" onClick={() => setShowAll(v => !v)}>{showAll ? '접기' : '펼치기'}</button>}
          >
            {showAll ? (
              <div className="adm-checks">{sheet.checks.map(c => <CheckCard key={c.check_code} c={c} />)}</div>
            ) : (
              <div className="adm-codes">
                {Object.entries(sheet.checks.reduce<Record<string, number>>((m, c) => ({ ...m, [c.status]: (m[c.status] ?? 0) + 1 }), {}))
                  .map(([s, n]) => <Chip key={s} tone={pick(CHECK_STATUS, s)[1]}>{pick(CHECK_STATUS, s)[0]} {n}</Chip>)}
              </div>
            )}
          </Section>

          <Section title="처리 이력" meta="최근 100건">
            <ol className="adm-timeline">
              {sheet.timeline.map((t, i) => (
                <li key={`${t.at}-${i}`} className={t.action.startsWith('staff.') ? 'is-staff' : ''}>
                  <i aria-hidden="true" />
                  <div>
                    <b>{ACTION_KO[t.action] ?? t.action}</b>{t.reason && <> <span className="adm-code">{t.reason}</span></>}
                    <small>{when(t.at)} · {t.actor_service ?? shortId(t.actor_user_id)}</small>
                  </div>
                </li>
              ))}
            </ol>
          </Section>
        </div>

        <aside className="adm-detail-side">
          <div className="adm-decide">
            <h2>심사 결정</h2>
            <p>
              {inReview
                ? `미해결 ${sheet.open_checks.length}건 · 수정본 ${shortId(r.revision_id)}`
                : `지금은 ‘${pick(RELEASE_STATUS, r.status)[0]}’ 상태라 결정할 수 없어요.`}
            </p>
            <div className="adm-decide-actions">
              <button type="button" className="adm-btn primary" disabled={!inReview || !canReview || !!pending} onClick={() => setAction('APPROVE')}>승인</button>
              <button type="button" className="adm-btn warn" disabled={!inReview || !canReview || sheet.open_checks.length === 0} onClick={() => setAction('REQUEST_CORRECTION')}>보완 요청</button>
              <button type="button" className="adm-btn danger" disabled={!inReview || !canReview} onClick={() => setAction('REJECT')}>거절</button>
            </div>
            <div className="adm-decide-note">
              승인은 검사 결과를 고치지 않고 통과 기록(override)을 남긴 뒤 파이프라인이 다시 평가해요. 권리·중복 등 민감 항목은 다른 담당자의 2차 승인이 필요해요.
            </div>
          </div>
          {!canReview && <NoDuty duty="발매 심사" />}
          {r.status === 'READY_FOR_DELIVERY' && canReview && (
            <div className="adm-card">
              <b>식별자 재발급</b>
              <p className="small muted" style={{ margin: '6px 0 12px' }}>임시(테스트) UPC·ISRC로 준비된 패키지를 정식 코드로 다시 만들어요.</p>
              <button type="button" className="adm-btn soft small" onClick={() => setModal('reissue')}>재발급 요청</button>
            </div>
          )}
          <div className="adm-card adm-id-card">
            <dl className="adm-kv" style={{ gridTemplateColumns: '1fr' }}>
              <div><dt>발매 ID</dt><dd><span className="adm-code">{r.id}</span></dd></div>
              <div><dt>작업 공간</dt><dd>{r.org_name} <span className="adm-code">{shortId(r.org_id)}</span></dd></div>
            </dl>
          </div>
        </aside>
      </div>

      {action && (
        <Modal title={ACTION_COPY[action].title} onClose={() => setAction(null)} dismissible={false}>
          <DecisionForm sheet={sheet} action={action} onDone={after} />
        </Modal>
      )}
      {modal === 'proof' && (
        <Modal title="권리 증빙 요청" onClose={() => setModal(null)} dismissible={false}>
          <ProofForm sheet={sheet} onDone={() => after('권리 증빙을 요청했어요.')} />
        </Modal>
      )}
      {modal === 'reissue' && (
        <Modal title="식별자 재발급" onClose={() => setModal(null)} dismissible={false}>
          <ReissueForm sheet={sheet} onDone={() => after('식별자 재발급을 요청했어요.')} />
        </Modal>
      )}
    </div>
  );
}
