// 발매 상세 '퍼블리싱' 탭 — 이 앨범 곡들의 작사·작곡 저작권을 '관리'하는 곳. 배급과 같은 발매 상세 안에서 끝낸다:
//  · 신청 준비 점검: '신청 가능 3곡 · 지분 확인 필요 1곡' — 항목을 누르면 그 곡 시트의 그 칸이 열린다 (발매 준비 항목과 같은 카드)
//  · 곡별 저작권: '정보' 탭 트랙 목록과 같은 줄. 누르면 곡 시트에서 참여자·지분·관리 상태를 보고 고친다 (저장하면 이 목록으로)
//  · 참여자 확인 현황: 확인 완료·대기·이의 제기 — 미확인 참여자와 관련 곡만 모아 본다
//  · 자료 첨부: 창에서 곡·종류·파일을 골라 올린다
// 번 돈은 한 장으로만 — 월별 추이·곡별·자세한 내역은 퍼블리싱 메뉴에서 이 앨범을 골라 본다
import { useEffect, useMemo, useState } from 'react';
import { Link } from '../../lib/router';
import type { ReleaseDetail } from '../../api/types';
import { pubApi, type Work } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { pct } from '../../lib/publishing';
import {
  CONFIRM_LABEL, CONFIRM_TONE, STAGE_LABEL, albumReadiness, confirmationBoard, workStage,
  type ConfirmState, type PrepField, type PrepGroup, type WorkStage,
} from '../../lib/publishingPrep';
import { Glyph, type GlyphName } from '../Glyph';
import { SkeletonBlock } from '../Skeleton';
import { useToast } from '../Toast';
import { peopleLine } from './parts';
import { royaltyTotals } from './Royalties';
import { Money } from '../Money';
import { albumPublishing } from './ReleasePublishing';
import { Modal } from '../Modal';
import { TrackSheet } from '../TrackSheet';
import { draftTrackOf } from '../../lib/albumPrep';
import { AttachmentModal } from './Attachments';
import { Badge, Callout, ChipGroup, StatGrid, StatTile } from '../../ui';

import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

const PREP_ICON: Record<PrepGroup['key'], GlyphName> = { tracks: 'music', fix: 'warning', split: 'scale', confirm: 'users', ipi: 'user' };
const STAGE_TONE: Record<WorkStage, 'green' | 'amber' | 'violet' | 'blue' | 'gray'> = { ready: 'green', blocked: 'amber', applied: 'violet', managed: 'blue', readonly: 'gray' };
type PeopleFilter = 'open' | ConfirmState;

interface Sheet { work: Work; no?: number; focus?: { field: PrepField; contributorId?: string } | null }

/** 신청 준비 점검 — 발매 상세 '발매 준비 항목'(AlbumPrep)과 같은 카드: 숫자 · 막대 · 할 일 줄 */
function ApplyPrep({ title, summary, groups, readyCount, total, onGo, applyTo }: {
  title: string; summary: string; groups: PrepGroup[]; readyCount: number; total: number;
  onGo: (g: PrepGroup) => void; applyTo: string | null;
}) {
  // 끝난 항목('보완 요청 없음', '모두 정리했어요')은 할 일이 아니다 — 남은 일만 보여 준다
  const open = groups.filter(g => g.state !== 'done');
  const apply = applyTo && readyCount > 0 && <Link to={applyTo} className="ui-btn is-primary is-block">{readyCount}곡 저작권 관리 신청</Link>;
  if (!open.length) {
    // 남은 일이 없으면 목록 대신 한 줄과 신청 버튼만
    return (
      <section className="aq-relinfo-card aq-prep" aria-labelledby="pubPrepTitle">
        <div className="aq-prep-head">
          <div className="min-0">
            <h2 className="subhead" id="pubPrepTitle">신청 준비가 끝났어요</h2>
            <p className="aq-prep-sub">{readyCount ? `${readyCount}곡 모두 참여자와 지분을 확인했어요` : summary}</p>
          </div>
        </div>
        {apply}
      </section>
    );
  }
  return (
    <section className="aq-relinfo-card aq-prep" aria-labelledby="pubPrepTitle">
      <div className="aq-prep-head">
        <div className="min-0">
          <h2 className="subhead" id="pubPrepTitle">{title}</h2>
          <p className="aq-prep-sub">{summary}</p>
        </div>
        {/* 진행 숫자는 여러 곡일 때만 — 1곡이면 '1/1'은 할 말이 없다 */}
        {total > 1 && <p className="aq-prep-count" aria-label={`${total}곡 중 ${readyCount}곡 신청 가능`}><b>{readyCount}</b>/{total}</p>}
      </div>
      {total > 1 && <div className="aq-prep-bar" aria-hidden="true"><i style={{ ['--p' as string]: `${Math.round((readyCount / total) * 100)}%` }} /></div>}
      <ul className="aq-task-list aq-prep-list">
        {open.map(g => {
          const body = (
            <>
              <span className={`ui-icon${g.state === 'wait' ? ' is-gray' : ''}`} aria-hidden="true">
                <Glyph name={g.state === 'wait' ? 'clock' : PREP_ICON[g.key]} size={18} />
              </span>
              <span className="aq-task-text"><strong>{g.label}</strong><small>{g.sub}</small></span>
            </>
          );
          const go = g.key === 'tracks' || g.works.length > 0;
          return (
            <li key={g.key} className={`is-${g.state}`}>
              {go ? (
                <button type="button" className="aq-task" onClick={() => onGo(g)}>
                  {body}
                  <span className="aq-task-go">{g.key === 'tracks' ? '정리' : g.state === 'wait' ? '보기' : '고치기'}<i aria-hidden="true"><Glyph name="chevron-right" size={14} /></i></span>
                </button>
              ) : <div className="aq-task is-static">{body}</div>}
            </li>
          );
        })}
      </ul>
      {apply}
    </section>
  );
}

/** 참여자 확인 현황 — 준비 점검의 '참여자 확인'을 누르면 뜨는 창. 사람마다 한 줄, 그 사람이 들어간 곡을 아래에.
 *  탭에 카드로 두면 준비 점검과 같은 이야기를 한 번 더 했다 */
function PeopleModal({ works, onOpen, onClose }: { works: Work[]; onOpen: (w: Work, contributorId: string) => void; onClose: () => void }) {
  const board = useMemo(() => confirmationBoard(works), [works]);
  const open = board.counts.pending + board.counts.disputed;
  const [filter, setFilter] = useState<PeopleFilter>('open');
  const shown = board.people.filter(p => (filter === 'open' ? p.state !== 'confirmed' : p.state === filter));
  return (
    <Modal title={`참여자 확인 현황 · ${board.people.length}명`} onClose={onClose}>
    <div className="aq-pbl-people-board">
      <ChipGroup size="sm" label="확인 상태" value={filter} onChange={setFilter} options={[
        { value: 'open', label: '미확인', count: open },
        { value: 'pending', label: CONFIRM_LABEL.pending, count: board.counts.pending, hidden: !board.counts.pending },
        { value: 'disputed', label: CONFIRM_LABEL.disputed, count: board.counts.disputed, hidden: !board.counts.disputed },
        { value: 'confirmed', label: CONFIRM_LABEL.confirmed, count: board.counts.confirmed, hidden: !board.counts.confirmed },
      ]} />
      {shown.length ? (
        <ul className="aq-pbl-people-list">
          {shown.map(p => (
            <li key={p.key}>
              <div className="aq-pbl-people-head">
                <strong>{p.name}{p.isMe && ' (나)'}</strong>
                <Badge tone={CONFIRM_TONE[p.state]}>{CONFIRM_LABEL[p.state]}</Badge>
              </div>
              <div className="ui-list is-inset">
                {p.entries.map(e => (
                  <button key={e.work.id + e.contributor.id} type="button" className="ui-row" onClick={() => onOpen(e.work, e.contributor.id)}>
                    <span className="min-0">
                      <span className="ui-row-title">{e.work.title}</span>
                      <span className="ui-row-meta"><span>창작 지분 {pct(e.contributor.share)}</span><span>{CONFIRM_LABEL[e.contributor.confirm]}</span></span>
                    </span>
                    <Glyph name="chevron-right" size={16} />
                  </button>
                ))}
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="aq-empty-val">{filter === 'open' ? '확인을 기다리는 참여자가 없어요.' : '해당하는 참여자가 없어요.'}</p>
      )}
    </div>
    </Modal>
  );
}

export function AlbumPublishingTab({ rel, focus = false }: { rel: ReleaseDetail; focus?: boolean }) {
  const toast = useToast();
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [busy, setBusy] = useState(false);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [peopleOpen, setPeopleOpen] = useState(false);
  // 예전 '퍼블리싱' 탭 주소(?tab=publishing)로 오면 이 칸으로 내려간다
  const ready = !!works;
  useEffect(() => {
    if (!focus || !ready) return;
    const id = window.setTimeout(() => document.getElementById('albumPub')?.scrollIntoView({ behavior: 'smooth', block: 'start' }), 200);
    return () => window.clearTimeout(id);
  }, [focus, ready]);
  if (!works) return <SkeletonBlock height={240} />;

  const a = albumPublishing(works, rel.id, rel.tracks.length);
  const byTrack = new Map<string, Work>(a.mine.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const trackNo = new Map<string, number>();
  rel.tracks.forEach((t, i) => { const w = byTrack.get(t.id); if (w && !trackNo.has(w.id)) trackNo.set(w.id, i + 1); });
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const prep = albumReadiness(a.mine, missing);
  const applyTo = (list: Work[]) => `/publishing/apply?release=${encodeURIComponent(rel.id)}&works=${list.map(w => encodeURIComponent(w.id)).join(',')}`;
  const fixWork = a.mine.find(w => w.registration === 'needs_fix' || w.application?.status === 'needs_fix');
  // 숫자는 트랙 기준으로 — 한 곡이 원곡·Inst.처럼 두 트랙에 실리면 '2곡 중 1곡'으로 어긋났다
  const trackCount = (list: Work[]) => rel.tracks.filter(t => { const w = byTrack.get(t.id); return !!w && list.includes(w); }).length;
  const managedTracks = trackCount(a.managed);
  const applyingTracks = trackCount(a.applying);
  const text = a.fix ? '심사 중에 보완할 곡이 생겼어요. 고쳐서 다시 내면 심사가 이어져요.'
    : managedTracks ? `${managedTracks === rel.tracks.length ? `${rel.tracks.length}곡 모두` : `${rel.tracks.length}곡 중 ${managedTracks}곡의`} 저작권을 AUDENIQ이 관리하고 있어요. 저작권료는 정산 잔액에 함께 쌓여요.`
    : applyingTracks ? `${applyingTracks}곡을 심사하고 있어요. 끝나면 알림으로 알려 드려요.`
    : prep.ready.length ? `${prep.ready.length}곡을 바로 신청할 수 있어요. 신청하면 담당자가 참여자와 지분을 확인해요.`
    : '이 앨범 곡의 작사·작곡 저작권도 맡기면 방송·스트리밍 저작권료까지 받아 드려요. (선택)';
  // 숫자는 둘 이상이 의미 있을 때만 — 0곡이 줄지은 칸은 아무것도 알려 주지 않는다
  const stats = ([['관리 중', managedTracks], ['심사 중', applyingTracks], ['정리 전', missing]] as const).filter(([, n]) => n > 0);
  const editable = a.mine.filter(w => w.myRole === 'owner' || w.myRole === 'editor');
  const sheetTrack = sheet ? rel.tracks[(sheet.no ?? 0) - 1] ?? rel.tracks.find(t => sheet.work.recordings.some(r => r.trackId === t.id)) : undefined;
  // 이 앨범에서 아직 아무 곡도 정리하지 않았으면 처음 시작하는 화면
  const fresh = a.mine.length === 0 && missing > 0;
  const openPrep = !fresh && (prep.groups.some(g => g.state !== 'done') || prep.ready.length > 0);

  const prepare = async () => {
    setBusy(true);
    try {
      await pubApi.prepareWorksFromRelease(rel.id, false); reload();
      toast(`${missing}곡을 크레딧으로 정리했어요. 지분이 맞는지 확인해 주세요.`, 'success');
    } catch (err) { toast(errorMessage(err, '정리하지 못했어요. 다시 시도해 주세요.'), 'error'); } finally { setBusy(false); }
  };
  const goPrep = (g: PrepGroup) => {
    if (g.key === 'tracks') { void prepare(); return; }
    if (g.key === 'confirm') { setPeopleOpen(true); return; }
    const w = g.works[0];
    const issue = w && g.key === 'ipi' ? w.contributors.find(c => !c.isMe && !c.ipi) : null;
    if (w) setSheet({ work: w, no: trackNo.get(w.id), focus: { field: g.key, contributorId: issue?.id } });
  };
  const saved = () => reload();

  return (
    <div className="aq-detail-stack aq-album-pub" id="albumPub">
      {/* '배급·계약' 탭의 다른 카드와 같은 모양 — 제목 · 상태 · 한 문장. 계약은 위 계약 묶음의 '저작권 관리 계약' 줄, 저작권료는 '성과' 탭에 */}
      <section className="aq-relinfo-card aq-pbl-head" aria-labelledby="albumPubHead">
        <div className="ui-section-head">
          <h2 id="albumPubHead" className="subhead">작사·작곡 저작권{fresh && <small> (선택)</small>}</h2>
          {a.status && <Badge tone={a.status.tone}>{a.status.label}</Badge>}
        </div>
        {fresh ? (
          <>
            {/* 아직 아무 곡도 정리하지 않았으면 입구 하나 — 버튼 하나와 안심 문구 */}
            <p className="ui-field-help">이 앨범 곡의 작사·작곡 저작권도 맡기면 방송·스트리밍 저작권료를 배급 정산금과 같은 잔액으로 받아요.</p>
            <button type="button" className={`ui-btn is-soft is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void prepare()}>
              {busy ? '정리하는 중' : `크레딧으로 ${missing}곡 정리하고 시작하기`}
            </button>
            <p className="ui-field-help aq-pbl-start-note">정리만 해 두면 아무것도 신청되지 않아요. 지분을 확인한 뒤 원할 때 신청하면 돼요.</p>
          </>
        ) : (
          <>
            <p className="ui-field-help">{text}</p>
            {stats.length > 1 && (
              <StatGrid cols={stats.length as 2 | 3} className="aq-pbl-album-stats">
                {stats.map(([label, n]) => <StatTile key={label} compact label={label} value={n} unit="곡" />)}
              </StatGrid>
            )}
          </>
        )}
      </section>

      {a.fix && (
        <Callout tone="warn" action={fixWork && <button type="button" className="ui-btn is-sm is-white" onClick={() => setSheet({ work: fixWork, no: trackNo.get(fixWork.id), focus: { field: 'fix' } })}>보완하기</button>}>
          {fixWork ? `‘${fixWork.title}’에 보완 요청이 왔어요.` : '보완 요청이 왔어요. 아래 곡을 눌러 고쳐 주세요.'}
        </Callout>
      )}

      {openPrep && (
        <ApplyPrep
          title="저작권 관리 신청 준비"
          summary={prep.summary} groups={prep.groups.filter(g => g.key !== 'fix')}
          readyCount={prep.ready.length} total={a.mine.filter(w => ['ready', 'blocked'].includes(workStage(w))).length}
          onGo={goPrep} applyTo={prep.ready.length ? applyTo(prep.ready) : null}
        />
      )}

      {!fresh && <div className="aq-relinfo-card aq-pbl-track-list">
        <div className="aq-tracklist-head">
          <h2 className="subhead">곡별 저작권</h2>
          <span className="aq-pbl-track-tools">
            {editable.length > 0 && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setAttaching(true)}><Glyph name="clip" size={14} className="aq-glyph-lead" />자료 첨부</button>}
          </span>
        </div>
        <ol className="aq-tracklist">
          {rel.tracks.map((t, i) => {
            const w = byTrack.get(t.id);
            if (!w) {
              return (
                <li key={t.id}>
                  <Link to={`/works/new?release=${encodeURIComponent(rel.id)}&track=${encodeURIComponent(t.id)}`} className="aq-trackrow aq-pbl-trackrow">
                    <span className="aq-trackrow-no" aria-hidden="true">{i + 1}</span>
                    <span className="aq-trackrow-main">
                      <span className="aq-trackrow-title">{t.title || '곡명 없음'}</span>
                      <span className="aq-trackrow-meta">작사·작곡 정보 정리 전</span>
                    </span>
                    <span className="aq-trackrow-side"><span className="aq-trackrow-isrc">정리하기</span><Glyph name="chevron-right" size={16} className="aq-track-chev" /></span>
                  </Link>
                </li>
              );
            }
            const stage = workStage(w);
            return (
              <li key={t.id}>
                  <button type="button" className="aq-trackrow aq-pbl-trackrow" onClick={() => setSheet({ work: w, no: i + 1 })} aria-label={`${i + 1}번 곡 ${t.title} 저작권 보기`}>
                    <span className="aq-trackrow-no" aria-hidden="true">{i + 1}</span>
                    <span className="aq-trackrow-main">
                      <span className="aq-trackrow-title">{t.title || w.title}</span>
                      <span className="aq-trackrow-meta">
                        {w.pendingChange && <b className="aq-trackrow-warn">변경 검토 중</b>}
                        {peopleLine(w.contributors)}{w.attachments.length > 0 && ` · 자료 ${w.attachments.length}`}
                      </span>
                    </span>
                    <span className="aq-trackrow-side">
                      <Badge tone={STAGE_TONE[stage]}>{STAGE_LABEL[stage]}</Badge>
                      <Glyph name="chevron-right" size={16} className="aq-track-chev" />
                    </span>
                  </button>
              </li>
            );
          })}
        </ol>
        {missing > 0 && !fresh && (
          <div className="row-actions">
            <button type="button" className={`ui-btn is-soft is-sm${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void prepare()}>
              {busy ? '정리하는 중' : `크레딧으로 ${missing}곡 한 번에 정리`}
            </button>
          </div>
        )}
      </div>}

      {sheet && sheetTrack && (
        // 정보 탭과 같은 곡 시트 하나 — 크레딧 아래 저작권 칸으로 바로 내려간다
        <TrackSheet track={sheetTrack} no={rel.tracks.indexOf(sheetTrack) + 1} draft={draftTrackOf(sheetTrack, rel.draft)} releaseId={rel.id}
          rightsFocus={sheet.focus} toRights onClose={() => setSheet(null)} onRightsSaved={saved} />
      )}
      {attaching && <AttachmentModal works={editable} onClose={() => setAttaching(false)} onAdded={saved} />}
      {peopleOpen && (
        <PeopleModal works={a.mine} onClose={() => setPeopleOpen(false)}
          onOpen={(w, cid) => { setPeopleOpen(false); setSheet({ work: w, no: trackNo.get(w.id), focus: { field: 'confirm', contributorId: cid } }); }} />
      )}
    </div>
  );
}

/** '성과' 탭의 작사·작곡 저작권료 — 배급 수익 아래 한 장. 받은 저작권료가 있을 때만 */
export function AlbumRoyaltyCard({ releaseId }: { releaseId: string }) {
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const ids = new Set((works ?? []).filter(w => w.recordings.some(r => r.releaseId === releaseId)).map(w => w.id));
  const rows = (royalties ?? []).filter(r => ids.has(r.workId));
  if (!rows.length) return null;
  const t = royaltyTotals(rows);
  return (
    <section className="aq-relinfo-card" aria-labelledby="albumRoyalty">
      <div className="ui-section-head">
        <h2 id="albumRoyalty" className="subhead">작사·작곡 저작권료</h2>
        <Link to={`/reports?type=publishing&release=${encodeURIComponent(releaseId)}`} className="aq-head-link">리포트에서 보기<Glyph name="chevron-right" size={14} /></Link>
      </div>
      <dl className="ui-kv is-2">
        <div><dt>누적 확정</dt><dd><b className="aq-pub-amount"><Money value={t.confirmed + t.paid} /></b></dd></div>
        <div><dt>확정 전 예상</dt><dd>{t.estimated ? <Money value={t.estimated} /> : '—'}</dd></div>
      </dl>
      <p className="ui-field-help">배급 수익과 같은 정산 잔액에 쌓여요.</p>
    </section>
  );
}
