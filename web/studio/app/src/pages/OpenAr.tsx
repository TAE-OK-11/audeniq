// Open A&R — 내 곡의 데모를 AUDENIQ A&R에 보내 피칭·제안 기회를 받는다 (기획안 9.1, 9.2).
// 곡에 이미 있는 장르·설명·데모·가사는 다시 적지 않고 가져온다. 같은 제출은 멱등 키로 한 번만 접수된다
import { useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { pubApi, type ArSubmission, type Work } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { uid } from '../lib/store';
import { AR_LABEL, AR_TONE } from '../lib/publishing';
import { localStamp } from '../lib/format';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { SkeletonRows } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { Badge, Callout, EmptyState, Field, IconTile, KeyValue, Segmented, inputClass, selectClass } from '../ui';
import { ReviewCard, WizardShell } from '../components/WizardShell';
import '../styles/split/Dashboard.css';
import '../styles/split/Publishing.css';
import { AttachmentModal } from '../components/publishing/Attachments';
import { StageCard } from '../components/StageCard';

const MOODS = ['신나는', '잔잔한', '신비로운', '슬픈', '설레는', '강렬한', '따뜻한', '어두운', '청량한'];
const AR_STEPS = ['제출', '심사', '심사 완료', '제안'];
const AR_PATH = [
  ['데모 제출', '곡에 올린 데모·가사를 그대로 보내요'],
  ['A&R 심사', '담당자가 듣고 분류해요. 보완이 필요하면 알려 드려요'],
  ['비공개 카탈로그', '통과한 곡만 지정된 관계자에게 공개돼요'],
  ['미팅·계약 제안', '관심 있는 곳이 있으면 제안을 전해 드려요'],
] as const;
const arStep = (s: ArSubmission['status']) => ({ submitted: 0, in_review: 1, needs_fix: 1, reviewed: 2, proposing: 3, closed: 4 }[s]);

function SubmitForm({ works, preset, onDone, onClose, onWorkChanged }: { works: Work[]; preset: string | null; onDone: (s: ArSubmission) => void; onClose: () => void; onWorkChanged: () => void }) {
  const [attaching, setAttaching] = useState(false);
  const [step, setStep] = useState(0);
  const [reached, setReached] = useState(0);
  const go = (next: number) => { setStep(next); setReached(r => Math.max(r, next)); window.scrollTo({ top: 0 }); };
  const toast = useToast();
  const [workId, setWorkId] = useState(preset && works.some(w => w.id === preset) ? preset : '');
  const work = works.find(w => w.id === workId) ?? null;
  const [genre, setGenre] = useState('');
  const [mood, setMood] = useState<string[]>([]);
  const [desc, setDesc] = useState('');
  const [refs, setRefs] = useState('');
  const [scope, setScope] = useState<'internal' | 'partners'>('partners');
  const [consent, setConsent] = useState(false);
  useEffect(() => setConsent(false), [workId, scope]);
  const [busy, setBusy] = useState(false);
  const [key, setKey] = useState(() => uid('ar'));
  // 곡을 고르면 곡에 있던 정보로 채운다
  useEffect(() => { if (work) { setGenre(work.genre); setDesc(work.description); } }, [work?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const demo = work?.attachments.find(a => a.kind === 'demo');
  const lyrics = work?.attachments.find(a => a.kind === 'lyrics');
  const ready = !!work && !!demo && !!genre.trim() && mood.length > 0 && consent;

  const submit = async () => {
    if (!ready || !work || busy) return;
    setBusy(true);
    try {
      const s = await pubApi.submitAr({ workId: work.id, genre: genre.trim(), mood, description: desc.trim(), references: refs.trim(), scope, consent, demoName: demo?.name, lyrics: lyrics?.name, idempotencyKey: key });
      toast('A&R에 제출했어요. 심사가 시작되면 알려 드려요.', 'success');
      setKey(uid('ar'));
      setWorkId(''); setMood([]); setConsent(false); setRefs('');
      onDone(s);
    } catch (e) { toast(errorMessage(e, '제출하지 못했어요. 같은 제출은 한 번만 접수되니 다시 시도해 주세요.'), 'error'); }
    finally { setBusy(false); }
  };

  return (
    <WizardShell className="aq-pbl-wizard" label="데모 제출" title="데모 제출"
      steps={[
        { short: '곡·음원', title: '어떤 곡을 보낼까요?', sub: '등록한 곡과 데모 음원을 확인해 주세요.' },
        { short: '곡 소개', title: '곡을 소개해 주세요', sub: '장르와 분위기를 알려 주시면 피칭에 도움이 돼요.' },
        { short: '최종 확인', title: '제출 내용을 확인해 주세요', sub: '공개 범위를 확인하고 A&R에 보내 주세요.' },
      ]} step={step} reached={reached} onStep={i => { if (!busy) go(i); }} onBack={() => { if (!busy) step ? go(step - 1) : onClose(); }}
      summary={{ title: work?.title || '곡을 골라 주세요', lines: ['Open A&R', demo?.name || '데모 음원 확인 전'] }}
      actions={<>
        <button type="button" id="wizardBack" className="ui-btn is-soft" disabled={busy} onClick={() => step ? go(step - 1) : onClose()}>{step ? '이전' : '나가기'}</button>
        <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`}
          disabled={busy || (step === 0 ? !work || !demo : step === 1 ? !genre.trim() || !mood.length : !ready)}
          onClick={() => step < 2 ? go(step + 1) : void submit()}>{busy ? '제출하는 중' : step < 2 ? '다음으로' : 'A&R에 제출'}</button>
      </>}
    >
      {step === 0 && <>
      <Field label="곡" htmlFor="arWork" required help={!works.length ? undefined : '데모 음원이 있는 곡을 골라 주세요.'}>
        <select id="arWork" className={selectClass()} value={workId} onChange={e => setWorkId(e.target.value)}>
          <option value="">곡 고르기</option>
          {works.map(w => <option key={w.id} value={w.id}>{w.title}{w.attachments.some(a => a.kind === 'demo') ? '' : ' (데모 없음)'}</option>)}
        </select>
      </Field>
      {work && <KeyValue cols={1} items={[
        ['데모 음원', demo?.name || '등록된 데모가 없어요'],
        ['가사', lyrics?.name || '등록된 가사가 없어요 (선택)'],
      ]} />}
      {work && !demo && <Callout tone="warn" action={<button type="button" className="ui-btn is-white is-sm" onClick={() => setAttaching(true)}>음원 올리기</button>}>곡에 데모 음원을 먼저 올려 주세요.</Callout>}
      {!works.length && <EmptyState compact title="제출할 곡이 없어요" actions={<Link to="/works/new" className="ui-btn is-primary">새 곡 등록</Link>}>곡을 등록하고 데모 음원을 올려 주세요.</EmptyState>}
      </>}
      {step === 1 && <>
            <div className="form-grid">
              <Field label="장르" htmlFor="arGenre" required><input id="arGenre" className={inputClass()} value={genre} onChange={e => setGenre(e.target.value)} /></Field>
              <Field label="공개 범위">
                <Segmented fill label="공개 범위" value={scope} onChange={setScope} options={[{ value: 'internal', label: 'AUDENIQ만' }, { value: 'partners', label: '승인 파트너까지' }]} />
              </Field>
            </div>
            <Field label="분위기" required help="잘 어울리는 것을 골라 주세요 (여러 개).">
              <div className="ui-chips is-wrap" role="group" aria-label="분위기">
                {MOODS.map(m => <button key={m} type="button" className="ui-chip is-sm" aria-pressed={mood.includes(m)} onClick={() => setMood(x => (x.includes(m) ? x.filter(y => y !== m) : [...x, m]))}>{m}</button>)}
              </div>
            </Field>
            <Field label="곡 설명" htmlFor="arDesc" help="어떤 아티스트·프로젝트에 어울리는지 적으면 피칭에 도움이 돼요.">
              <textarea id="arDesc" className="ui-textarea" rows={3} value={desc} onChange={e => setDesc(e.target.value)} />
            </Field>
            <Field label="참고 자료" htmlFor="arRefs" help="레퍼런스 곡 링크 등 (선택)">
              <input id="arRefs" className={inputClass()} value={refs} onChange={e => setRefs(e.target.value)} placeholder="https://" />
            </Field>
      </>}
      {step === 2 && <>
        <ReviewCard head="곡·음원" onEdit={() => go(0)}>{work?.title}<br />{demo?.name}</ReviewCard>
        <ReviewCard head="곡 소개" onEdit={() => go(1)}>{genre} · {mood.join(' · ')}{desc && <p>{desc}</p>}{refs && <p>{refs}</p>}</ReviewCard>
        <ReviewCard head="공개 범위" onEdit={() => go(1)}>{scope === 'partners' ? '승인 파트너까지' : 'AUDENIQ 내부만'}</ReviewCard>
            <label className="ui-check">
              <input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} />
              <span><b>(필수) 피칭에 동의해요.</b> {scope === 'partners' ? '승인된 곡만 지정된 외부 관계자(레이블·기획사·A&R)에게 비공개로 공개돼요.' : 'AUDENIQ 내부 A&R만 들어요.'} 언제든 제출을 취소할 수 있어요.</span>
            </label>
      </>}
      {attaching && work && <AttachmentModal works={works} workId={work.id} onClose={() => setAttaching(false)} onAdded={onWorkChanged} />}
    </WizardShell>
  );
}

export function OpenAr() {
  const [params, setParams] = useSearchParams();
  const preset = params.get('work');
  const toast = useToast();
  const confirm = useConfirm();
  const { data: list, loading, reload } = useAsync(() => pubApi.listAr(), [], { key: 'ar-submissions' });
  const { data: works, reload: reloadWorks } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const submitting = params.get('submit') === '1' || !!preset;
  const openSubmit = () => setParams(new URLSearchParams({ submit: '1' }));
  const closeSubmit = () => setParams(new URLSearchParams(), { replace: true });
  const mine = useMemo(() => (works ?? []).filter(w => w.myRole === 'owner' || w.myRole === 'editor'), [works]);

  const withdraw = async (s: ArSubmission) => {
    if (!(await confirm({ title: '제출을 취소할까요?', message: '심사가 멈추고 카탈로그에서도 내려가요. 곡 정보는 그대로예요.', confirmLabel: '제출 취소', danger: true }))) return;
    try { await pubApi.withdrawAr(s.id); toast('제출을 취소했어요.', 'success'); reload(); } catch (e) { toast(errorMessage(e), 'error'); }
  };

  if (submitting) return works
    ? <SubmitForm works={mine} preset={preset} onWorkChanged={reloadWorks} onClose={closeSubmit} onDone={() => { reload(); closeSubmit(); }} />
    : <div className="view"><SkeletonRows count={3} /></div>;

  return (
    <div className="view" id="view-ar">
      <div className="ui-page-head aq-head-inline">
        <div>
          <p className="eyebrow">홍보</p>
          <h1>Open A&R</h1>
          <p>곡 데모를 AUDENIQ A&R에 보내면 심사 후 레이블·아티스트에게 피칭해 드려요.</p>
        </div>
        <button type="button" className="ui-btn is-primary" onClick={openSubmit}><Glyph name="plus" size={14} />데모 제출</button>
      </div>
      <div className="ui-section-head"><h2>내 제출</h2><span className="ui-row-meta">{list ? `${list.length}건` : ''}</span></div>
        <div className="ui-tab-panel" key="list">
          {loading && !list ? <SkeletonRows count={2} /> : list?.length ? (
            <div className="aq-stagger">
              {list.map(s => (
                <section key={s.id} className="ui-card aq-ar-card" aria-label={`${s.workTitle} A&R 제출`}>
                  <div className="ui-section-head">
                    <h2>{s.workTitle}</h2>
                    <Badge tone={AR_TONE[s.status]}>{AR_LABEL[s.status]}</Badge>
                  </div>
                  <Link to={`/works/${s.workId}`} className="aq-upcoming-link aq-row-link">
                    <IconTile><Glyph name="music" size={20} /></IconTile>
                    <div className="min-0">
                      <span className="ui-row-title">{s.genre} · {s.mood.join(' · ')}</span>
                      <span className="ui-row-meta">{localStamp(s.submittedAt)} 제출 · {s.scope === 'partners' ? '승인 파트너까지 공개' : 'AUDENIQ 내부만'}</span>
                    </div>
                    <Glyph name="chevron-right" size={16} />
                  </Link>
                  {s.status !== 'closed' && <StageCard steps={AR_STEPS} current={arStep(s.status)} tone={s.status === 'needs_fix' ? 'warn' : undefined} label="A&R 진행 단계" />}
                  {s.fixReason && <Callout tone="warn">{s.fixReason}</Callout>}
                  {['submitted', 'in_review', 'needs_fix'].includes(s.status) && (
                    <div className="row-actions"><button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => void withdraw(s)}>제출 취소</button></div>
                  )}
                </section>
              ))}
            </div>
          ) : (
            <EmptyState icon={<Glyph name="megaphone" size={22} />} title="아직 제출한 곡이 없어요" actions={<button type="button" className="ui-btn is-primary" onClick={openSubmit}>첫 데모 제출</button>}>
              발매 전 데모도 돼요. 심사를 통과한 곡은 비공개 카탈로그에 올라가 관계자에게 소개돼요.
            </EmptyState>
          )}
        </div>

      {/* 진행 순서 — 계약서 화면의 '계약은 이렇게 진행돼요'와 같은 목록 */}
      <details className="aq-doc-path-wrap aq-pbl-guide" open={!list?.length}>
        <summary>Open A&R 진행 안내<Glyph name="chevron-down" size={16} /></summary>
        <ol className="ui-card ui-list is-inset aq-doc-path">
          {AR_PATH.map(([title, when], i) => (
            <li key={title} className="ui-row">
              <IconTile size="sm"><span className="aq-preview-no">{i + 1}</span></IconTile>
              <span className="ui-row-body min-0"><b className="ui-row-title">{title}</b><span className="ui-row-meta">{when}</span></span>
            </li>
          ))}
        </ol>
      </details>
    </div>
  );
}
