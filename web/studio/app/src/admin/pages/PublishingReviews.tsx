// 저작권 관리 신청(퍼블리싱) 심사 — '발매 심사' 메뉴 안의 하위 탭. 새 메뉴를 두지 않고 발매 심사와 같은 틀을 쓴다:
// 목록은 상태 칩 · 검색 · 오래 기다린 순, 상세는 왼쪽 내용 + 오른쪽 어두운 '심사 결정' 판(휴대폰은 아래 고정 버튼),
// 결정은 발매와 같은 말(담당하기 → 승인 · 보완 요청 · 반려)에 퍼블리싱만의 '관리 단체 등록'을 더했다.
// 참여자·지분은 스튜디오와 같은 SplitEditor(읽기 전용)로 보여 준다 — 아티스트가 본 그대로
import { useState } from 'react';
import { Link, useParams, useSearchParams } from '../../lib/router';
import { Modal } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { Glyph } from '../../components/Glyph';
import { SplitEditor } from '../../components/publishing/SplitEditor';
import { RegBadge } from '../../components/publishing/parts';
import { SOCIETIES, pct, splitReady, sumShares } from '../../lib/publishing';
import { staffApi, type PubAppStatus, type PubApplication, type PubDecisionAction } from '../api';
import { ago, when } from '../labels';
import { Badge, ChipGroup, EmptyState, ErrorBox, NoDuty, PageHeader, ReviewTabs, Section, SkeletonList, StatusBadge, useStaff } from '../ui';
import type { Tone } from '../../ui';
import '../../styles/split/Publishing.css';

export const PUB_STATUS: Record<PubAppStatus, [string, Tone]> = {
  submitted: ['접수', 'blue'], in_review: ['심사 중', 'violet'], needs_fix: ['보완 요청', 'amber'],
  approved: ['승인', 'green'], rejected: ['반려', 'red'], withdrawn: ['철회', 'gray'],
};
const FILTERS: PubAppStatus[] = ['submitted', 'in_review', 'needs_fix', 'approved', 'rejected'];
const TERRITORY: Record<string, string> = { KR: '대한민국', JP: '일본', US: '미국', WORLD: '전 세계' };
const RIGHT: Record<string, string> = { performance: '공연권', mechanical: '복제권', digital: '전송권', sync: '싱크' };
const DOCS = ['공동 작곡 합의서', '공동 작사 합의서', '기존 퍼블리셔 해지 확인서', '편곡·번안 허락서'];
const ACTION_TITLE: Record<PubDecisionAction, string> = { APPROVE: '승인', REQUEST_CORRECTION: '보완 요청', REJECT: '반려', REGISTER: '관리 단체 등록' };

export function PublishingReviews() {
  const [params, setParams] = useSearchParams();
  const status = (FILTERS.includes(params.get('status') as PubAppStatus) ? params.get('status') : 'submitted') as PubAppStatus;
  const [search, setSearch] = useState('');
  const { data, loading, error, reload } = useAsync(() => staffApi.pubApplications(status), [status]);
  const term = search.trim().toLowerCase();
  const shown = (data?.items ?? []).filter(a => !term || [a.orgName, a.number, ...a.workTitles].some(v => v.toLowerCase().includes(term)));

  return (
    <div className="view-enter">
      <PageHeader eyebrow="발매 심사" title="저작권 관리 신청"
        sub="곡의 작사·작곡 저작권을 맡기겠다는 신청이에요. 참여자와 지분이 맞는지 확인해 주세요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>} />
      <ReviewTabs />
      <ChipGroup label="신청 상태" value={status} onChange={v => setParams({ status: v }, { replace: true })}
        options={FILTERS.map(s => ({ value: s, label: PUB_STATUS[s][0] }))} />
      <div className="adm-queue-tools">
        <label htmlFor="pubSearch" className="sr-only">검색</label>
        <input id="pubSearch" type="search" className="ui-input" placeholder="곡·작업 공간·신청 번호로 찾기" value={search} onChange={e => setSearch(e.target.value)} />
      </div>
      {!loading && !!data?.items.length && <p className="adm-queue-count">{term ? `‘${search.trim()}’ 검색 결과 ${shown.length}건` : `${shown.length}건`}</p>}
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList /> : !shown.length ? (
        <EmptyState title={status === 'submitted' ? '새로 들어온 신청이 없어요' : '해당 상태의 신청이 없어요'}>
          {status === 'submitted' ? '아티스트가 저작권 관리를 신청하면 이곳에 쌓여요.' : '다른 상태를 선택해 보세요.'}
        </EmptyState>
      ) : (
        <div className="ui-list">
          {shown.map(a => (
            <Link key={a.id} to={`/admin/publishing/${a.id}`} className="ui-row">
              <span className="ui-icon" aria-hidden="true"><Glyph name="scale" size={18} /></span>
              <span className="adm-min">
                <span className="ui-row-title">{a.workTitles.slice(0, 2).join(', ')}{a.workTitles.length > 2 && ` 외 ${a.workTitles.length - 2}곡`}</span>
                <span className="ui-row-meta">
                  <span>{a.orgName}</span>
                  <span>{a.workTitles.length}곡</span>
                  <span>{a.territories.map(t => TERRITORY[t] ?? t).join('·')}</span>
                  <span className="is-mono">{a.number}</span>
                </span>
              </span>
              <span className="ui-row-end">
                {a.replyNote && a.status === 'in_review' && <Badge tone="blue">보완 제출</Badge>}
                <StatusBadge value={PUB_STATUS[a.status]} />
                <small className={(Date.now() - Date.parse(a.submittedAt)) / 3_600_000 >= 48 && ['submitted', 'in_review'].includes(a.status) ? 'is-late' : undefined}>접수 {ago(a.submittedAt) || '—'}</small>
              </span>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

/** 결정 창 — 사유 · (보완이면) 곡과 서류 · (등록이면) 관리 단체 */
function DecisionForm({ app, action, onDone }: { app: PubApplication; action: PubDecisionAction; onDone: (msg: string) => void }) {
  const toast = useToast();
  const [reason, setReason] = useState('');
  const [works, setWorks] = useState<string[]>(app.workIds);
  const [docs, setDocs] = useState<string[]>([]);
  const [other, setOther] = useState('');
  const [society, setSociety] = useState('KOMCA');
  const [busy, setBusy] = useState(false);
  const needReason = action === 'REQUEST_CORRECTION' || action === 'REJECT';
  const toggle = (list: string[], set: (v: string[]) => void, v: string) => set(list.includes(v) ? list.filter(x => x !== v) : [...list, v]);
  const submit = async () => {
    setBusy(true);
    try {
      const documents = [...docs, ...other.split(',').map(d => d.trim()).filter(Boolean)];
      await staffApi.pubDecide(app.id, { action, reason: reason.trim(), work_ids: action === 'REQUEST_CORRECTION' ? works : undefined, documents: action === 'REQUEST_CORRECTION' ? documents : undefined, society: action === 'REGISTER' ? society : undefined });
      onDone(action === 'APPROVE' ? '승인했어요. 관리 단체 등록을 준비해 주세요.' : action === 'REQUEST_CORRECTION' ? `보완을 요청했어요.${documents.length ? ' 서류는 아티스트의 권리·보완 서류에 올라가요.' : ''}` : action === 'REJECT' ? '반려했어요.' : `${society} 등록 완료로 기록했어요.`);
    } catch (err) { toast(errorMessage(err, '처리하지 못했어요.'), 'error'); } finally { setBusy(false); }
  };
  return (
    <div className="adm-pub-form">
      {action === 'APPROVE' && <p className="small muted">참여자·지분과 권리 정보를 확인했다면 승인해요. 승인한 곡은 ‘등록 준비 완료’가 돼요.</p>}
      {action === 'REQUEST_CORRECTION' && app.workIds.length > 1 && (
        <fieldset className="ui-field">
          <legend>보완할 곡</legend>
          <div className="ui-chips is-wrap">
            {app.workIds.map((w, i) => <button key={w} type="button" className="ui-chip is-sm" aria-pressed={works.includes(w)} onClick={() => toggle(works, setWorks, w)}>{app.workTitles[i] ?? w}</button>)}
          </div>
        </fieldset>
      )}
      {needReason && (
        <div className="ui-field">
          <label htmlFor="pubReason">{action === 'REJECT' ? '반려 사유' : '아티스트에게 보낼 요청'}</label>
          <textarea id="pubReason" className="ui-textarea" rows={4} maxLength={2000} value={reason} onChange={e => setReason(e.target.value)}
            placeholder={action === 'REJECT' ? '예: 기존 퍼블리셔와의 계약이 남아 있어 관리할 수 없어요.' : '예: 공동 작곡가 김하늘 님의 IPI 번호를 적어 주세요.'} />
        </div>
      )}
      {action === 'REQUEST_CORRECTION' && (
        <fieldset className="ui-field">
          <legend>함께 받을 서류 (선택)</legend>
          <div className="ui-chips is-wrap">
            {DOCS.map(d => <button key={d} type="button" className="ui-chip is-sm" aria-pressed={docs.includes(d)} onClick={() => toggle(docs, setDocs, d)}>{d}</button>)}
          </div>
          <input className="ui-input" aria-label="그 밖의 서류" placeholder="그 밖의 서류 — 쉼표로 나눠 적어요" value={other} onChange={e => setOther(e.target.value)} />
          <small className="ui-field-help">고른 서류는 아티스트의 ‘권리·보완 서류’에 제출 요청으로 올라가요.</small>
        </fieldset>
      )}
      {action === 'REGISTER' && (
        <div className="ui-field">
          <label htmlFor="pubSociety">등록한 관리 단체</label>
          <select id="pubSociety" className="ui-select" value={society} onChange={e => setSociety(e.target.value)}>
            {SOCIETIES.filter(s => s !== '없음').map(s => <option key={s} value={s}>{s}</option>)}
          </select>
        </div>
      )}
      <div className="adm-form-actions">
        <button type="button" className={`ui-btn ${action === 'REJECT' ? 'is-danger' : action === 'REQUEST_CORRECTION' ? 'is-warn' : 'is-primary'}${busy ? ' is-busy' : ''}`}
          disabled={busy || (needReason && !reason.trim()) || (action === 'REQUEST_CORRECTION' && !works.length)} onClick={() => void submit()}>
          {busy ? '처리 중' : ACTION_TITLE[action]}
        </button>
      </div>
    </div>
  );
}

export function PublishingReviewDetail() {
  const { id = '' } = useParams();
  const toast = useToast();
  const { can, refreshCounts } = useStaff();
  const { data: app, loading, error, reload } = useAsync(() => staffApi.pubApplication(id), [id]);
  const [action, setAction] = useState<PubDecisionAction | null>(null);
  const [done, setDone] = useState('');
  const [claiming, setClaiming] = useState(false);
  if (loading && !app) return <SkeletonList rows={6} />;
  if (error && !app) return <ErrorBox message={error} onRetry={reload} />;
  if (!app) return null;

  const canReview = can('REVIEW');
  const open = app.status === 'submitted' || app.status === 'in_review';
  const actions: PubDecisionAction[] = app.status === 'in_review' ? ['APPROVE', 'REQUEST_CORRECTION', 'REJECT']
    : app.status === 'needs_fix' ? ['REJECT'] : app.status === 'approved' ? ['REGISTER'] : [];
  const works = app.works ?? [];
  const problems = works.filter(w => !splitReady(w.contributors));
  const missingIpi = works.flatMap(w => w.contributors.filter(c => !c.ipi).map(c => `${w.title} · ${c.name}`));
  const statusLine = app.status === 'submitted' ? '담당하면 심사를 시작할 수 있어요.'
    : app.status === 'in_review' ? (app.replyNote ? '아티스트가 보완해서 다시 냈어요.' : '참여자·지분·권리 정보를 확인하고 결정해 주세요.')
    : app.status === 'needs_fix' ? '아티스트의 보완을 기다리고 있어요.'
    : app.status === 'approved' ? '관리 단체에 등록한 뒤 등록 완료로 기록해 주세요.'
    : PUB_STATUS[app.status][0];
  const claim = async () => {
    setClaiming(true);
    try { await staffApi.pubClaim(app.id); toast('이 신청을 담당했어요. 이제 결정할 수 있어요.', 'success'); reload(); refreshCounts(); }
    catch (err) { toast(errorMessage(err, '담당하지 못했어요.'), 'error'); reload(); } finally { setClaiming(false); }
  };
  const after = (msg: string) => { setAction(null); setDone(msg); reload(); refreshCounts(); };
  const buttons = (
    <div className="adm-decide-actions">
      {app.status === 'submitted' && <button type="button" className={`ui-btn is-primary${claiming ? ' is-busy' : ''}`} disabled={claiming || !canReview} onClick={() => void claim()}>담당하기</button>}
      {actions.map(a => (
        <button key={a} type="button" className={`ui-btn ${a === 'REJECT' ? 'is-danger' : a === 'REQUEST_CORRECTION' ? 'is-warn' : 'is-primary'}`} disabled={!canReview} onClick={() => setAction(a)}>{ACTION_TITLE[a]}</button>
      ))}
    </div>
  );

  return (
    <div className="view-enter">
      <div className="adm-detail-nav">
        <Link to={`/admin/publishing?status=${app.status}`} className="adm-back"><Glyph name="arrow-left" size={14} className="aq-inline-glyph" />저작권 관리 신청</Link>
      </div>
      <div className="adm-detail">
        <div>
          <div className="adm-release-hero ui-on-soft">
            <span className="ui-icon is-lg" aria-hidden="true"><Glyph name="scale" size={26} /></span>
            <div className="adm-min">
              <div className="adm-codes" style={{ marginBottom: 6 }}>
                <StatusBadge value={PUB_STATUS[app.status]} />
                <Badge tone="gray">{works.length || app.workIds.length}곡</Badge>
              </div>
              <h1>{app.workTitles.join(', ')}</h1>
              <span className="ui-row-meta"><span>{app.orgName}</span><span className="is-mono">{app.number}</span></span>
              <span className="adm-hero-when">접수 {when(app.submittedAt)}</span>
            </div>
          </div>
          {done && <div className="ui-callout is-success" role="status">{done}</div>}
          {app.replyNote && <div className="ui-callout is-row"><span><b>아티스트 답변</b> · {app.replyNote}</span></div>}
          {app.fixReason && app.status !== 'in_review' && <div className={`ui-callout ${app.status === 'rejected' ? 'is-danger' : 'is-warn'}`}>{app.status === 'rejected' ? '반려 사유' : '보낸 보완 요청'} · {app.fixReason}</div>}
          {open && (problems.length > 0 || missingIpi.length > 0) && (
            <div className="ui-callout is-warn">
              {problems.length > 0 && <p><b>지분 확인 필요</b> · {problems.map(w => `${w.title} (합계 ${pct(sumShares(w.contributors))})`).join(', ')}</p>}
              {missingIpi.length > 0 && <p><b>IPI 없음</b> · {missingIpi.join(', ')}</p>}
            </div>
          )}

          <Section title="신청 정보">
            <dl className="ui-kv is-2">
              <div><dt>신청 번호</dt><dd className="is-mono">{app.number}</dd></div>
              <div><dt>작업 공간</dt><dd>{app.orgName}</dd></div>
              <div><dt>관리 지역</dt><dd>{app.territories.map(t => TERRITORY[t] ?? t).join(', ')}</dd></div>
              <div><dt>맡긴 권리</dt><dd>{app.rights.map(r => RIGHT[r] ?? r).join(', ')}</dd></div>
              {app.documentCode && <div className="is-wide"><dt>문서 확인 코드</dt><dd className="is-mono break">{app.documentCode}</dd></div>}
            </dl>
          </Section>

          {works.map(w => (
            <Section key={w.id} title={w.title} meta={[w.iswc ?? w.internalCode, w.recordings[0]?.releaseTitle].filter(Boolean).join(' · ')}
              action={<RegBadge status={w.registration} />}>
              {w.fix && <div className="ui-callout is-warn">{w.fix.reason}{w.fix.documents?.length ? ` · 서류: ${w.fix.documents.join(', ')}` : ''}</div>}
              <SplitEditor value={w.contributors} onChange={() => {}} readOnly />
            </Section>
          ))}

          <Section title="처리 이력">
            <ol className="ui-timeline is-latest-first">
              {[...app.history].reverse().map((h, i) => <li key={i}><strong>{h.text}</strong><small>{when(h.at)}</small></li>)}
            </ol>
          </Section>
        </div>

        <aside className="adm-detail-side">
          <div className="adm-decide ui-dark">
            <h2>심사 결정</h2>
            <p>{statusLine}</p>
            {canReview && buttons}
            <div className="adm-decide-note">보완 요청에 서류를 고르면 아티스트의 ‘권리·보완 서류’에 제출 요청이 올라가고, 서류가 들어와야 보완 제출을 할 수 있어요.</div>
          </div>
          {!canReview && <NoDuty duty="발매 심사" />}
        </aside>
      </div>
      {canReview && (app.status === 'submitted' || actions.length > 0) && !action && (
        <div className={`adm-decide-bar${app.status === 'submitted' || actions.length === 1 ? ' is-claim' : ''}`} role="group" aria-label="심사 결정">{buttons}</div>
      )}
      {action && (
        <Modal title={ACTION_TITLE[action]} onClose={() => setAction(null)} dismissible={false}>
          <DecisionForm app={app} action={action} onDone={after} />
        </Modal>
      )}
    </div>
  );
}
