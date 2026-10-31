// 퍼블리싱 요청 — 아티스트가 퍼블리싱에서 요청한 것에 담당자가 답하는 곳 (0173). 한 메뉴 안의 탭:
//  · 지분 변경: 관리 중인 곡의 변경 요청 — 지금 적용값 → 변경값을 보고 반영하거나 사유와 함께 반려
//  · 계약 요청: 연장(종료일이 늘어난다) · 종료 요청 — 처리하거나 사유와 함께 반려
//  · Open A&R: 데모를 듣고 담당 → 보완 요청 · 심사 완료 → 제안 → 종료
//  · 저작권료: 관리 단체 명세서(CSV)를 올려 곡별 저작권료로 — 먼저 확인(맞지 않는 줄은 이유와 함께), 그다음 올리기. 정산 담당만
// 모양은 저작권 관리 신청 심사와 같다: 목록 줄(ui-row), 결정은 창(Modal) 하나, 사유는 반려·보완에만 필수
import { useState, type ChangeEvent } from 'react';
import { useSearchParams } from '../../lib/router';
import { Modal } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { Glyph } from '../../components/Glyph';
import { FilePicker } from '../../components/FilePicker';
import { SplitDiffList } from '../../components/publishing/WorkSheet';
import { splitDiff } from '../../lib/publishingPrep';
import { parseCsv } from '../../lib/workImport';
import { SOCIETIES, societyName } from '../../lib/publishing';
import { money } from '../../lib/format';
import { staffApi, type ArDecisionAction, type PubChange, type PubContractRequest, type RoyaltyImportResult, type StaffArSubmission } from '../api';
import { ago } from '../labels';
import { Badge, EmptyState, ErrorBox, PageHeader, Seg, SkeletonList, StatusBadge, useStaff } from '../ui';
import type { Tone } from '../../ui';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

type Tab = 'changes' | 'contracts' | 'ar' | 'royalties';
const AR_STATUS: Record<StaffArSubmission['status'], [string, Tone]> = {
  submitted: ['접수', 'blue'], in_review: ['심사 중', 'violet'], needs_fix: ['보완 요청', 'amber'], reviewed: ['심사 완료', 'green'], proposing: ['제안 중', 'green'], closed: ['종료', 'gray'],
};
const AR_ACTIONS: Record<StaffArSubmission['status'], [ArDecisionAction, string][]> = {
  submitted: [['CLAIM', '담당하기']], in_review: [['FIX', '보완 요청'], ['REVIEWED', '심사 완료']], needs_fix: [], reviewed: [['PROPOSING', '제안 시작']], proposing: [], closed: [],
};
const KIND: Record<string, string> = { 공연: 'performance', 공연권: 'performance', 복제: 'mechanical', 복제권: 'mechanical', 전송: 'digital', 전송권: 'digital', 싱크: 'sync', 유튜브: 'youtube', 출판: 'print', 기타: 'other' };

/** 반영·반려처럼 예/아니오 결정 — 아니오는 사유가 필요하다 */
function Decide({ title, approveLabel, rejectLabel, onDecide, onClose, children }: {
  title: string; approveLabel: string; rejectLabel: string; onDecide: (approve: boolean, reason: string) => Promise<void>; onClose: () => void; children: React.ReactNode;
}) {
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const go = async (approve: boolean) => { setBusy(true); try { await onDecide(approve, reason.trim()); onClose(); } finally { setBusy(false); } };
  return (
    <Modal title={title} onClose={onClose} dismissible={!busy}>
      <div className="aq-track-sheet">
        {children}
        <div className="ui-field">
          <label htmlFor="opsReason">사유 <small className="muted">(반려할 때 필수 · 아티스트에게 그대로 보여요)</small></label>
          <textarea id="opsReason" className="ui-textarea" rows={3} maxLength={1000} value={reason} onChange={e => setReason(e.target.value)} />
        </div>
        <div className="aq-confirm-actions">
          <button type="button" className="ui-btn is-soft" disabled={busy || !reason.trim()} onClick={() => void go(false)}>{rejectLabel}</button>
          <button type="button" className="ui-btn is-primary" disabled={busy} onClick={() => void go(true)}>{approveLabel}</button>
        </div>
      </div>
    </Modal>
  );
}

function Changes() {
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => staffApi.pubChanges(), []);
  const [open, setOpen] = useState<PubChange | null>(null);
  if (error) return <ErrorBox message={error} onRetry={reload} />;
  if (loading && !data) return <SkeletonList />;
  if (!data?.items.length) return <EmptyState title="기다리는 지분 변경이 없어요">관리 중인 곡의 지분을 아티스트가 바꾸면 이곳에 쌓여요.</EmptyState>;
  return (
    <>
      <div className="ui-list">
        {data.items.map(c => (
          <button key={c.work.id} type="button" className="ui-row" onClick={() => setOpen(c)}>
            <span className="ui-icon" aria-hidden="true"><Glyph name="scale" size={18} /></span>
            <span className="adm-min">
              <span className="ui-row-title">{c.work.title}</span>
              <span className="ui-row-meta"><span>{c.orgName}</span><span className="is-mono">{c.work.internalCode}</span></span>
            </span>
            <span className="ui-row-end"><small>요청 {ago(c.work.pendingChange?.requestedAt ?? '') || '—'}</small></span>
          </button>
        ))}
      </div>
      {open && open.work.pendingChange && (
        <Decide title={`‘${open.work.title}’ 지분 변경`} approveLabel="반영" rejectLabel="반려" onClose={() => setOpen(null)}
          onDecide={async (approve, reason) => {
            try { await staffApi.pubChangeDecide(open.work.id, { approve, reason }); toast(approve ? '반영했어요. 지분이 바뀐 참여자는 다시 확인해요.' : '반려했어요.', 'success'); reload(); }
            catch (e) { toast(errorMessage(e), 'error'); throw e; }
          }}>
          <p className="ui-field-help">{open.orgName} · 반영하면 바로 이 값으로 관리하고, 지분이 바뀐 공동 작가에게는 다시 확인을 받아요.</p>
          <SplitDiffList title="지금 적용값 → 변경값" changes={splitDiff(open.work.contributors, open.work.pendingChange.contributors)} />
        </Decide>
      )}
    </>
  );
}

function Contracts() {
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => staffApi.pubContractRequests(), []);
  const [open, setOpen] = useState<PubContractRequest | null>(null);
  if (error) return <ErrorBox message={error} onRetry={reload} />;
  if (loading && !data) return <SkeletonList />;
  if (!data?.items.length) return <EmptyState title="기다리는 계약 요청이 없어요">아티스트가 연장이나 종료를 요청하면 이곳에 쌓여요.</EmptyState>;
  const kind = (c: PubContractRequest) => (c.renewal ? 'renewal' : 'termination') as 'renewal' | 'termination';
  return (
    <>
      <div className="ui-list">
        {data.items.map(c => (
          <button key={c.id} type="button" className="ui-row" onClick={() => setOpen(c)}>
            <span className="ui-icon" aria-hidden="true"><Glyph name="sign" size={18} /></span>
            <span className="adm-min">
              <span className="ui-row-title">{c.works.join(', ') || c.number}</span>
              <span className="ui-row-meta"><span>{c.orgName}</span><span>종료일 {c.endDate ?? '—'}</span><span className="is-mono">{c.number}</span></span>
            </span>
            <span className="ui-row-end">
              {c.renewal ? <Badge tone="blue">{c.renewal.months}개월 연장</Badge> : <Badge tone="red">종료 요청</Badge>}
              <small>{ago(c.renewal?.requestedAt ?? c.terminationRequestedAt ?? '') || '—'}</small>
            </span>
          </button>
        ))}
      </div>
      {open && (
        <Decide title={kind(open) === 'renewal' ? '계약 연장 요청' : '계약 종료 요청'} approveLabel={kind(open) === 'renewal' ? '연장 처리' : '종료 처리'} rejectLabel="반려"
          onClose={() => setOpen(null)}
          onDecide={async (approve, reason) => {
            try { await staffApi.pubContractDecide(open.id, { kind: kind(open), approve, reason }); toast(approve ? '처리했어요.' : '반려했어요.', 'success'); reload(); }
            catch (e) { toast(errorMessage(e), 'error'); throw e; }
          }}>
          <dl className="ui-kv">
            <div><dt>계약</dt><dd>{open.number} · {open.orgName}</dd></div>
            <div><dt>곡</dt><dd>{open.works.join(', ')}</dd></div>
            <div><dt>지금 종료일</dt><dd>{open.endDate ?? '—'}</dd></div>
            <div><dt>요청</dt><dd>{open.renewal ? `${open.renewal.months}개월 연장 — 종료일이 그만큼 늘어나요` : '종료 — 관리 단체 절차를 마친 뒤 처리해 주세요. 처리하면 오늘로 끝나요'}</dd></div>
          </dl>
        </Decide>
      )}
    </>
  );
}

function OpenAr() {
  const toast = useToast();
  const [status, setStatus] = useState<'open' | 'all'>('open');
  const { data, loading, error, reload } = useAsync(() => staffApi.pubArList(), []);
  const [open, setOpen] = useState<StaffArSubmission | null>(null);
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  if (error) return <ErrorBox message={error} onRetry={reload} />;
  if (loading && !data) return <SkeletonList />;
  const items = (data?.items ?? []).filter(a => status === 'all' || !['closed', 'proposing'].includes(a.status));
  const act = async (a: StaffArSubmission, action: ArDecisionAction) => {
    setBusy(true);
    try { const n = await staffApi.pubArDecide(a.id, { action, reason: reason.trim() }); setOpen(n); setReason(''); toast('처리했어요.', 'success'); reload(); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <>
      <Seg label="보기" value={status} onChange={setStatus} options={[['open', '진행 중'], ['all', '전체']]} size="sm" />
      {!items.length ? <EmptyState title="심사할 제출이 없어요">아티스트가 발매 전 곡의 데모를 보내면 이곳에 쌓여요.</EmptyState> : (
        <div className="ui-list">
          {items.map(a => (
            <button key={a.id} type="button" className="ui-row" onClick={() => { setOpen(a); setReason(''); }}>
              <span className="ui-icon" aria-hidden="true"><Glyph name="megaphone" size={18} /></span>
              <span className="adm-min">
                <span className="ui-row-title">{a.workTitle}</span>
                <span className="ui-row-meta"><span>{a.orgName}</span><span>{a.genre} · {a.mood.join(' · ')}</span>{a.sync && <span>싱크 소개 동의</span>}</span>
              </span>
              <span className="ui-row-end"><StatusBadge value={AR_STATUS[a.status]} /><small>{ago(a.submittedAt) || '—'}</small></span>
            </button>
          ))}
        </div>
      )}
      {open && (
        <Modal title={`${open.workTitle} · Open A&R`} onClose={() => setOpen(null)} dismissible={!busy}>
          <div className="aq-track-sheet">
            <dl className="ui-kv">
              <div><dt>작업 공간</dt><dd>{open.orgName}</dd></div>
              <div><dt>데모</dt><dd>{open.demoName ?? '—'}</dd></div>
              <div><dt>장르·분위기</dt><dd>{open.genre} · {open.mood.join(' · ')}</dd></div>
              <div><dt>공개 범위</dt><dd>{open.scope === 'partners' ? '승인 파트너까지' : 'AUDENIQ 내부만'}{open.sync && ' · 싱크 소개 동의'}</dd></div>
              {open.description && <div><dt>곡 설명</dt><dd>{open.description}</dd></div>}
              {open.references && <div><dt>참고 자료</dt><dd>{open.references}</dd></div>}
              {open.fixReply && <div><dt>보완 답변</dt><dd>{open.fixReply}</dd></div>}
            </dl>
            {open.lyrics && <p className="aq-track-sheet-lyrics aq-pbl-file-text">{open.lyrics}</p>}
            <ol className="ui-timeline is-latest-first">{[...open.history].reverse().map((h, i) => <li key={i}><strong>{h.text}</strong><small>{ago(h.at)}</small></li>)}</ol>
            {AR_ACTIONS[open.status].some(([k]) => k === 'FIX') && (
              <div className="ui-field">
                <label htmlFor="arReason">보완 요청 사유 <small className="muted">(보완 요청에 필수)</small></label>
                <textarea id="arReason" className="ui-textarea" rows={3} maxLength={1000} value={reason} onChange={e => setReason(e.target.value)} />
              </div>
            )}
            <div className="aq-confirm-actions">
              {open.status !== 'closed' && <button type="button" className="ui-btn is-text aq-danger-text" disabled={busy} onClick={() => void act(open, 'CLOSE')}>종료</button>}
              {AR_ACTIONS[open.status].map(([k, label]) => (
                <button key={k} type="button" className={`ui-btn ${k === 'FIX' ? 'is-soft' : 'is-primary'}`} disabled={busy || (k === 'FIX' && !reason.trim())} onClick={() => void act(open, k)}>{label}</button>
              ))}
            </div>
          </div>
        </Modal>
      )}
    </>
  );
}

function Royalties() {
  const toast = useToast();
  const { data, loading, error, reload } = useAsync(() => staffApi.pubStatements(), []);
  const [source, setSource] = useState('KOMCA');
  const [period, setPeriod] = useState(() => new Date(Date.now() - 31 * 86_400_000).toISOString().slice(0, 7));
  const [status, setStatus] = useState<'estimated' | 'confirmed'>('estimated');
  const [text, setText] = useState('');
  const [fileName, setFileName] = useState<string | undefined>();
  const [check, setCheck] = useState<RoyaltyImportResult | null>(null);
  const [busy, setBusy] = useState(false);
  // 표 → 줄: 곡 코드 · 종류 · 국가 · 금액 (첫 줄은 칸 이름). 종류는 한국어(공연·전송…)나 영문 키
  const rows = () => {
    const [head, ...body] = parseCsv(text);
    if (!head) return [];
    const col = (...names: string[]) => head.findIndex(h => names.includes(h.trim()));
    const [w, k, c, m] = [col('곡 코드', 'work', 'code'), col('종류', 'kind'), col('국가', 'country'), col('금액', 'collected', 'amount')];
    return body.map(r => ({ work: (r[w] ?? '').trim(), kind: KIND[(r[k] ?? '').trim()] ?? (r[k] ?? '').trim(), country: (r[c] ?? '').trim().toUpperCase() || 'KR', collected: Math.round(Number((r[m] ?? '').replace(/[,\s원]/g, ''))) || 0 }));
  };
  const run = async (preview: boolean) => {
    const list = rows();
    if (!list.length) { toast('표를 넣어 주세요. 첫 줄은 칸 이름(곡 코드, 종류, 국가, 금액)이에요.', 'error'); return; }
    setBusy(true);
    try {
      const r = await staffApi.pubRoyaltyImport({ source, period, status, fileName, rows: list, preview });
      setCheck(r);
      if (!preview) { toast(`${list.length}줄을 올렸어요.`, 'success'); setText(''); setCheck(null); reload(); }
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  const pick = async (e: ChangeEvent<HTMLInputElement>) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) { setText(await f.text()); setFileName(f.name); setCheck(null); } };
  const mark = async (sid: string, next: 'confirmed' | 'paid') => {
    try { await staffApi.pubStatementStatus(sid, next); toast(next === 'paid' ? '지급 완료로 표시했어요.' : '확정했어요.', 'success'); reload(); } catch (e) { toast(errorMessage(e), 'error'); }
  };
  const total = check?.rows.reduce((n, r) => n + (r.collected ?? 0), 0) ?? 0;
  return (
    <>
      <section className="ui-card adm-section">
        <h2 className="subhead">명세서 올리기</h2>
        <p className="ui-field-help">관리 단체가 보낸 명세서를 표로 올리면 곡 코드(AQW-… 또는 ISWC)로 곡을 찾아 곡별 저작권료를 만들어요. 수수료는 그 곡 계약서의 비율로 떼요. 맞지 않는 줄이 하나라도 있으면 올리지 않아요.</p>
        <div className="form-grid">
          <div className="ui-field"><label htmlFor="rsSource">징수 단체</label>
            <select id="rsSource" className="ui-select" value={source} onChange={e => setSource(e.target.value)}>{SOCIETIES.filter(s => s !== '없음').map(s => <option key={s} value={s}>{societyName(s)}</option>)}<option value="YouTube">YouTube</option></select></div>
          <div className="ui-field"><label htmlFor="rsPeriod">사용 기간 (월)</label><input id="rsPeriod" type="month" className="ui-input" value={period} onChange={e => setPeriod(e.target.value)} /></div>
          <div className="ui-field"><label>상태</label><Seg label="상태" value={status} onChange={setStatus} options={[['estimated', '예상'], ['confirmed', '확정']]} size="sm" /></div>
        </div>
        <div className="ui-field"><label htmlFor="rsText">표 붙여 넣기</label>
          <textarea id="rsText" className="ui-textarea" rows={6} spellCheck={false} value={text} onChange={e => { setText(e.target.value); setCheck(null); }} placeholder={'곡 코드,종류,국가,금액\nAQW-000004,전송,KR,178200\nT-345.246.800-1,공연,JP,34650'} /></div>
        <div className="ui-field"><label htmlFor="rsFile">또는 CSV 파일</label><FilePicker id="rsFile" accept=".csv,text/csv" onChange={e => void pick(e)} /></div>
        {check && (
          <>
            <p className={`ui-field-help${check.errors ? ' is-error' : ''}`}>{check.errors ? `맞지 않는 줄 ${check.errors}개 — 고친 뒤 다시 확인해 주세요.` : `${check.rows.length}줄 모두 맞아요 · 합계 ${money(total)}`}</p>
            <div className="ui-list is-inset">
              {check.rows.slice(0, 50).map(r => (
                <div key={r.line} className="ui-row is-flat">
                  <span className="adm-min"><span className="ui-row-title">{r.line}줄 · {r.workTitle ?? r.work}</span>
                    <span className="ui-row-meta">{r.error ? <span className="aq-danger-text">{r.error}</span> : <><span>{r.orgName}</span><span>징수 {money(r.collected ?? 0)}</span><span>수수료 {money(r.fee ?? 0)}</span><span>아티스트 {money(r.net ?? 0)}</span></>}</span></span>
                </div>
              ))}
            </div>
          </>
        )}
        <div className="aq-confirm-actions">
          <button type="button" className="ui-btn is-soft" disabled={busy || !text.trim()} onClick={() => void run(true)}>확인</button>
          <button type="button" className="ui-btn is-primary" disabled={busy || !check || check.errors > 0} onClick={() => void run(false)}>올리기</button>
        </div>
      </section>
      <h2 className="subhead">올린 명세서</h2>
      {error ? <ErrorBox message={error} onRetry={reload} /> : loading && !data ? <SkeletonList /> : !data?.items.length ? <EmptyState title="올린 명세서가 없어요">명세서를 올리면 아티스트의 저작권료 리포트에 바로 보여요.</EmptyState> : (
        <div className="ui-list">
          {data.items.map(st => (
            <div key={st.id} className="ui-row is-flat">
              <span className="ui-icon" aria-hidden="true"><Glyph name="won" size={18} /></span>
              <span className="adm-min">
                <span className="ui-row-title">{societyName(st.source)} · {st.period}</span>
                <span className="ui-row-meta"><span>{st.rowCount}줄</span><span>징수 {money(st.totalCollected)}</span>{st.fileName && <span>{st.fileName}</span>}<span>{ago(st.importedAt)}</span></span>
              </span>
              <span className="ui-row-end">
                <StatusBadge value={st.status === 'paid' ? ['지급 완료', 'green'] : st.status === 'confirmed' ? ['확정', 'blue'] : ['예상', 'gray']} />
                {st.status === 'estimated' && <button type="button" className="ui-btn is-soft is-sm" onClick={() => void mark(st.id, 'confirmed')}>확정</button>}
                {st.status === 'confirmed' && <button type="button" className="ui-btn is-soft is-sm" onClick={() => void mark(st.id, 'paid')}>지급 완료</button>}
              </span>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

export function PublishingOps() {
  const { can } = useStaff();
  const [params, setParams] = useSearchParams();
  const finance = can('FINANCE');
  const tabs: [Tab, string][] = [['changes', '지분 변경'], ['contracts', '계약 요청'], ['ar', 'Open A&R'], ...(finance ? [['royalties', '저작권료'] as [Tab, string]] : [])];
  const asked = params.get('tab') as Tab | null;
  const tab = tabs.some(([k]) => k === asked) ? asked! : 'changes';
  return (
    <div className="view-enter">
      <PageHeader eyebrow="퍼블리싱" title="퍼블리싱 요청" sub="아티스트가 퍼블리싱에서 요청한 것에 답하고, 관리 단체 명세서로 저작권료를 올려요." />
      <Seg label="퍼블리싱 요청" value={tab} onChange={v => setParams({ tab: v }, { replace: true })} options={tabs} />
      <div className="adm-pub-ops">
        {tab === 'changes' && <Changes />}
        {tab === 'contracts' && <Contracts />}
        {tab === 'ar' && <OpenAr />}
        {tab === 'royalties' && finance && <Royalties />}
      </div>
    </div>
  );
}
