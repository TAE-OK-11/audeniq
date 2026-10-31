// 곡 등록 첫 화면 — 퍼블리싱은 곡을 세 갈래로 받는다 (lib/publishing WORK_ORIGINS).
// 내가 발매한 곡은 여기서 그 발매 곡을 고르면 곡 정보·크레딧·음원·가사·커버를 가져온 등록 화면(?release=&track=)으로 간다 — 두 번 적지 않게.
// 다른 아티스트가 발매한 곡 · 발매 전 곡은 고른 갈래(?type=)로 등록 화면을 연다.
// 모양은 배급 신청서와 같다: 신청서 틀(WizardShell), 고르기 카드(aq-choice), 아래 고정 버튼
import { useState } from 'react';
import { useNavigate } from '../../lib/router';
import { api } from '../../api/client';
import type { Work, WorkOrigin } from '../../api/publishing';
import { useAsync } from '../../hooks/useAsync';
import { WORK_ORIGINS } from '../../lib/publishing';
import { STATUS_LABEL, statusTone } from '../../lib/format';
import { WizardShell } from '../WizardShell';
import { ReleaseCover } from '../ReleaseCover';
import { SkeletonBlock } from '../Skeleton';
import { Glyph } from '../Glyph';
import { Badge, EmptyState } from '../../ui';
import '../../styles/split/Upload.css';
import '../../styles/split/Publishing.css';

export function WorkOriginStart({ works, onPick }: { works: Work[] | null; onPick: (o: Exclude<WorkOrigin, 'own_release'>) => void }) {
  const nav = useNavigate();
  const [origin, setOrigin] = useState<WorkOrigin | null>(null);
  const { data: tracks, loading } = useAsync(async () => {
    const rels = await api.listReleases();
    const details = await Promise.all(rels.map(r => api.getRelease(r.id).catch(() => null)));
    return details.filter(Boolean).flatMap(d => d!.tracks.map(t => ({ t, d: d! })));
  }, [], { key: 'release-tracks-for-works' });
  // 이미 곡으로 등록한 발매 곡 — 새로 만들지 않고 그 곡을 연다
  const linked = new Map((works ?? []).flatMap(w => w.recordings.map(r => [r.trackId, w.id] as const)));

  return (
    <WizardShell
      className="aq-pbl-wizard"
      label="곡 정보 등록" title="새 곡 정보"
      steps={[{ short: '발매 상태', title: '이 곡은 지금\n어떤 상태인가요?', sub: '발매 상태에 따라 할 수 있는 일이 달라요. 고른 상태에 맞는 것만 물어요.' }]}
      step={0} reached={0} onStep={() => {}} onBack={() => nav(-1)}
      summary={{ title: origin ? WORK_ORIGINS.find(x => x[0] === origin)![1] : '발매 상태를 골라 주세요', lines: ['저작권 관리 · Open A&R'] }}
      actions={<>
        <button type="button" id="wizardBack" className="ui-btn is-soft" onClick={() => nav(-1)}>나가기</button>
        {/* 내가 발매한 곡은 아래 목록에서 곡을 누르면 바로 넘어간다 */}
        {origin !== 'own_release' && (
          <button type="button" id="wizardNext" className="ui-btn is-primary" disabled={!origin} onClick={() => { if (origin) onPick(origin); }}>다음으로</button>
        )}
      </>}
    >
      <div className="aq-choice-row aq-pbl-origin-row" role="radiogroup" aria-label="발매 상태">
        {WORK_ORIGINS.map(([v, name, desc]) => (
          <label key={v} className={`aq-choice${origin === v ? ' is-on' : ''}`}>
            <input type="radio" name="workOrigin" value={v} checked={origin === v} onChange={() => setOrigin(v)} />
            <strong>{name}</strong>
            <small>{desc}</small>
          </label>
        ))}
      </div>

      {origin === 'own_release' && (
        <>
          <h2 className="subhead">어느 곡인가요?</h2>
          <p className="ui-field-help">발매 곡을 고르면 배급 신청서에 적은 곡 정보·크레딧·음원·가사·커버를 그대로 가져와요. 지분만 나눠 주면 돼요.</p>
          {loading && !tracks ? <SkeletonBlock height={160} /> : !tracks?.length ? (
            <EmptyState compact title="AUDENIQ으로 낸 발매가 없어요">다른 곳에서 발매했다면 ‘다른 아티스트가 발매한 곡’처럼 발매 정보를 직접 적어 등록할 수 있어요.</EmptyState>
          ) : (
            <div className="ui-list aq-stagger">
              {tracks.map(({ t, d }) => {
                const workId = linked.get(t.id);
                return (
                  <button key={d.id + t.id} type="button" className="ui-row"
                    onClick={() => nav(workId ? `/works/${workId}` : `/works/new?release=${encodeURIComponent(d.id)}&track=${encodeURIComponent(t.id)}`)}>
                    <ReleaseCover id={d.id} src={d.draft?.coverData} />
                    <span className="min-0">
                      <span className="ui-row-title">{t.title || '제목 없는 곡'}{t.version && ` (${t.version})`}</span>
                      <span className="ui-row-meta"><span>{d.title}</span><span>{d.artist}</span>{t.isrc && <span>{t.isrc}</span>}</span>
                    </span>
                    <span className="ui-row-end">
                      {workId ? <Badge tone="green">등록한 곡</Badge> : d.status && <Badge tone={statusTone(d.status)}>{STATUS_LABEL[d.status] ?? d.status}</Badge>}
                      <Glyph name="chevron-right" size={16} />
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </>
      )}
    </WizardShell>
  );
}
