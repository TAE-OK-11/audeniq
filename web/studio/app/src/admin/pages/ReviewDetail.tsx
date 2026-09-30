// 발매 심사 시트 — 검사 결과·신청 정보·서류·이력을 보고 승인 / 보완 요청 / 거절을 결정한다.
// 두 경우를 결정한다: 2차 검사에서 멈춘 발매(STAGE2_REVIEW), 자동 검사를 통과했고 신청서(배급 계약서)가
// 검토 전인 새 발매 신청(READY_FOR_DELIVERY). 신청서는 발매와 함께 결정되고 서류 검토에는 나오지 않는다.
import { useState } from 'react';
import { Link, useNavigate, useParams } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type Check, type DecisionAction, type DecisionInput, type DecisionResult, type ReleaseSheet } from '../api';
import { FIX_PRESETS } from '../fixPresets';
import { STAFF_FIX_OPTIONS, WIZ_STEP_NAMES, correctionTarget, isKnownCorrection, staffFixCode } from '../../lib/corrections';
import {
  CHECK_STATUS, DECISION_LABEL, DOC_KIND, DOC_STATUS, MAX_REASON, RELEASE_STATUS, RELEASE_TYPE,
  REJECT_REASONS, applicationPending, checkLabel, checkSummary, needsSecond, pick, shortId, stageStateLabel, systemStages, when,
} from '../labels';
import { Chip, ErrorBox, Initial, NoDuty, Section, Skeleton, StatusChip, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';
import { CheckIcon } from '../../components/Check';
import { ApplicationSection, EnteredInfoSection, OptionsSection, TracksSection, useIntegrity } from '../Submission';
import { CurrentValue, ReviewBrief, type BriefFix } from '../ReviewBrief';

const ACTION_KO: Record<string, string> = {
  'release.submitted': '발매 접수', 'stage1.decision': '1차 검사 결과', 'stage1.completed': '1차 검사 완료', 'stage2.decision': '2차 검사 결과',
  'stage2.pass': '2차 검사 통과', 'stage3.prepared': '배급 준비 완료', 'delivery.staged': '플랫폼별 전송 준비',
  'delivery.held': '배급 대기 (계약서 서명 전)', 'delivery.enqueued': '플랫폼 전송 예약', 'release.updated': '발매 정보 수정',
  'staff.approved': '담당자 승인', 'staff.correction_requested': '담당자 보완 요청', 'staff.rejected': '담당자 거절',
  'staff.approval_requested': '2차 승인 요청', 'staff.approval_granted': '2차 승인 완료', 'staff.approval_declined': '2차 승인 반려',
  'staff.proof_requested': '권리 증빙 요청', 'release.withdrawn': '신청 취소', 'staff.identifiers_reissue': '식별자 재발급 요청', 'staff.document_reviewed': '신청서 처리',
};

/** 검사 항목 — 쉬운 설명을 먼저, 검사 코드와 원문은 ‘상세 보기’에 */
function CheckCard({ c, open }: { c: Check; open?: boolean }) {
  const sensitive = open && needsSecond(c);
  const cls = ['adm-check', open ? 'is-open' : '', sensitive ? 'is-sensitive' : ''].filter(Boolean).join(' ');
  return (
    <div className={cls}>
      <div className="adm-check-top">
        <b>{checkLabel(c.check_code)}</b>
        <span className="adm-codes">
          {sensitive && <Chip tone="red">2인 승인 필요</Chip>}
          <StatusChip value={pick(CHECK_STATUS, c.status)} />
        </span>
      </div>
      <p>{checkSummary(c)}</p>
      <details className="adm-more">
        <summary>상세 보기</summary>
        <div><span className="adm-code">{c.check_code}</span></div>
        {c.detail && <p className="adm-raw">{c.detail}</p>}
      </details>
    </div>
  );
}

/** withdraw::WITHDRAWABLE와 같게 유지 */
const WITHDRAWABLE = ['STAGE1_CORRECTION', 'STAGE2_REVIEW', 'STAGE2_CORRECTION', 'STAGE3_CORRECTION', 'READY_FOR_DELIVERY', 'ON_HOLD_RIGHTS'];

const ACTION_TITLE: Record<DecisionAction, string> = { APPROVE: '발매 승인', REQUEST_CORRECTION: '보완 요청', REJECT: '발매 거절' };

function useDecide(sheet: ReleaseSheet, onDone: (msg: string) => void) {
  const close = useModalClose();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const run = async (input: Omit<DecisionInput, 'revision_id'>, msg: (r: DecisionResult) => string) => {
    if (busy || !sheet.release.revision_id) return;
    setBusy(true);
    try {
      const res = await staffApi.decide(sheet.release.id, { ...input, revision_id: sheet.release.revision_id });
      onDone(msg(res));
      close();
    } catch (err) {
      toast(errorMessage(err, '결정을 저장하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return { busy, run, close };
}

function ApproveForm({ sheet, application, onDone }: { sheet: ReleaseSheet; application: boolean; onDone: (msg: string) => void }) {
  const { busy, run, close } = useDecide(sheet, onDone);
  const [memo, setMemo] = useState('');
  const integrity = useIntegrity(sheet);
  const sensitive = sheet.open_checks.some(needsSecond);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    void run({ action: 'APPROVE', reason: memo.trim() }, res => {
      if (res.result === 'PENDING_SECOND_APPROVAL') return '민감 항목이 있어 2차 승인 요청을 올렸어요. 다른 심사 담당자가 승인하면 반영돼요.';
      return application ? '승인했어요. 아티스트가 계약서에 서명하면 문제 없는 플랫폼으로 자동 배급돼요.' : '승인했어요. 시스템이 나머지 검사와 배급 준비를 이어서 해요.';
    });
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">
        {application
          ? '최종 승인이에요. 아티스트가 계약서에 서명하면 문제 없는 플랫폼으로 시스템이 자동 배급해요. 따로 배급 승인할 필요 없어요.'
          : '남은 검사 항목을 통과로 처리해요. 이후 배급 준비는 시스템이 자동으로 이어서 해요.'}
      </p>
      <ReviewBrief sheet={sheet} integrity={integrity} title="승인 전 확인할 것" />
      {sensitive && (
        <div className="adm-alert is-warn" style={{ marginTop: 14 }}>
          권리·중복·보호명 등 <b>민감 항목</b>이 있어 다른 심사 담당자의 <b>2차 승인</b> 후 반영돼요.
        </div>
      )}
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="aMemo">승인 메모 <span className="muted">(선택)</span></label>
        <textarea id="aMemo" data-autofocus className="adm-textarea" rows={3} maxLength={MAX_REASON} value={memo} placeholder="남길 내용이 있으면 적어 주세요. 비워 두면 ‘담당자 승인’으로 기록돼요." onChange={e => setMemo(e.target.value)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy}>{busy ? '저장 중…' : '승인하기'}</button>
      </div>
    </form>
  );
}

function RejectForm({ sheet, onDone }: { sheet: ReleaseSheet; onDone: (msg: string) => void }) {
  const { busy, run, close } = useDecide(sheet, onDone);
  const [picked, setPicked] = useState<string[]>([]);
  const [other, setOther] = useState(false);
  const [custom, setCustom] = useState('');
  const [sure, setSure] = useState(false);
  const integrity = useIntegrity(sheet);
  const toggle = (id: string) => setPicked(p => (p.includes(id) ? p.filter(x => x !== id) : [...p, id]));
  const reason = [
    ...REJECT_REASONS.filter(r => picked.includes(r.id)).map(r => `· ${r.label}: ${r.text}`),
    ...(other && custom.trim() ? [`· 기타: ${custom.trim()}`] : []),
  ].join('\n');
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!reason || !sure) return;
    void run({ action: 'REJECT', reason, notes: [] }, () => '발매를 거절했어요. 아티스트에게 거절 사유와 함께 알림이 갔어요.');
  };
  return (
    <form onSubmit={submit}>
      <ReviewBrief sheet={sheet} integrity={integrity} title="거절 전 확인할 것" />
      <p className="small muted" style={{ marginTop: 14 }}>사유를 누르면 그대로 기록되고 아티스트에게 전달돼요. 여러 개 고를 수 있어요. 고칠 수 있는 문제면 거절 대신 보완 요청을 보내 주세요.</p>
      <div className="adm-reasons" role="group" aria-label="거절 사유">
        {REJECT_REASONS.map(r => (
          <button key={r.id} type="button" className={`adm-reason${picked.includes(r.id) ? ' is-on' : ''}`} aria-pressed={picked.includes(r.id)} title={r.text} onClick={() => toggle(r.id)}>{r.label}</button>
        ))}
        <button type="button" className={`adm-reason${other ? ' is-on' : ''}`} aria-pressed={other} onClick={() => setOther(v => !v)}>기타 (직접 입력)</button>
      </div>
      {other && (
        <div className="adm-field">
          <label htmlFor="rCustom">기타 사유</label>
          <textarea id="rCustom" data-autofocus className="adm-textarea" rows={3} maxLength={1000} value={custom} placeholder="아티스트에게 전달할 거절 사유" onChange={e => setCustom(e.target.value)} />
        </div>
      )}
      {reason && <div className="adm-alert" style={{ whiteSpace: 'pre-line' }}>{reason}</div>}
      <label className="adm-check-line">
        <input type="checkbox" checked={sure} onChange={e => setSure(e.target.checked)} />
        <span><b>{sheet.release.title}</b> 발매를 거절하면 되돌릴 수 없어요. 아티스트는 새로 접수해야 해요.</span>
      </label>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn solid-danger" disabled={busy || !reason || !sure}>{busy ? '저장 중…' : '거절 확정'}</button>
      </div>
    </form>
  );
}

interface FixItem { key: string; step: number; code: string; trackId: string; note: string; custom?: boolean; system?: boolean }
let fixSeq = 0;
const newFix = (over: Partial<FixItem> = {}): FixItem => ({ key: `fx${++fixSeq}`, step: -1, code: '', trackId: '', note: '', ...over });

/** 보완 요청 — 페이지 → 항목(→ 트랙)을 고르고 문제를 적으면 아티스트는 그 입력칸으로 바로 가서 고친다 */
function CorrectionForm({ sheet, onDone }: { sheet: ReleaseSheet; onDone: (msg: string) => void }) {
  const { busy, run, close } = useDecide(sheet, onDone);
  const tracks = sheet.application.tracks ?? [];
  // 시스템이 찾은 문제(미해결 검사)는 미리 채워 둔다 — 담당자는 문구만 다듬으면 된다
  const [items, setItems] = useState<FixItem[]>(() => {
    const sys = sheet.open_checks.map(c => newFix({ code: c.check_code, note: isKnownCorrection(c.check_code) ? correctionTarget(c.check_code).hint : checkSummary(c), system: true }));
    return sys.length ? sys : [newFix()];
  });
  const [summary, setSummary] = useState('');
  const integrity = useIntegrity(sheet);
  // ‘확인할 것’에서 누른 문제를 보완 항목으로 (빈 첫 항목이 있으면 그 자리에)
  const addFromBrief = (f: BriefFix) => setItems(list => {
    const item = newFix({ step: correctionTarget(f.code).step, code: f.code, trackId: f.trackId ?? '', note: f.note, custom: !FIX_PRESETS[f.code]?.includes(f.note) });
    const blank = list.findIndex(x => !x.system && !x.code);
    return blank >= 0 ? list.map((x, n) => (n === blank ? { ...item, key: x.key } : x)) : [...list, item];
  });
  const set = (key: string, patch: Partial<FixItem>) => setItems(list => list.map(i => (i.key === key ? { ...i, ...patch } : i)));
  const opt = (code: string) => STAFF_FIX_OPTIONS.find(o => o.code === code);
  const ready = items.filter(i => i.code && (!opt(i.code)?.track || i.trackId));
  const incomplete = items.some(i => !i.system && (!i.code || (opt(i.code)?.track && !i.trackId) || !i.note.trim()));
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready.length || incomplete) return;
    const notes = ready.map(i => ({
      check_code: i.system ? i.code : staffFixCode(i.code, opt(i.code)?.track ? i.trackId : undefined),
      note: i.note.trim() || correctionTarget(i.code).hint,
    }));
    const reason = summary.trim() || '표시된 항목을 고친 뒤 다시 접수해 주세요.';
    void run({ action: 'REQUEST_CORRECTION', reason, notes }, () => '보완 요청을 보냈어요. 아티스트는 표시된 칸으로 바로 가서 고칠 수 있어요.');
  };
  return (
    <form onSubmit={submit}>
      <ReviewBrief
        sheet={sheet} integrity={integrity} title="찾은 문제" onAddFix={addFromBrief}
        added={items.filter(i => i.code).map(i => `${i.code}@${i.trackId || ''}`)}
      />
      <p className="small muted" style={{ marginTop: 14 }}>고칠 곳을 페이지와 항목으로 지정하고 무엇이 문제인지 적어 주세요. 아티스트 화면에서 그 입력칸으로 바로 이동해요.</p>
      <div className="adm-fixes">
        {items.map((i, n) => {
          const o = opt(i.code);
          const step = o ? o.step : i.system ? correctionTarget(i.code).step : i.step;
          return (
            <div key={i.key} className="adm-fix">
              <div className="adm-fix-top">
                <b>{i.system ? `시스템이 찾은 문제 · ${checkLabel(i.code)}` : `보완 항목 ${n + 1}`}</b>
                {!i.system && items.length > 1 && (
                  <button type="button" className="adm-btn soft small" onClick={() => setItems(list => list.filter(x => x.key !== i.key))}>삭제</button>
                )}
              </div>
              {!i.system && (
                <div className="adm-fix-pick">
                  {/* 1. 페이지 → 2. 고칠 부분 (→ 트랙) — 눌러서 고른다 */}
                  <span className="adm-fix-label">페이지</span>
                  <div className="adm-picks" role="radiogroup" aria-label="페이지">
                    {WIZ_STEP_NAMES.map((name, s2) => STAFF_FIX_OPTIONS.some(x => x.step === s2) && (
                      <button
                        key={name} type="button" role="radio" aria-checked={step === s2}
                        className={`adm-pick${step === s2 ? ' is-on' : ''}`}
                        onClick={() => set(i.key, { step: s2, code: '', trackId: '', note: '', custom: false })}
                      >{name}</button>
                    ))}
                  </div>
                  {step >= 0 && (
                    <>
                      <span className="adm-fix-label">보완할 부분</span>
                      <div className="adm-picks" role="radiogroup" aria-label="보완할 부분">
                        {STAFF_FIX_OPTIONS.filter(x => x.step === step).map(x => (
                          <button
                            key={x.code} type="button" role="radio" aria-checked={i.code === x.code}
                            className={`adm-pick${i.code === x.code ? ' is-on' : ''}`}
                            onClick={() => set(i.key, { code: x.code, trackId: '', note: '', custom: !FIX_PRESETS[x.code]?.length })}
                          >{x.label}</button>
                        ))}
                      </div>
                    </>
                  )}
                  {o?.track && (
                    <>
                      <span className="adm-fix-label">트랙</span>
                      <div className="adm-picks" role="radiogroup" aria-label="트랙">
                        {tracks.map(t => (
                          <button
                            key={t.id} type="button" role="radio" aria-checked={i.trackId === t.id}
                            className={`adm-pick${i.trackId === t.id ? ' is-on' : ''}`}
                            onClick={() => set(i.key, { trackId: t.id })}
                          >{t.track_number}. {t.title}</button>
                        ))}
                      </div>
                    </>
                  )}
                  {i.code && (!o?.track || i.trackId) && <CurrentValue sheet={sheet} code={i.code} trackId={i.trackId || undefined} />}
                  {i.code && (
                    <>
                      <span className="adm-fix-label">안내 문구</span>
                      <div className="adm-reasons" role="radiogroup" aria-label="안내 문구">
                        {(FIX_PRESETS[i.code] ?? []).map(text => (
                          <button
                            key={text} type="button" role="radio" aria-checked={!i.custom && i.note === text}
                            className={`adm-reason${!i.custom && i.note === text ? ' is-on' : ''}`}
                            onClick={() => set(i.key, { note: text, custom: false })}
                          >
                            <span>{text}</span>
                            <span className="adm-reason-mark" aria-hidden="true">{!i.custom && i.note === text && <CheckIcon size={11} />}</span>
                          </button>
                        ))}
                        <button
                          type="button" role="radio" aria-checked={!!i.custom}
                          className={`adm-reason${i.custom ? ' is-on' : ''}`}
                          onClick={() => set(i.key, { custom: true, note: i.custom ? i.note : '' })}
                        >
                          <span>직접 입력</span>
                          <span className="adm-reason-mark" aria-hidden="true">{i.custom && <CheckIcon size={11} />}</span>
                        </button>
                      </div>
                    </>
                  )}
                </div>
              )}
              {(i.system || i.custom) && (
                <textarea
                  className="adm-textarea" rows={2} maxLength={2000} value={i.note} aria-label="문제 내용"
                  autoFocus={!i.system}
                  placeholder={i.code ? correctionTarget(i.code).hint : '무엇이 문제이고 어떻게 고치면 되는지 적어 주세요.'}
                  onChange={e => set(i.key, { note: e.target.value })}
                />
              )}
            </div>
          );
        })}
      </div>
      <button type="button" className="adm-btn soft small" style={{ marginTop: 10 }} onClick={() => setItems(list => [...list, newFix()])}>+ 항목 추가</button>
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="cSummary">전체 안내 <span className="muted">(선택)</span></label>
        <textarea id="cSummary" className="adm-textarea" rows={2} maxLength={MAX_REASON} value={summary} placeholder="비워 두면 ‘표시된 항목을 고친 뒤 다시 접수해 주세요.’로 보내요." onChange={e => setSummary(e.target.value)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className="adm-btn primary" disabled={busy || !ready.length || incomplete}>{busy ? '보내는 중…' : `보완 요청 보내기 (${ready.length}건)`}</button>
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

function WithdrawForm({ sheet, onDone }: { sheet: ReleaseSheet; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [reason, setReason] = useState('아티스트 문의로 취소 요청');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!reason.trim() || busy) return;
    setBusy(true);
    try {
      await staffApi.withdraw(sheet.release.id, reason.trim());
      toast('발매 신청을 취소했어요. 아티스트에게 알림이 갔어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '취소하지 못했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">아티스트가 문의로 요청한 취소를 처리해요. 아티스트의 월 3회 직접 취소 횟수에는 포함되지 않아요. 계약서 서명 후(배급 시작)에는 테이크다운으로 처리해야 해요.</p>
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="wReason">처리 메모 <span className="required">*</span></label>
        <input id="wReason" data-autofocus className="adm-input" maxLength={2000} required value={reason} onChange={e => setReason(e.target.value)} />
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>닫기</button>
        <button type="submit" className="adm-btn solid-danger" disabled={busy || !reason.trim()}>{busy ? '처리 중…' : '신청 취소 처리'}</button>
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
      <p className="small muted">테스트용 임시 UPC·ISRC로 준비된 배급을 정식 코드로 다시 만들어요. 발매는 ‘배포 보완 요청’ 상태가 되고, 재접수 시 정식 코드가 발급돼요.</p>
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
  const [modal, setModal] = useState<'proof' | 'reissue' | 'withdraw' | null>(null);
  const [done, setDone] = useState('');
  const [showAll, setShowAll] = useState(false);

  if (loading && !sheet) return <Skeleton rows={6} />;
  if (error && !sheet) return <ErrorBox message={error} onRetry={reload} />;
  if (!sheet) return null;

  const r = sheet.release;
  const app = sheet.application;
  const inReview = r.status === 'STAGE2_REVIEW';
  // 자동 검사를 통과했고 신청서(배급 계약서)가 검토 전인 새 발매 신청
  const application = !inReview && r.status === 'READY_FOR_DELIVERY'
    && sheet.documents.some(d => d.kind === 'AGREEMENT' && applicationPending(d.status));
  const decidable = inReview || application;
  const stages = systemStages(r.status);
  const passed = sheet.checks.filter(c => c.status === 'PASS' || c.status === 'NOT_APPLICABLE').length;
  const pending = sheet.second_approvals.find(a => a.status === 'PENDING');
  const canReview = can('REVIEW');
  const after = (msg: string) => { setDone(msg); reload(); refreshCounts(); };

  return (
    <div className="view-enter">
      <Link to="/admin/reviews" className="adm-back"><Glyph name="arrow-left" size={14} className="aq-inline-glyph" />심사 목록</Link>

      <div className="adm-detail">
        <div>
          <div className="adm-release-hero">
            <Initial text={r.title} src={r.cover} />
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

          <Section title="시스템 검사" meta={`검사 ${sheet.checks.length}개 중 ${passed}개 자동 통과`}>
            <ol className="adm-stages">
              {stages.map((st, i) => (
                <li key={st.key} className={`is-${st.state}`}>
                  <span className="adm-stage-dot" aria-hidden="true">{st.state === 'done' ? <CheckIcon size={13} /> : st.state === 'stopped' ? <Glyph name="close" size={13} /> : i + 1}</span>
                  <span className="adm-min">
                    <b>{st.label} {stageStateLabel(st.state)}</b>
                    <small>{st.hint}</small>
                  </span>
                </li>
              ))}
            </ol>
            {decidable && sheet.open_checks.length === 0 && (
              <div className="adm-alert is-ok" style={{ marginTop: 12 }}>시스템 검사를 모두 통과했어요. 신청 정보와 커버·음원만 확인하고 최종 결정해 주세요.</div>
            )}
          </Section>


          {sheet.open_checks.length > 0 && (
            <Section title="담당자 확인 필요" meta={`${sheet.open_checks.length}건 · 시스템이 판단을 넘긴 항목`}>
              <div className="adm-checks">{sheet.open_checks.map(c => <CheckCard key={c.check_code} c={c} open />)}</div>
            </Section>
          )}

          {sheet.advisories.length > 0 && (
            <Section title="참고 사항" meta="발매를 막지 않는 음원 권고">
              <div className="adm-checks">{sheet.advisories.map(c => <CheckCard key={c.check_code} c={c} />)}</div>
            </Section>
          )}

          <ApplicationSection sheet={sheet} />
          <EnteredInfoSection sheet={sheet} />
          <OptionsSection sheet={sheet} />
          <TracksSection sheet={sheet} />

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
                    <b title={t.reason ?? undefined}>{ACTION_KO[t.action] ?? t.action}</b>
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
                ? `담당자 확인 필요 ${sheet.open_checks.length}건`
                : application
                  ? '새 발매 신청 · 시스템 검사 모두 통과'
                  : `지금은 ‘${pick(RELEASE_STATUS, r.status)[0]}’ 상태라 결정할 수 없어요.`}
            </p>
            <div className="adm-decide-actions">
              <button type="button" className="adm-btn primary" disabled={!decidable || !canReview || !!pending} onClick={() => setAction('APPROVE')}>승인</button>
              <button type="button" className="adm-btn warn" disabled={!decidable || !canReview} onClick={() => setAction('REQUEST_CORRECTION')}>보완 요청</button>
              <button type="button" className="adm-btn danger" disabled={!decidable || !canReview} onClick={() => setAction('REJECT')}>거절</button>
            </div>
            <div className="adm-decide-note">
              {application
                ? '승인하면 신청서(배급 계약서)가 승인되고 아티스트가 서명하면 배급이 시작돼요. 추가 서류가 필요하면 ‘권리 증빙 요청’으로 요청하세요 — 서류 검토에서 확인해요.'
                : '승인하면 시스템이 남은 검사와 배급 준비를 이어서 해요. 권리·중복 등 민감 항목은 다른 담당자의 2차 승인이 필요해요.'}
            </div>
          </div>
          {!canReview && <NoDuty duty="발매 심사" />}
          {WITHDRAWABLE.includes(r.status) && canReview && !sheet.documents.some(d => d.kind === 'AGREEMENT' && d.status === 'SIGNED') && (
            <div className="adm-card">
              <b>취소 요청 처리</b>
              <p className="small muted" style={{ margin: '6px 0 12px' }}>아티스트가 문의로 취소를 요청했을 때 써요.</p>
              <button type="button" className="adm-btn soft small" onClick={() => setModal('withdraw')}>신청 취소 처리</button>
            </div>
          )}
          {r.status === 'READY_FOR_DELIVERY' && canReview && (
            <div className="adm-card">
              <b>식별자 재발급</b>
              <p className="small muted" style={{ margin: '6px 0 12px' }}>테스트용 임시 UPC·ISRC로 준비된 배급을 정식 코드로 다시 만들어요.</p>
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
        <Modal title={ACTION_TITLE[action]} onClose={() => setAction(null)} dismissible={false}>
          {action === 'APPROVE' && <ApproveForm sheet={sheet} application={application} onDone={after} />}
          {action === 'REJECT' && <RejectForm sheet={sheet} onDone={after} />}
          {action === 'REQUEST_CORRECTION' && <CorrectionForm sheet={sheet} onDone={after} />}
        </Modal>
      )}
      {modal === 'proof' && (
        <Modal title="권리 증빙 요청" onClose={() => setModal(null)} dismissible={false}>
          <ProofForm sheet={sheet} onDone={() => after('권리 증빙을 요청했어요.')} />
        </Modal>
      )}
      {modal === 'withdraw' && (
        <Modal title="신청 취소 처리" onClose={() => setModal(null)} dismissible={false}>
          <WithdrawForm sheet={sheet} onDone={() => after('발매 신청을 취소했어요.')} />
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
