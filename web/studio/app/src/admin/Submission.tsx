// 심사 화면 — 아티스트가 스튜디오에서 입력·서명한 내용 전체
//   · 배급 신청서: 번호·신청인·서명·원본 일치 확인 + 정식 서류 보기
//   · 입력 내용: 발매 정보·권리 표기·플랫폼 프로필·앨범 소개·권리 확인
//   · 부가서비스·해당 항목: 고른 항목과 세부 내용(첨부 파일 이름 포함), 고르지 않은 항목도 함께
//   · 트랙: 곡마다 펼쳐서 크레딧 이름·가사·음원 파일까지
// 스튜디오 외 경로로 접수돼 입력 기록(draft)이 없으면 서버 신청 정보만 보여 준다.
import { useEffect, useState, type ReactNode } from 'react';
import type { ReleaseSheet, Track } from './api';
import { day, dspLabel, when } from './labels';
import { Chip, Empty, Section } from './ui';
import {
  CONTENT_ID_ACKS, PROFILE_LINKS, RIGHTS_OPTIONS, SERVICE_OPTIONS, draftFromSheet, displayCode, draftNotes, genreLabel, kindLabel, languageLabel,
  rightsChecksFor, verifyDraft, wantsContentId, type Integrity, type StudioDraftTrack, type StudioOptions,
} from './application';
import { ApplicationPaper, INTEGRITY_LABEL } from './ApplicationPaper';
import { Modal } from '../components/Modal';
import { Glyph } from '../components/Glyph';
import { CheckIcon } from '../components/Check';
import { audioSpecs } from './audio';

const dash = (v?: string | null) => (v && String(v).trim() ? v : '—');
const INTEGRITY_TONE: Record<Integrity, 'green' | 'red' | 'gray'> = { ok: 'green', changed: 'red', unsigned: 'gray', checking: 'gray' };

export function useIntegrity(sheet: ReleaseSheet): Integrity {
  const [state, setState] = useState<Integrity>('checking');
  const draft = sheet.draft;
  useEffect(() => {
    if (!draft) { setState('unsigned'); return; }
    let live = true;
    setState('checking');
    verifyDraft(draft, sheet.release.title, sheet.signed_application?.content_hash)
      .then(s => { if (live) setState(s); })
      .catch(() => { if (live) setState('changed'); });
    return () => { live = false; };
  }, [draft, sheet.release.title, sheet.signed_application?.content_hash]);
  return state;
}

// ---------------------------------------------------------------------------
// 배급 신청서
// ---------------------------------------------------------------------------
export function ApplicationSection({ sheet }: { sheet: ReleaseSheet }) {
  const draft = sheet.draft;
  const signed = sheet.signed_application;
  const app = draft?.application;
  const integrity = useIntegrity(sheet);
  const [open, setOpen] = useState(false);
  const no = app?.no ?? signed?.application_no ?? '';
  // 스튜디오 입력 기록이 없으면(스튜디오 밖·예전 접수) 서버 신청 정보로 같은 신청서를 만든다
  const paperDraft = draft ?? draftFromSheet(sheet);
  const signature = app?.signature || signed?.signature || '';

  return (
    <Section
      title="배급 신청서"
      meta={no ? displayCode(no) : '전자서명 기록 없음'}
      action={<button type="button" className="adm-btn soft small" onClick={() => setOpen(true)}>신청서 보기</button>}
    >
      <div className="adm-card adm-appdoc">
        <dl className="adm-kv">
          <div><dt>신청인</dt><dd>{dash(app?.signerName ?? signed?.signer_name)}</dd></div>
          <div><dt>구분</dt><dd>{dash(app?.signerRole ?? signed?.signer_role)}</dd></div>
          <div><dt>접수</dt><dd>{when(signed?.received_at) || dash(app?.submittedAt)}</dd></div>
          <div><dt>연락 이메일</dt><dd>{dash(signed?.contact_email)}</dd></div>
        </dl>
        <div className="adm-appdoc-foot">
          {signature
            ? <span className="adm-sig"><img src={signature} alt={`${app?.signerName ?? signed?.signer_name ?? ''} 서명`} /></span>
            : <span className="adm-sig is-empty">서명 이미지 없음</span>}
          <span className="adm-appdoc-state">
            <Chip tone={INTEGRITY_TONE[integrity]}>{INTEGRITY_LABEL[integrity]}</Chip>
            <small>동의 {(app?.agreements ?? signed?.agreements ?? []).length} / 4 항목</small>
          </span>
        </div>
        {integrity === 'changed' && (
          <p className="adm-appdoc-warn">제출 내용으로 다시 계산한 문서 확인 코드가 서명 때와 달라요. 신청서 전체를 열어 입력 내용과 대조해 주세요.</p>
        )}
      </div>
      {open && (
        <Modal title="배급 신청서" onClose={() => setOpen(false)} modalClass="adm-paper-modal">
          <div className="adm-paper-tools">
            <button type="button" className="adm-btn soft small" onClick={() => window.print()}>인쇄 · PDF 저장</button>
          </div>
          <ApplicationPaper sheet={sheet} draft={paperDraft} integrity={integrity} />
        </Modal>
      )}
    </Section>
  );
}

// ---------------------------------------------------------------------------
// 입력 내용
// ---------------------------------------------------------------------------
const DECL_LABEL: Record<string, string> = {
  rights_confirmed: '권리 보유 확인', adult_confirmed: '성인 확인', is_cover: '커버곡', is_remix: '리믹스',
  contains_samples: '샘플 사용', ai_involved: 'AI 활용', explicit_content: '19금 표현',
};

/** 아티스트 권리 확인 — 체크하지 않은 항목만 펼쳐 두고, 확인한 항목은 접는다 */
function RightsChecks({ d }: { d: NonNullable<ReleaseSheet['draft']> }) {
  const list = rightsChecksFor(d.options);
  const off = list.filter(([k]) => !d.rightsChecks?.[k]);
  const on = list.filter(([k]) => d.rightsChecks?.[k]);
  const row = ([k, label]: [string, string], ok: boolean) => (
    <li key={k} className={ok ? 'is-on' : 'is-off'}>
      <span aria-label={ok ? '확인함' : '확인 안 함'}>{ok ? <CheckIcon size={11} /> : <Glyph name="close" size={11} />}</span>
      {label}
    </li>
  );
  return (
    <div className="adm-rights">
      <dt>권리 확인 (아티스트 체크) <small className={off.length ? 'is-bad' : ''}>{off.length ? `${off.length}개 체크 안 함` : `${list.length}개 모두 확인`}</small></dt>
      {off.length > 0 && <ul>{off.map(c => row(c, false))}</ul>}
      {on.length > 0 && (
        <details className="adm-rights-more">
          <summary>확인한 항목 {on.length}개<Glyph name="chevron-right" size={12} /></summary>
          <ul>{on.map(c => row(c, true))}</ul>
        </details>
      )}
    </div>
  );
}

export function EnteredInfoSection({ sheet }: { sheet: ReleaseSheet }) {
  const r = sheet.release;
  const app = sheet.application;
  const d = sheet.draft;
  const decl = app.declarations ?? {};
  const platforms = d?.platforms?.length ? d.platforms : app.platforms;
  const territories = d?.territories ?? [];
  const notes = d ? draftNotes(d) : '';
  const ap = d?.artistProfile;
  return (
    <Section title="입력 내용" meta={d ? '아티스트가 신청서에 적은 그대로' : '서버 신청 정보'}>
      <div className="adm-card">
        <dl className="adm-kv">
          <div><dt>아티스트</dt><dd>{dash(d?.artist ?? app.artist)}</dd></div>
          <div><dt>발매 유형</dt><dd>{d ? kindLabel(d.type) || '—' : '—'}</dd></div>
          <div><dt>장르 · 언어</dt><dd>{[d ? genreLabel(d) : app.genre, d ? languageLabel(d.language) : app.language].filter(Boolean).join(' · ') || '—'}</dd></div>
          <div><dt>발매 예정일</dt><dd>{day(d?.release_date ?? app.release_date)}</dd></div>
          {(d?.originalDate || app.original_date) && <div><dt>최초 발매일</dt><dd>{day(d?.originalDate ?? app.original_date)}</dd></div>}
          <div><dt>레이블</dt><dd>{dash(d?.label ?? app.label)}</dd></div>
          <div><dt>UPC</dt><dd>{d?.upc || r.upc || '발급 전'}</dd></div>
          {d && <div><dt>마스터 권리자</dt><dd>{dash(d.ownership)}</dd></div>}
          <div><dt>℗ / ©</dt><dd>{[d?.phonogram ?? app.p_line, d?.copyright ?? app.c_line].filter(Boolean).join(' / ') || '—'}</dd></div>
          {d && (
            <div style={{ gridColumn: '1 / -1' }}>
              <dt>플랫폼 프로필</dt>
              <dd>
                {!ap || ap.isNew ? '신규 아티스트 (새 프로필 생성)' : (
                  <span className="adm-links">
                    {PROFILE_LINKS.filter(l => ap[l.key]).map(l => (
                      <a key={l.key} href={ap[l.key]} target="_blank" rel="noopener noreferrer">{l.label} <Glyph name="arrow-up-right" size={11} /></a>
                    ))}
                  </span>
                )}
              </dd>
            </div>
          )}
          {d && <div style={{ gridColumn: '1 / -1' }}><dt>배급 지역</dt><dd>{territories.includes('WORLD') ? '전 세계' : dash(territories.join(', '))}</dd></div>}
          <div style={{ gridColumn: '1 / -1' }}>
            <dt>배급 플랫폼 <small className="muted">{platforms.length}곳</small></dt>
            <dd className="adm-dsps">{platforms.length ? platforms.map(p => <span key={p}>{dspLabel(p)}</span>) : '—'}</dd>
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
        {notes && (
          <div className="adm-notes">
            <dt>앨범 소개</dt>
            <p>{notes}</p>
          </div>
        )}
        {d?.rightsChecks && <RightsChecks d={d} />}
      </div>
    </Section>
  );
}

// ---------------------------------------------------------------------------
// 부가서비스 · 해당 항목
// ---------------------------------------------------------------------------
function Row({ label, children }: { label: string; children: ReactNode }) {
  return <div className="adm-opt-row"><dt>{label}</dt><dd>{children}</dd></div>;
}
const file = (name?: string) => (name ? <span className="adm-file"><Glyph name="doc" size={12} />{name}</span> : <span className="muted">첨부 없음</span>);

function optionDetail(key: keyof StudioOptions, o: StudioOptions, tracks: StudioDraftTrack[]): ReactNode {
  switch (key) {
    case 'express':
      return <Row label="사유">{dash(o.expressReason)}{o.expressAck ? ' · 이용 조건 동의함' : ''}</Row>;
    case 'minor':
      return (
        <>
          <Row label="법정대리인">{[o.guardian, o.guardianRelation, o.guardianContact].filter(Boolean).join(' · ') || '—'}</Row>
          {o.guardian2 && <Row label="법정대리인 2">{[o.guardian2, o.guardian2Relation, o.guardian2Contact].filter(Boolean).join(' · ')}</Row>}
          <Row label="동의 절차">{o.guardianConsentDone ? `완료 · ${[o.familyCertName, o.familyCertMethod].filter(Boolean).join(' · ') || '가족관계 확인'}` : '미완료'}</Row>
        </>
      );
    case 'cover':
      return (
        <>
          <Row label="원곡">
            {o.coverTracks?.length ? o.coverTracks.map((c, i) => {
              const t = tracks.find(x => x.id === c.trackId);
              return <p key={c.trackId || i}>{t?.title ? `${t.title} ← ` : ''}{c.originalTitle || '제목 미기재'} · {c.originalArtist || '원곡 아티스트 미기재'}{c.originalWriters ? ` · ${c.originalWriters}` : ''}</p>;
            }) : '원곡 정보 미기재'}
          </Row>
          <Row label="이용 허락서">{file(o.coverLicenseFile)}{o.coverRightsAck ? ' · 권리 확보 확인함' : ''}</Row>
        </>
      );
    case 'sample': return <Row label="원본 이용 허락서">{file(o.sampleLicenseFile)}</Row>;
    case 'featured': return <Row label="참여자 동의서">{file(o.featuredConsentFile)}</Row>;
    case 'shared': return <Row label="공동 권리 계약서">{file(o.sharedContractFile)}</Row>;
    case 'rerelease': {
      const pick = (map: Record<string, string>, v?: string) => (v && map[v]) || '확인 필요';
      const isrcs = (o.rereleaseTracks ?? []).filter(t => t.previousIsrc?.trim());
      return (
        <>
          <Row label="상황">{pick(RERELEASE_KIND, o.rereleaseKind)}</Row>
          <Row label="기존 발매">{[o.previousTitle, o.previousReleaseDate && `최초 ${o.previousReleaseDate}`].filter(Boolean).join(' · ') || '기존 발매명 미기재'}</Row>
          <Row label="이전 유통사">{dash(o.previousDistributor)} · {pick(RERELEASE_AVAILABILITY, o.previousAvailability)}</Row>
          <Row label="녹음 · 권한">{pick(RERELEASE_AUDIO, o.rereleaseAudio)} · {pick(RERELEASE_RIGHTS, o.rereleaseRights)}</Row>
          {(o.previousUpc || isrcs.length > 0 || o.previousId) && (
            <Row label="기존 코드">{[o.previousUpc && `UPC ${o.previousUpc}`, ...(isrcs.length ? isrcs.map(t => `ISRC ${t.previousIsrc}`) : o.previousId ? [`ISRC ${o.previousId}`] : [])].filter(Boolean).join(' · ')}</Row>
          )}
          {o.rereleasePermissionFile && <Row label="배급 허락서">{file(o.rereleasePermissionFile)}</Row>}
          {o.previousUrl && <Row label="기존 링크"><a href={o.previousUrl} target="_blank" rel="noopener noreferrer">{o.previousUrl}</a></Row>}
          {o.rereleaseNotes && <Row label="메모">{o.rereleaseNotes}</Row>}
        </>
      );
    }
    case 'ai': {
      const uses = [...(o.aiUses ?? []), o.aiUseOther].filter(Boolean);
      const tools = [...(o.aiTools ?? []), o.aiToolOther].filter(Boolean);
      return (
        <>
          <Row label="활용 내역">{uses.length ? <span className="adm-tags">{uses.map(u => <span key={u}>{u}</span>)}</span> : dash(o.aiTool)}</Row>
          <Row label="사용한 도구">{tools.length ? <span className="adm-tags">{tools.map(u => <span key={u}>{u}</span>)}</span> : '—'}</Row>
        </>
      );
    }
    default: return null;
  }
}

const RERELEASE_KIND: Record<string, string> = { transfer: '유통사를 AUDENIQ로 이전', redistribute: '서비스 종료된 음원 재발매', new_version: '새 녹음·변경 버전 발매' };
const RERELEASE_AVAILABILITY: Record<string, string> = { live: '현재 서비스 중', takedown_requested: '이전 유통사에 종료 요청', removed: '서비스 종료됨', unknown: '서비스 상태 확인 필요' };
const RERELEASE_AUDIO: Record<string, string> = { same: '기존 녹음 그대로 (기존 ISRC 유지)', changed: '음악 내용 변경 (새 ISRC)', unknown: '녹음 동일 여부 확인 필요' };
const RERELEASE_RIGHTS: Record<string, string> = { owned: '권리자 본인', permission: '배급 허락 확보', pending: '권한 확인 중' };

function OptionGroup({ title, list, o, tracks }: { title: string; list: [keyof StudioOptions, string][]; o: StudioOptions; tracks: StudioDraftTrack[] }) {
  const on = list.filter(([k]) => o[k] === true);
  const off = list.filter(([k]) => o[k] !== true);
  return (
    <div className="adm-opt-group">
      <h3>{title} <small>{on.length ? `${on.length}개 선택` : '선택 없음'}</small></h3>
      {on.map(([k, label]) => (
        <div key={k} className="adm-opt is-on">
          <div className="adm-opt-head"><span className="adm-opt-mark"><CheckIcon size={11} /></span><b>{label}</b></div>
          <dl>{optionDetail(k, o, tracks)}</dl>
        </div>
      ))}
      {off.length > 0 && <p className="adm-opt-off">선택 안 함 · {off.map(([, label]) => label).join(', ')}</p>}
    </div>
  );
}

export function OptionsSection({ sheet }: { sheet: ReleaseSheet }) {
  const o = (sheet.draft?.options ?? sheet.application.options ?? null) as StudioOptions | null;
  if (!o) return null;
  const tracks = sheet.draft?.draftTracks ?? [];
  const picked = [...SERVICE_OPTIONS, ...RIGHTS_OPTIONS].filter(([k]) => o[k] === true).length;
  const contentId = wantsContentId(sheet.draft?.platforms ?? sheet.application.platforms);
  const acked = CONTENT_ID_ACKS.filter(([k]) => o[k] === true).length;
  return (
    <Section title="부가서비스 · 해당 항목" meta={picked ? `${picked}개 선택` : '일반 발매'}>
      <div className="adm-card adm-opts">
        <OptionGroup title="부가서비스" list={SERVICE_OPTIONS} o={o} tracks={tracks} />
        <OptionGroup title="해당 항목" list={RIGHTS_OPTIONS} o={o} tracks={tracks} />
        {contentId && (
          <div className="adm-opt-group">
            <h3>YouTube Content ID 권리 확인 <small>{acked}/{CONTENT_ID_ACKS.length} 확인</small></h3>
            {CONTENT_ID_ACKS.map(([k, label]) => (
              <div key={k} className={`adm-opt-ack${o[k] === true ? ' is-on' : ''}`}>
                <span className="adm-opt-mark" aria-hidden="true">{o[k] === true ? <CheckIcon size={11} /> : <Glyph name="close" size={10} />}</span>
                <span>{label}</span>
                <small>{o[k] === true ? '확인함' : '체크 안 함'}</small>
              </div>
            ))}
          </div>
        )}
      </div>
    </Section>
  );
}

// ---------------------------------------------------------------------------
// 트랙
// ---------------------------------------------------------------------------
const ROLE_KO: Record<string, string> = { COMPOSER: '작곡', LYRICIST: '작사', ARRANGER: '편곡', PRODUCER: '프로듀서', PERFORMER: '연주', MAIN_ARTIST: '아티스트' };

/** 서버 트랙과 스튜디오 입력 트랙을 짝지운다 (serverId → 같은 순서의 곡) */
function matchDraft(t: Track, i: number, drafts: StudioDraftTrack[]): StudioDraftTrack | undefined {
  return drafts.find(x => x.serverId === t.id) ?? drafts.filter(x => x.title?.trim())[i];
}

export function TracksSection({ sheet }: { sheet: ReleaseSheet }) {
  const server = sheet.application.tracks ?? [];
  const drafts = (sheet.draft?.draftTracks ?? []).filter(t => t.title?.trim());
  const rows: { key: string; no: string; s?: Track; d?: StudioDraftTrack }[] = server.length
    ? server.map((t, i) => ({ key: t.id, no: `${t.disc_number > 1 ? `${t.disc_number}-` : ''}${t.track_number}`, s: t, d: matchDraft(t, i, drafts) }))
    : drafts.map((d, i) => ({ key: d.id ?? String(i), no: String(i + 1), d }));
  return (
    <Section title="트랙" meta={`${rows.length}곡 · 눌러서 크레딧·가사 보기`}>
      {rows.length ? (
        <div className="adm-tracks">
          {rows.map(({ key, no, s, d }) => {
            const title = d?.title || s?.title || '제목 없음';
            const explicit = d?.explicit ?? s?.parental_advisory;
            return (
              <details key={key} className="adm-track">
                <summary>
                  <span className="adm-track-no">{no}</span>
                  <span className="adm-track-title">
                    <b>{title}</b>{(d?.version || s?.version) && <small> ({d?.version || s?.version})</small>}
                    {explicit && <Chip tone="red">19</Chip>}
                    {d?.instrumental && <Chip tone="gray">연주곡</Chip>}
                  </span>
                  <span className="adm-track-meta">
                    <span>{d?.duration || '—'}</span>
                    <span className="adm-code">{s?.isrc || d?.isrc || 'ISRC 발급 전'}</span>
                  </span>
                  <Glyph name="chevron-right" size={14} className="adm-track-chev" />
                </summary>
                <div className="adm-track-body">
                  {d ? (
                    <dl className="adm-kv">
                      <div><dt>작곡</dt><dd>{dash(d.composers)}</dd></div>
                      <div><dt>작사</dt><dd>{d.instrumental ? '연주곡' : dash(d.lyricists)}</dd></div>
                      <div><dt>편곡</dt><dd>{dash(d.arrangers)}</dd></div>
                      <div><dt>실연·연주</dt><dd>{dash(d.performers)}</dd></div>
                      <div><dt>프로듀서</dt><dd>{dash(d.producer)}</dd></div>
                      <div><dt>피처링</dt><dd>{dash(d.featuring)}</dd></div>
                      <div style={{ gridColumn: '1 / -1' }}><dt>음원 파일</dt><dd>{[d.audioName, d.audioSpec].filter(Boolean).join(' · ') || s?.asset_kind || '—'}</dd></div>
                      {s && <div style={{ gridColumn: '1 / -1' }}><dt>서버 측정</dt><dd>{audioSpecs(sheet.track_audio?.[s.id])}</dd></div>}
                    </dl>
                  ) : (
                    <dl className="adm-kv">
                      <div><dt>크레딧</dt><dd>{s?.credits.map(c => ROLE_KO[c.role] ?? c.role).join(', ') || '—'}</dd></div>
                      <div><dt>음원</dt><dd>{s?.asset_kind ?? '—'}</dd></div>
                      {s && <div><dt>서버 측정</dt><dd>{audioSpecs(sheet.track_audio?.[s.id])}</dd></div>}
                    </dl>
                  )}
                  {d && !d.instrumental && (
                    <div className="adm-lyrics">
                      <dt>가사</dt>
                      {d.lyrics?.trim() ? <pre>{d.lyrics}</pre> : <p className="muted">가사를 입력하지 않았어요.</p>}
                    </div>
                  )}
                </div>
              </details>
            );
          })}
        </div>
      ) : <Empty icon={<Glyph name="music" size={22} />} title="트랙 정보가 없어요" />}
    </Section>
  );
}
