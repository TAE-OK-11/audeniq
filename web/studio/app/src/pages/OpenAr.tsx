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
import { Badge, Callout, EmptyState, Field, IconTile, Segmented, Tabs, inputClass, selectClass } from '../ui';
import { Reveal } from '../ui/Reveal';
import '../styles/split/Dashboard.css';
import '../styles/split/Publishing.css';
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

function SubmitForm({ works, preset, onDone }: { works: Work[]; preset: string | null; onDone: (s: ArSubmission) => void }) {
  const toast = useToast();
  const [workId, setWorkId] = useState(preset && works.some(w => w.id === preset) ? preset : '');
  const work = works.find(w => w.id === workId) ?? null;
  const [genre, setGenre] = useState('');
  const [mood, setMood] = useState<string[]>([]);
  const [desc, setDesc] = useState('');
  const [refs, setRefs] = useState('');
  const [scope, setScope] = useState<'internal' | 'partners'>('partners');
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [key, setKey] = useState(() => uid('ar'));
  // 곡을 고르면 곡에 있던 정보로 채운다
  useEffect(() => { if (work) { setGenre(work.genre); setDesc(work.description); } }, [work?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const demo = work?.attachments.find(a => a.kind === 'demo');
  const lyrics = work?.attachments.find(a => a.kind === 'lyrics');
  const ready = !!work && !!demo && !!genre.trim() && mood.length > 0 && consent;

  const submit = async () => {
    if (!work || busy) return;
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
    <section className="ui-card aq-pbl-ar-form" aria-labelledby="arForm">
      <h2 id="arForm">데모 제출</h2>
      <Field label="곡" htmlFor="arWork" required help={!works.length ? undefined : '데모 음원이 있는 곡을 골라 주세요.'}>
        <select id="arWork" className={selectClass()} value={workId} onChange={e => setWorkId(e.target.value)}>
          <option value="">곡 고르기</option>
          {works.map(w => <option key={w.id} value={w.id}>{w.title}{w.attachments.some(a => a.kind === 'demo') ? '' : ' (데모 없음)'}</option>)}
        </select>
      </Field>
      <Reveal open={!!work}>
        {work && (
          <div>
            <dl className="ui-kv is-2">
              <div><dt>데모 음원</dt><dd className="break">{demo ? demo.name : <span className="aq-empty-val">없음 — <Link to={`/works/${work.id}?tab=files`} className="aq-inline-link">곡에서 올리기</Link></span>}</dd></div>
              <div><dt>가사</dt><dd className="break">{lyrics ? lyrics.name : <span className="aq-empty-val">없음 (선택)</span>}</dd></div>
            </dl>
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
            <label className="ui-check">
              <input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} />
              <span><b>(필수) 피칭에 동의해요.</b> {scope === 'partners' ? '승인된 곡만 지정된 외부 관계자(레이블·기획사·A&R)에게 비공개로 공개돼요.' : 'AUDENIQ 내부 A&R만 들어요.'} 언제든 제출을 취소할 수 있어요.</span>
            </label>
            <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={!ready || busy} onClick={() => void submit()}>{busy ? '제출하는 중' : 'A&R에 제출'}</button>
          </div>
        )}
      </Reveal>
    </section>
  );
}

export function OpenAr() {
  const [params] = useSearchParams();
  const preset = params.get('work');
  const toast = useToast();
  const confirm = useConfirm();
  const { data: list, loading, reload } = useAsync(() => pubApi.listAr(), [], { key: 'ar-submissions' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [view, setView] = useState<'submit' | 'list'>(preset ? 'submit' : 'list');
  const mine = useMemo(() => (works ?? []).filter(w => w.myRole === 'owner' || w.myRole === 'editor'), [works]);
  const active = (list ?? []).filter(s => s.status !== 'closed');

  const withdraw = async (s: ArSubmission) => {
    if (!(await confirm({ title: '제출을 취소할까요?', message: '심사가 멈추고 카탈로그에서도 내려가요. 곡 정보는 그대로예요.', confirmLabel: '제출 취소', danger: true }))) return;
    try { await pubApi.withdrawAr(s.id); toast('제출을 취소했어요.', 'success'); reload(); } catch (e) { toast(errorMessage(e), 'error'); }
  };

  return (
    <div className="view" id="view-ar">
      <div className="ui-page-head aq-head-inline">
        <div>
          <p className="eyebrow">홍보</p>
          <h1>Open A&R</h1>
          <p>곡 데모를 AUDENIQ A&R에 보내면 심사 후 레이블·아티스트에게 피칭해 드려요.</p>
        </div>
        {view === 'list' && <button type="button" className="ui-btn is-primary" onClick={() => setView('submit')}><Glyph name="plus" size={14} />데모 제출</button>}
      </div>
      <Tabs label="Open A&R" value={view} onChange={setView}
        items={[{ value: 'list', label: '내 제출', count: active.length }, { value: 'submit', label: '새로 제출' }]} />

      {view === 'submit' ? (
        <div className="ui-tab-panel" key="submit">
          {works && !mine.length ? (
            <EmptyState title="제출할 곡이 없어요" actions={<Link to="/works/new" className="ui-btn is-primary">새 곡 등록</Link>}>먼저 곡을 등록하고 데모 음원을 올려 주세요.</EmptyState>
          ) : works ? (
            <SubmitForm works={mine} preset={preset} onDone={() => { reload(); setView('list'); }} />
          ) : <SkeletonRows count={2} />}
        </div>
      ) : (
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
            <EmptyState icon={<Glyph name="megaphone" size={22} />} title="아직 제출한 곡이 없어요" actions={<button type="button" className="ui-btn is-primary" onClick={() => setView('submit')}>첫 데모 제출</button>}>
              발매 전 데모도 돼요. 심사를 통과한 곡은 비공개 카탈로그에 올라가 관계자에게 소개돼요.
            </EmptyState>
          )}
        </div>
      )}

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
