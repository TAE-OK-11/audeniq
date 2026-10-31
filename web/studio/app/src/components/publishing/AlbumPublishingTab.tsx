// 발매 상세 '퍼블리싱' 탭 — 이 앨범 곡들의 작사·작곡 저작권을 '관리'하는 곳. 배급과 같은 발매 상세 안에서 끝낸다:
//  · 신청 준비 점검: '신청 가능 3곡 · 지분 확인 필요 1곡' — 항목을 누르면 그 곡 시트의 그 칸이 열린다 (발매 준비 항목과 같은 카드)
//  · 곡별 저작권: '정보' 탭 트랙 목록과 같은 줄. 누르면 곡 시트에서 참여자·지분·관리 상태를 보고 고친다 (저장하면 이 목록으로)
//  · 여러 곡 선택: 고른 곡으로 준비 상태 확인 → 신청 대상으로 묶기 → 저작권료 내역 받기를 같은 선택으로 이어서
//  · 참여자 확인 현황: 확인 완료·대기·이의 제기 — 미확인 참여자와 관련 곡만 모아 본다
//  · 자료 첨부: 창에서 곡·종류·파일을 골라 올린다
// 번 돈은 한 장으로만 — 월별 추이·곡별·자세한 내역은 퍼블리싱 메뉴에서 이 앨범을 골라 본다
import { useMemo, useState } from 'react';
import { Link, useNavigate } from '../../lib/router';
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
import { ManageHead } from '../ManageHead';
import { useToast } from '../Toast';
import { peopleLine } from './parts';
import { royaltyTotals, saveRoyaltyCsv } from './Royalties';
import { Money } from '../Money';
import { PublishingBundleStep, albumPublishing } from './ReleasePublishing';
import { WorkSheet } from './WorkSheet';
import { AttachmentModal } from './Attachments';
import { Badge, Callout, ChipGroup, KeyValue, StatGrid, StatTile } from '../../ui';

import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

const PREP_ICON: Record<PrepGroup['key'], GlyphName> = { tracks: 'music', fix: 'warning', split: 'scale', confirm: 'users', ipi: 'user' };
const STAGE_TONE: Record<WorkStage, 'green' | 'amber' | 'violet' | 'blue' | 'gray'> = { ready: 'green', blocked: 'amber', applied: 'violet', managed: 'blue', readonly: 'gray' };
type PeopleFilter = 'open' | ConfirmState;
const START_STEPS = [
  ['곡 정리', '발매할 때 적은 크레딧으로 작사·작곡가를 채워 둬요'],
  ['지분 확인', '참여자마다 몫(%)을 맞추고 함께 확인해요'],
  ['신청·서명', '맡길 곡을 골라 신청하면 담당자가 등록까지 맡아요'],
] as const;

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

/** 참여자 확인 현황 — 사람마다 한 줄, 그 사람이 들어간 곡을 아래에. 곡을 누르면 그 참여자 줄이 있는 시트 */
function Confirmations({ works, onOpen }: { works: Work[]; onOpen: (w: Work, contributorId: string) => void }) {
  const board = useMemo(() => confirmationBoard(works), [works]);
  const open = board.counts.pending + board.counts.disputed;
  const [filter, setFilter] = useState<PeopleFilter>('open');
  // 모두 확인했으면 카드를 두지 않는다 — '확인 완료' 목록은 할 일이 아니다 (참여자는 곡 시트에서 볼 수 있다)
  if (!open) return null;
  const shown = board.people.filter(p => (filter === 'open' ? p.state !== 'confirmed' : p.state === filter));
  return (
    <section className="aq-relinfo-card aq-pbl-people-board" aria-labelledby="pubPeople">
      <div className="ui-section-head">
        <h2 id="pubPeople" className="subhead">참여자 확인 현황</h2>
        <span className="ui-count is-soft">{board.people.length}명</span>
      </div>
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
    </section>
  );
}

export function AlbumPublishingTab({ rel }: { rel: ReleaseDetail }) {
  const toast = useToast();
  const nav = useNavigate();
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const [busy, setBusy] = useState(false);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [attaching, setAttaching] = useState(false);
  // 고른 곡 — 시트를 열고 닫아도, 저장해도 그대로 남는다
  const [picked, setPicked] = useState<Set<string>>(() => new Set());
  if (!works) return <SkeletonBlock height={240} />;

  const a = albumPublishing(works, rel.id, rel.tracks.length);
  const byTrack = new Map<string, Work>(a.mine.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const trackNo = new Map<string, number>();
  rel.tracks.forEach((t, i) => { const w = byTrack.get(t.id); if (w && !trackNo.has(w.id)) trackNo.set(w.id, i + 1); });
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const selected = a.mine.filter(w => picked.has(w.id));
  const scope = selected.length ? selected : a.mine;
  const prep = albumReadiness(scope, selected.length ? 0 : missing);
  const applyTo = (list: Work[]) => `/publishing/apply?release=${encodeURIComponent(rel.id)}&works=${list.map(w => encodeURIComponent(w.id)).join(',')}`;
  const workIds = new Set(a.mine.map(w => w.id));
  const albumRoyalties = (royalties ?? []).filter(r => workIds.has(r.workId));
  const totals = royaltyTotals(albumRoyalties);
  const fixWork = a.mine.find(w => w.registration === 'needs_fix' || w.application?.status === 'needs_fix');
  // 숫자는 트랙 기준으로 — 한 곡이 원곡·Inst.처럼 두 트랙에 실리면 '2곡 중 1곡'으로 어긋났다
  const trackCount = (list: Work[]) => rel.tracks.filter(t => { const w = byTrack.get(t.id); return !!w && list.includes(w); }).length;
  const managedTracks = trackCount(a.managed);
  const applyingTracks = trackCount(a.applying);
  const text = a.fix ? '심사 중에 보완할 곡이 생겼어요. 고쳐서 다시 내면 심사가 이어져요.'
    : managedTracks ? `${managedTracks === rel.tracks.length ? `${rel.tracks.length}곡 모두` : `${rel.tracks.length}곡 중 ${managedTracks}곡의`} 저작권을 AUDENIQ이 관리하고 있어요. 저작권료는 정산 잔액에 함께 쌓여요.`
    : applyingTracks ? `${applyingTracks}곡을 심사하고 있어요. 끝나면 알림으로 알려 드려요.`
    : prep.ready.length && !selected.length ? `${prep.ready.length}곡을 바로 신청할 수 있어요. 신청하면 담당자가 참여자와 지분을 확인해요.`
    : '이 앨범 곡의 작사·작곡 저작권도 맡기면 방송·스트리밍 저작권료까지 받아 드려요. (선택)';
  // 숫자는 둘 이상이 의미 있을 때만 — 0곡이 줄지은 칸은 아무것도 알려 주지 않는다
  const stats = ([['관리 중', managedTracks], ['심사 중', applyingTracks], ['정리 전', missing]] as const).filter(([, n]) => n > 0);
  const editable = a.mine.filter(w => w.myRole === 'owner' || w.myRole === 'editor');
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
    const w = g.works[0];
    const issue = w && (g.key === 'confirm' ? w.contributors.find(c => c.confirm !== 'confirmed') : g.key === 'ipi' ? w.contributors.find(c => !c.isMe && !c.ipi) : null);
    if (w) setSheet({ work: w, no: trackNo.get(w.id), focus: { field: g.key, contributorId: issue?.id } });
  };
  const toggle = (id: string) => setPicked(p => { const n = new Set(p); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allPicked = a.mine.length > 0 && a.mine.every(w => picked.has(w.id));
  const saved = () => reload();
  const bundle = () => {
    const ready = selected.filter(w => workStage(w) === 'ready');
    if (!ready.length) { toast('고른 곡 중 바로 신청할 수 있는 곡이 없어요.', { tone: 'error', sub: '준비 점검에서 남은 일을 먼저 끝내 주세요.' }); return; }
    if (ready.length < selected.length) toast(`${selected.length}곡 중 신청 가능한 ${ready.length}곡만 묶었어요.`, { sub: '나머지는 준비가 끝나면 다시 고를 수 있어요.' });
    nav(applyTo(ready));
  };
  const download = () => {
    const ids = new Set(selected.map(w => w.id));
    const rows = albumRoyalties.filter(r => ids.has(r.workId));
    if (!rows.length) { toast('고른 곡에 들어온 저작권료가 아직 없어요.'); return; }
    saveRoyaltyCsv(rows, `${rel.title} ${selected.length}곡`);
    toast(`${selected.length}곡 저작권료 ${rows.length}건을 CSV로 내려받았어요.`, 'success');
  };

  return (
    <div className="aq-detail-stack aq-album-pub">
      <section className="ui-card aq-manage-card" aria-labelledby="albumPubHead">
        <ManageHead id="albumPubHead" icon="scale" title="작사·작곡 저작권 관리" status={a.status} />
        {fresh ? (
          <>
            {/* 처음 여는 사람에게는 숫자 0 세 개 대신 무엇을 하면 되는지 — 세 단계와 버튼 하나 */}
            <p className="aq-pbl-start-lead">이 앨범 곡의 <b>작사·작곡 저작권료</b>도 받아 보세요. 방송·스트리밍에서 생기는 저작권료를 AUDENIQ이 받아 배급 정산금과 같은 잔액에 쌓아 드려요.</p>
            <ol className="aq-pbl-start-steps">
              {START_STEPS.map(([t, d], i) => (
                <li key={t}><span className="aq-bundle-no" aria-hidden="true">{i + 1}</span><span className="min-0"><strong>{t}</strong><small>{d}</small></span></li>
              ))}
            </ol>
            <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void prepare()}>
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

      {(openPrep || selected.length > 0) && (
        <ApplyPrep
          title={selected.length ? `고른 ${selected.length}곡 준비 상태` : '저작권 관리 신청 준비'}
          summary={prep.summary} groups={prep.groups.filter(g => g.key !== 'fix' && (!selected.length || g.key !== 'tracks'))}
          readyCount={prep.ready.length} total={scope.filter(w => ['ready', 'blocked'].includes(workStage(w))).length}
          onGo={goPrep} applyTo={prep.ready.length ? applyTo(prep.ready) : null}
        />
      )}

      <div className="aq-relinfo-card aq-pbl-track-list">
        <div className="aq-tracklist-head">
          <h2 className="subhead">곡별 저작권</h2>
          <span className="aq-pbl-track-tools">
            {a.mine.length > 1 && (
              <button type="button" className="ui-btn is-text is-sm" onClick={() => setPicked(allPicked ? new Set() : new Set(a.mine.map(w => w.id)))}>
                {allPicked ? '선택 해제' : '전체 선택'}
              </button>
            )}
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
                    <span className="aq-pbl-pick is-empty" aria-hidden="true" />
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
            const on = picked.has(w.id);
            return (
              <li key={t.id} className={on ? 'is-picked' : undefined}>
                <div className="aq-pbl-trackline">
                  <label className="aq-pbl-pick">
                    <input type="checkbox" checked={on} onChange={() => toggle(w.id)} aria-label={`${t.title} 선택`} />
                  </label>
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
                </div>
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
      </div>

      {/* 고른 곡으로 이어서 하는 일 — 화면 아래에 붙어 따라온다 */}
      {selected.length > 0 && (
        <div className="aq-pbl-selbar" role="region" aria-label="고른 곡으로 할 일">
          <span className="aq-pbl-selbar-count"><b>{selected.length}곡</b> 골랐어요</span>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => document.getElementById('pubPrepTitle')?.scrollIntoView({ behavior: 'smooth', block: 'center' })}>준비 상태</button>
          <button type="button" className="ui-btn is-soft is-sm" onClick={download}><Glyph name="download" size={14} className="aq-glyph-lead" />내역</button>
          <button type="button" className="ui-btn is-primary is-sm" onClick={bundle}>신청 대상으로 묶기</button>
          <button type="button" className="ui-btn is-text is-sm aq-pbl-selbar-close" aria-label="선택 해제" onClick={() => setPicked(new Set())}><Glyph name="close" size={16} /></button>
        </div>
      )}

      <Confirmations works={a.mine} onOpen={(w, cid) => setSheet({ work: w, no: trackNo.get(w.id), focus: { field: 'confirm', contributorId: cid } })} />

      {/* 계약은 생긴 뒤에만 — 심사·보완 중에는 위 안내가 할 일을 말한다 */}
      {a.managed.some(w => w.contractId) && (
        <div className="aq-relinfo-card">
          <h2 className="subhead">계약</h2>
          <ol className="aq-bundle-docs"><PublishingBundleStep releaseId={rel.id} /></ol>
        </div>
      )}

      {(a.managed.length > 0 || albumRoyalties.length > 0) && (
        <section className="aq-relinfo-card aq-album-earn" aria-labelledby="albumEarn">
          <div className="ui-section-head">
            <h2 id="albumEarn" className="subhead">이 앨범 저작권료</h2>
            {albumRoyalties.length > 0 && <Link to={`/reports?type=publishing&release=${encodeURIComponent(rel.id)}`} className="aq-head-link">리포트에서 보기<Glyph name="chevron-right" size={14} /></Link>}
          </div>
          {albumRoyalties.length ? (
            <KeyValue cols={2} items={[
              ['누적 확정 저작권료', <b className="aq-pub-amount"><Money value={totals.confirmed + totals.paid} animate /></b>],
              ['확정 전 예상', totals.estimated ? <Money value={totals.estimated} /> : '—'],
            ]} />
          ) : (
            <p className="ui-field-help">관리 단체가 사용료를 나눠 주면 여기에 보여 드려요. 보통 사용된 달부터 3~6개월 걸려요.</p>
          )}
        </section>
      )}

      {sheet && (
        <WorkSheet work={works.find(x => x.id === sheet.work.id) ?? sheet.work} no={sheet.no} all={works} focus={sheet.focus} releaseId={rel.id}
          onClose={() => setSheet(null)} onSaved={saved} />
      )}
      {attaching && <AttachmentModal works={editable} onClose={() => setAttaching(false)} onAdded={saved} />}
    </div>
  );
}
