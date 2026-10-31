// 발매 상세 '퍼블리싱' 탭 — 이 앨범 곡들의 작사·작곡 저작권을 '관리'하는 곳. 배급과 같은 발매 상세 안에서 끝낸다:
//  · 신청 준비 점검: '신청 가능 3곡 · 지분 확인 필요 1곡' — 항목을 누르면 그 곡 시트의 그 칸이 열린다 (발매 준비 항목과 같은 카드)
//  · 곡별 저작권: '정보' 탭 트랙 목록과 같은 줄. 누르면 곡 시트에서 참여자·지분·관리 상태를 보고 고친다 (저장하면 이 목록으로)
//  · 참여자 확인 현황: 확인 완료·대기·이의 제기 — 미확인 참여자와 관련 곡만 모아 본다
//  · 자료 첨부: 창에서 곡·종류·파일을 골라 올린다
// 번 돈은 한 장으로만 — 월별 추이·곡별·자세한 내역은 퍼블리싱 메뉴에서 이 앨범을 골라 본다
import { useMemo, useState } from 'react';
import { Link } from '../../lib/router';
import type { ReleaseDetail } from '../../api/types';
import { pubApi, type Work } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { pct } from '../../lib/publishing';
import {
  CONFIRM_LABEL, CONFIRM_TONE, STAGE_LABEL, STAGE_TONE, albumReadiness, confirmationBoard, workStage,
  type ConfirmState, type PrepField, type PrepGroup,
} from '../../lib/publishingPrep';
import { Glyph, type GlyphName } from '../Glyph';
import { SkeletonBlock } from '../Skeleton';
import { ManageHead } from '../ManageHead';
import { useToast } from '../Toast';
import { peopleLine } from './parts';
import { royaltyTotals } from './Royalties';
import { Money } from '../Money';
import { albumPublishing } from './ReleasePublishing';
import { Modal } from '../Modal';
import { WorkSheet } from './WorkSheet';
import { AttachmentModal } from './Attachments';
import { InviteModal } from './InviteModal';
import { Badge, ChipGroup, IconTile } from '../../ui';

import '../../styles/split/Dashboard.css';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

const PREP_ICON: Record<PrepGroup['key'], GlyphName> = { tracks: 'music', fix: 'warning', split: 'scale', confirm: 'users', ipi: 'user' };
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

/** 참여자 확인 현황 — 준비 점검의 '참여자 확인'을 누르면 뜨는 창. 사람마다 한 줄, 그 사람이 들어간 곡을 아래에.
 *  탭에 카드로 두면 준비 점검과 같은 이야기를 한 번 더 했다 */
function PeopleModal({ works, onOpen, onClose, onSent }: { works: Work[]; onOpen: (w: Work, contributorId: string) => void; onClose: () => void; onSent: () => void }) {
  const [inviteWork, setInviteWork] = useState<Work | null>(null);
  const board = useMemo(() => confirmationBoard(works), [works]);
  // 확인 대기인 사람·곡 모두에게 한 번에 — 앨범이면 같은 사람에게 곡마다 따로 누를 일이 많았다
  const pendingEntries = board.people.filter(p => !p.isMe).flatMap(p => p.entries.filter(e => e.contributor.confirm === 'pending' && (e.work.myRole === 'owner' || e.work.myRole === 'editor')));
  // 곡마다 한 창에서 그 곡의 확인 대기 모두에게 링크를 — 곡별로 묶는다
  const pendingWorks = Array.from(new Map(pendingEntries.map(e => [e.work.id, e.work])).values());
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
      {pendingWorks.length > 0 && (
        <div className="ui-list is-inset" aria-label="곡별 확인 링크 보내기">
          {pendingWorks.map(w => (
            <button key={w.id} type="button" className="ui-row" onClick={() => setInviteWork(w)}>
              <Glyph name="link" size={16} />
              <span className="min-0"><span className="ui-row-title">‘{w.title}’ 확인 링크 보내기</span><span className="ui-row-meta"><span>{w.contributors.filter(c => !c.isMe && c.confirm === 'pending').map(c => c.name).join(' · ')}</span></span></span>
              <Glyph name="chevron-right" size={16} />
            </button>
          ))}
        </div>
      )}
      {inviteWork && <InviteModal work={inviteWork} people={inviteWork.contributors.filter(c => !c.isMe && c.confirm === 'pending')} onClose={() => setInviteWork(null)} onDone={onSent} />}
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

export function AlbumPublishingTab({ rel }: { rel: ReleaseDetail }) {
  const toast = useToast();
  const { data: works, reload } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const { data: royalties } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const { data: contracts } = useAsync(() => pubApi.listContracts(), [], { key: 'pub-contracts' });
  const [busy, setBusy] = useState(false);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [peopleOpen, setPeopleOpen] = useState(false);
  if (!works) return <SkeletonBlock height={240} />;

  const a = albumPublishing(works, rel.id, rel.tracks.length);
  const byTrack = new Map<string, Work>(a.mine.flatMap(w => w.recordings.filter(r => r.releaseId === rel.id).map(r => [r.trackId, w] as const)));
  const trackNo = new Map<string, number>();
  rel.tracks.forEach((t, i) => { const w = byTrack.get(t.id); if (w && !trackNo.has(w.id)) trackNo.set(w.id, i + 1); });
  const missing = rel.tracks.filter(t => !byTrack.has(t.id)).length;
  const prep = albumReadiness(a.mine, missing);
  const applyTo = (list: Work[]) => `/publishing/apply?release=${encodeURIComponent(rel.id)}&works=${list.map(w => encodeURIComponent(w.id)).join(',')}`;
  const workIds = new Set(a.mine.map(w => w.id));
  const albumRoyalties = (royalties ?? []).filter(r => workIds.has(r.workId));
  const totals = royaltyTotals(albumRoyalties);
  const fixWork = a.mine.find(w => w.registration === 'needs_fix' || w.application?.status === 'needs_fix');
  // 숫자는 트랙 기준으로 — 한 곡이 원곡·Inst.처럼 두 트랙에 실리면 '2곡 중 1곡'으로 어긋났다
  const trackCount = (list: Work[]) => rel.tracks.filter(t => { const w = byTrack.get(t.id); return !!w && list.includes(w); }).length;
  const managedTracks = trackCount(a.managed);
  // 보완 요청은 심사 중과 따로 센다 — 이용자가 할 일이 있는 곡이라 막대·이름표에서도 구분한다
  const fixTracks = trackCount(a.applying.filter(w => !!w.fix || w.application?.status === 'needs_fix'));
  const applyingTracks = trackCount(a.applying) - fixTracks;
  const signingTracks = trackCount(a.toSign);
  // 승인돼 계약서가 만들어졌는데 아직 서명 전 — 배급처럼 계약서 화면에서 서명한다
  const toSign = (contracts ?? []).find(c => c.status === 'pending_sign' && c.releaseIds?.includes(rel.id));
  const text = a.fix ? '심사 중에 보완할 곡이 생겼어요. 고쳐서 다시 내면 심사가 이어져요.'
    : managedTracks ? `${managedTracks === rel.tracks.length ? `${rel.tracks.length}곡 모두` : `${rel.tracks.length}곡 중 ${managedTracks}곡의`} 저작권을 AUDENIQ이 관리하고 있어요. 저작권료는 정산 잔액에 함께 쌓여요.`
    : applyingTracks ? `${applyingTracks}곡을 심사하고 있어요. 끝나면 알림으로 알려 드려요.`
    : prep.ready.length ? `${prep.ready.length}곡을 바로 신청할 수 있어요. 신청하면 담당자가 참여자와 지분을 확인해요.`
    : '이 앨범 곡의 작사·작곡 저작권도 맡기면 방송·스트리밍 저작권료까지 받아 드려요. 맡기지 않아도 배급은 그대로예요.';
  // 앨범 곡이 지금 어디에 있는지 한 막대로 — 정산 화면의 '쌓인 정산금' 막대와 같은 읽는 법(큰 숫자 · 막대 · 이름표)
  const preparedTracks = rel.tracks.length - managedTracks - applyingTracks - fixTracks - signingTracks - missing;
  const cover = ([
    ['managed', '관리 중', managedTracks], ['signing', '서명 필요', signingTracks], ['fix', '보완 필요', fixTracks], ['applying', '심사 중', applyingTracks],
    ['prepared', '정리함', preparedTracks], ['missing', '정리 전', missing],
  ] as const).filter(([, , n]) => n > 0);
  const editable = a.mine.filter(w => w.myRole === 'owner' || w.myRole === 'editor');
  const contractId = a.managed.find(w => w.contractId)?.contractId ?? null;
  // 이 앨범에서 아직 아무 곡도 정리하지 않았으면 처음 시작하는 화면
  const fresh = a.mine.length === 0 && missing > 0;
  const openPrep = !fresh && (prep.groups.some(g => g.state !== 'done') || prep.ready.length > 0);

  const prepare = async () => {
    setBusy(true);
    try {
      // 실제로 한 일을 알린다 — 같은 제목의 곡이 이미 있으면 새로 만들지 않고 그 곡에 연결하고, 제목 없는 곡은 건너뛴다
      const r = await pubApi.prepareWorksFromRelease(rel.id, false); reload();
      const done = [r.created && `${r.created}곡을 크레딧으로 정리`, r.linked && `${r.linked}곡을 같은 제목의 곡에 연결`].filter(Boolean).join('하고 ');
      if (done) toast(`${done}했어요. 지분이 맞는지 확인해 주세요.`, { tone: 'success', sub: r.skipped ? `제목이 없는 ${r.skipped}곡은 건너뛰었어요. 곡 제목을 적은 뒤 다시 정리해 주세요.` : undefined });
      else toast('새로 정리할 곡이 없어요.', { tone: 'info', sub: r.skipped ? `제목이 없는 ${r.skipped}곡은 정리할 수 없어요.` : undefined });
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
    <div className="aq-detail-stack aq-album-pub">
      {/* 먼저 할 일은 큰 카드 밖, 탭 맨 위에 따로 — 카드 안 카드로 들어가 모바일에서 답답했다 (곡 상세의 보완 카드처럼) */}
      {/* 보완 요청이 있으면 그 일이 이 탭의 판단 하나 — 홈 '먼저 할 일'과 같은 주황 면 */}
      {!fresh && !a.fix && toSign && (
        <div className="aq-pbl-hero-fix is-sign">
          <span className="min-0">
            <small>먼저 할 일</small>
            <strong>저작권 관리 계약서에 서명해 주세요</strong>
            <span>담당자가 {toSign.works.map(w => `‘${w.title}’`).join(', ')} 신청을 승인했어요. 서명하면 계약이 맺어지고 관리 단체 등록이 시작돼요.</span>
          </span>
          <Link to={`/contracts/publishing/${encodeURIComponent(toSign.id)}`} className="ui-btn is-primary aq-hero-cta">
            확인하고 서명
            <span className="aq-cta-arrow" aria-hidden="true">
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
            </span>
          </Link>
        </div>
      )}
      {!fresh && a.fix && (
        <div className="aq-pbl-hero-fix ui-tone-warn">
          <span className="min-0">
            <small>먼저 할 일</small>
            <strong>{fixWork ? `‘${fixWork.title}’에 보완 요청이 왔어요` : '보완 요청이 왔어요'}</strong>
            <span>{fixWork?.fix?.reason ?? '아래 곡을 눌러 고쳐 주세요. 고쳐서 다시 내면 심사가 이어져요.'}</span>
          </span>
          {fixWork && (
            <button type="button" className="ui-btn is-primary aq-hero-cta" onClick={() => setSheet({ work: fixWork, no: trackNo.get(fixWork.id), focus: { field: 'fix' } })}>
              보완하기
              <span className="aq-cta-arrow" aria-hidden="true">
                <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
              </span>
            </button>
          )}
        </div>
      )}
      <section className="ui-card aq-manage-card aq-pbl-hero" aria-labelledby="albumPubHead">
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
            <div className="aq-pbl-cover" role="group" aria-label={`${rel.tracks.length}곡 중 ${cover.map(([, l, n]) => `${l} ${n}곡`).join(', ')}`}>
              <p className="aq-pbl-cover-cap" aria-hidden="true">저작권을 맡긴 곡</p>
              <strong className="aq-pbl-cover-num" aria-hidden="true"><b>{managedTracks + signingTracks + applyingTracks + fixTracks}</b><small>/ {rel.tracks.length}곡</small></strong>
              <div className="aq-pbl-cover-bar" aria-hidden="true">
                {cover.map(([k, , n]) => <i key={k} className={`is-${k}`} style={{ flexGrow: n }} />)}
              </div>
              <ul className="aq-pbl-cover-legend" aria-hidden="true">
                {cover.map(([k, l, n]) => <li key={k}><i className={`is-${k}`} />{l}<b>{n}곡</b></li>)}
              </ul>
            </div>
            {!a.fix && <p className="aq-pbl-cover-note">{text}</p>}
            {/* 계약·저작권료는 한 줄씩 — 따로 카드를 두면 탭이 길어지기만 했다. 자세한 건 계약서·리포트에서 */}
            {(contractId || albumRoyalties.length > 0) && (
              <div className="ui-list is-inset aq-pbl-head-rows">
                {contractId && (
                  <Link to={`/contracts/publishing/${encodeURIComponent(contractId)}`} className="ui-row">
                    <IconTile size="sm"><Glyph name="sign" size={16} /></IconTile>
                    <span className="min-0"><span className="ui-row-title">저작권 관리 계약</span><span className="ui-row-meta"><span>{managedTracks}곡 관리 중</span></span></span>
                    <Glyph name="chevron-right" size={16} />
                  </Link>
                )}
                {albumRoyalties.length > 0 && (
                  <Link to={`/reports?type=publishing&release=${encodeURIComponent(rel.id)}`} className="ui-row">
                    <IconTile size="sm"><Glyph name="won" size={16} /></IconTile>
                    <span className="min-0">
                      <span className="ui-row-title">누적 저작권료 <Money value={totals.confirmed + totals.paid} /></span>
                      {totals.estimated > 0 && <span className="ui-row-meta"><span>확정 전 예상 <Money value={totals.estimated} /></span></span>}
                    </span>
                    <Glyph name="chevron-right" size={16} />
                  </Link>
                )}
              </div>
            )}
          </>
        )}
      </section>

      {openPrep && (
        <ApplyPrep
          title="저작권 관리 신청 준비"
          summary={prep.summary} groups={prep.groups.filter(g => g.key !== 'fix')}
          readyCount={prep.ready.length} total={a.mine.filter(w => ['ready', 'blocked'].includes(workStage(w))).length}
          onGo={goPrep} applyTo={prep.ready.length ? applyTo(prep.ready) : null}
        />
      )}

      <div className="aq-relinfo-card aq-pbl-track-list">
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
      </div>

      {sheet && (
        <WorkSheet work={works.find(x => x.id === sheet.work.id) ?? sheet.work} no={sheet.no} all={works} focus={sheet.focus} releaseId={rel.id}
          onClose={() => setSheet(null)} onSaved={saved} />
      )}
      {attaching && <AttachmentModal works={editable} onClose={() => setAttaching(false)} onAdded={saved} />}
      {peopleOpen && (
        <PeopleModal works={a.mine} onClose={() => setPeopleOpen(false)} onSent={saved}
          onOpen={(w, cid) => { setPeopleOpen(false); setSheet({ work: w, no: trackNo.get(w.id), focus: { field: 'confirm', contributorId: cid } }); }} />
      )}
    </div>
  );
}
