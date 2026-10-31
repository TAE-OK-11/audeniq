// 앨범 퍼블리싱 탭에서 곡을 누르면 열리는 곡 시트 — 발매 상세 '정보' 탭의 곡 시트(TrackSheet)와 같은 창.
// 참여자·지분·관리 상태를 보고 그 자리에서 고친다. 저장하면 창이 닫히고 앨범 목록에서 이어서 작업한다 (곡 상세로 건너가지 않는다).
// 지분을 고칠 때는 바뀌는 내용을 '서린 60% → 50%'로 먼저 보여 주고, 관리 중인 곡은 지금 적용값과 검토 중인 변경값을 나눠 보여 준다
import { useEffect, useRef, useState } from 'react';
import { Link } from '../../lib/router';
import { pubApi, type Work, type WorkContributor } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { MOCK } from '../../lib/mode';
import { splitReady } from '../../lib/publishing';
import { STAGE_LABEL, STAGE_TONE, changeNeedsReview, reusableWorks, splitDiff, workIssues, workStage, type PrepField, type SplitChange } from '../../lib/publishingPrep';
import { localStamp } from '../../lib/format';
import { Modal } from '../Modal';
import { Glyph } from '../Glyph';
import { useToast } from '../Toast';
import { Badge, Callout, Segmented } from '../../ui';
import { RegBadge, RegProgress } from './parts';
import { SplitEditor } from './SplitEditor';
import { AttachmentList, AttachmentModal } from './Attachments';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';


/** 바뀌는 내용 — 참여자마다 '창작 지분 60% → 50%' */
export function SplitDiffList({ changes, title }: { changes: SplitChange[]; title: string }) {
  if (!changes.length) return null;
  return (
    <section className="aq-pbl-diff" aria-label={title}>
      <h4>{title}</h4>
      <ul>
        {changes.map(c => (
          <li key={c.key} className={`is-${c.kind}`}>
            <strong>{c.name}</strong>
            {c.lines.map(l => <span key={l}>{l}</span>)}
          </li>
        ))}
      </ul>
    </section>
  );
}

export function WorkSheet({ work, no, all, focus, onClose, onSaved, releaseId }: {
  work: Work;
  /** 앨범 트랙 순서 — 제목 앞 번호 */
  no?: number;
  /** 참여자를 가져올 이전 곡 · 자료 첨부 창 */
  all: readonly Work[];
  /** 준비 점검에서 눌러 들어온 자리 — 그 칸을 열고 내려간다 */
  focus?: { field: PrepField; contributorId?: string } | null;
  onClose: () => void;
  onSaved: (w: Work) => void;
  releaseId?: string;
}) {
  const toast = useToast();
  const [w, setW] = useState(work);
  useEffect(() => setW(work), [work]);
  const canEdit = w.myRole === 'owner' || w.myRole === 'editor';
  const startEdit = canEdit && (focus?.field === 'split' || focus?.field === 'ipi');
  const [draft, setDraft] = useState<WorkContributor[] | null>(startEdit ? structuredClone(w.pendingChange?.contributors ?? w.contributors) : null);
  const [saving, setSaving] = useState(false);
  const [view, setView] = useState<'now' | 'pending'>('pending');
  const [attaching, setAttaching] = useState(false);
  const splitRef = useRef<HTMLElement>(null);
  const statusRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const el = focus?.field === 'fix' ? statusRef.current : focus ? splitRef.current : null;
    const target = focus?.contributorId ? document.getElementById(`pbl-person-${focus.contributorId}`) : null;
    if (el) window.requestAnimationFrame(() => (target ?? el).scrollIntoView({ behavior: 'smooth', block: 'start' }));
  }, [focus]);

  const stage = workStage(w);
  const blocking = workIssues(w).filter(i => i.blocking);
  const base = w.pendingChange?.contributors ?? w.contributors;
  const changes = draft ? splitDiff(base, draft) : [];
  const managed = changeNeedsReview(w);
  const pendingDiff = w.pendingChange ? splitDiff(w.contributors, w.pendingChange.contributors) : [];
  const shown = w.pendingChange && view === 'pending' ? w.pendingChange.contributors : w.contributors;
  const back = releaseId ? `?release=${encodeURIComponent(releaseId)}` : '';

  const save = async () => {
    if (!draft || saving) return;
    setSaving(true);
    try {
      const nw = await pubApi.updateWork(w.id, { contributors: draft }, w.rowVersion);
      setW(nw);
      setDraft(null);
      toast(nw.pendingChange ? '변경을 요청했어요.' : '지분을 저장했어요.', {
        tone: 'success',
        sub: nw.pendingChange ? '참여자 확인과 담당자 검토가 끝날 때까지 지금 값으로 관리돼요.' : splitReady(nw.contributors) ? '바뀐 지분은 참여자에게 확인을 요청해요.' : '합계가 100%가 아니에요. 신청 전에 맞춰 주세요.',
      });
      onSaved(nw);
      onClose();  // 저장하면 앨범 목록으로 돌아가 다음 곡을 이어서
    } catch (err) {
      toast(errorMessage(err, '저장하지 못했어요. 다시 시도해 주세요.'), 'error');
    } finally { setSaving(false); }
  };

  const remind = (c: WorkContributor) => void pubApi.requestSplitConfirm(w.id, c.id)
    .then(nw => { setW(nw); onSaved(nw); toast(`${c.name} 님에게 확인을 요청했어요.`, 'success'); })
    .catch(err => toast(errorMessage(err), 'error'));
  const simulate = (c: WorkContributor) => void pubApi.simulateConfirm(w.id, c.id).then(nw => { setW(nw); onSaved(nw); });

  return (
    <Modal title={`${no ? `${no}. ` : ''}${w.title || '제목 없는 곡'}`} onClose={onClose} dismissible={!draft}>
      <div className="aq-track-sheet aq-pbl-sheet">
        <div className="aq-track-sheet-tags">
          {/* 상태는 한 개만 — 보완 요청이 있으면 그것이 가장 먼저 할 일 */}
          {w.fix ? <Badge tone="amber">보완 요청</Badge> : <Badge tone={STAGE_TONE[stage]}>{STAGE_LABEL[stage]}</Badge>}
          {!w.fix && stage === 'managed' && <RegBadge status={w.registration} />}
        </div>

        {!draft && blocking.length > 0 && stage === 'blocked' && (
          <Callout tone="warn">{blocking[0].text}{blocking.length > 1 && ` 외 ${blocking.length - 1}건`}</Callout>
        )}

        <section ref={splitRef} className="aq-pbl-sheet-sec" aria-labelledby="wsSplit">
          <div className="ui-section-head">
            <h3 id="wsSplit" className="aq-track-sheet-h">참여자·지분</h3>
            {canEdit && !draft && (
              <button type="button" className="ui-btn is-soft is-sm" onClick={() => setDraft(structuredClone(base))}>
                <Glyph name="pencil" size={14} className="aq-glyph-lead" />{w.pendingChange ? '변경값 고치기' : '수정'}
              </button>
            )}
          </div>
          {draft ? (
            <>
              <SplitEditor value={draft} onChange={setDraft} reuse={reusableWorks(all, w.id)} focusId={focus?.contributorId} />
              {changes.length > 0
                ? <SplitDiffList title="바뀌는 내용" changes={changes} />
                : <p className="ui-field-help">아직 바뀐 내용이 없어요.</p>}
              {managed && changes.length > 0 && (
                <p className="ui-field-help">관리 중인 곡이라 지금 적용값은 그대로 두고, 참여자 확인과 담당자 검토가 끝나면 바뀐 값으로 관리해요.</p>
              )}
            </>
          ) : (
            <>
              {w.pendingChange && (
                <>
                  <Segmented label="볼 지분" value={view} onChange={setView} fill size="sm"
                    options={[{ value: 'now', label: '지금 적용값' }, { value: 'pending', label: '검토 중인 변경값' }]} />
                  {view === 'pending' && <SplitDiffList title={`${localStamp(w.pendingChange.requestedAt)} 요청 · 검토 중`} changes={pendingDiff} />}
                </>
              )}
              <SplitEditor value={shown} onChange={() => {}} readOnly
                onRequestConfirm={canEdit ? remind : undefined} onSimulate={MOCK && canEdit ? simulate : undefined} />
            </>
          )}
        </section>

        <section ref={statusRef} className="aq-pbl-sheet-sec" aria-labelledby="wsStatus">
          <h3 id="wsStatus" className="aq-track-sheet-h">관리 상태</h3>
          {w.fix && (
            <Callout tone="warn" action={canEdit && <Link to={`/works/${w.id}?tab=register${back && `&${back.slice(1)}`}`} className="ui-btn is-white is-sm">보완하기</Link>}>{w.fix.reason}</Callout>
          )}
          <RegProgress status={w.registration} log={w.registrationLog} />
        </section>

        <section className="aq-pbl-sheet-sec" aria-labelledby="wsFiles">
          <div className="ui-section-head">
            <h3 id="wsFiles" className="aq-track-sheet-h">첨부 자료</h3>
            {canEdit && !draft && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setAttaching(true)}><Glyph name="clip" size={14} className="aq-glyph-lead" />자료 첨부</button>}
          </div>
          <AttachmentList items={w.attachments} />
        </section>

        {draft ? (
          <div className="aq-sticky-foot ui-actions">
            <button type="button" className="ui-btn is-soft" disabled={saving} onClick={() => setDraft(null)}>취소</button>
            <button type="button" className={`ui-btn is-primary${saving ? ' is-busy' : ''}`} disabled={saving || !changes.length} onClick={() => void save()}>
              {saving ? '저장 중' : managed ? '변경 요청' : '저장'}
            </button>
          </div>
        ) : (
          <Link to={`/works/${w.id}${back}`} className="ui-btn is-soft is-block">곡 전체 정보 보기<Glyph name="chevron-right" size={14} /></Link>
        )}
      </div>
      {attaching && <AttachmentModal works={[w]} workId={w.id} onClose={() => setAttaching(false)} onAdded={nw => { setW(nw); onSaved(nw); }} />}
    </Modal>
  );
}
