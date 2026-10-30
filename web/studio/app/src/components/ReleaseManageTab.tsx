// 발매 상세 '관리' 탭 — 발매 뒤에 하는 일을 성격별로 묶는다 (카드 8장이 한 줄로 쌓이면 무엇이 어디 있는지 안 보인다).
// 탭을 열 때 받는다: 처음 여는 '정보' 탭에는 필요 없는 카드들이라 발매 상세 화면을 가볍게 연다.
import type { ReleaseDetail } from '../api/types';
import { kindLabel } from '../lib/catalog';
import { ContentIdCard } from './ContentIdCard';
import { CorrectionCard } from './CorrectionCard';
import { InfringementCard } from './InfringementCard';
import { PitchCard } from './PitchCard';
import { PreReleaseCard } from './PreReleaseCard';
import { ReleaseAddons } from './ReleaseAddons';
import { SmartLinkCard } from './SmartLinkCard';
import { SplitsCard } from './SplitsCard';
import { ReleasePublishingCard } from './publishing/ReleasePublishing';
import { TakedownCard } from './TakedownCard';
import { TermCard } from './TermCard';

export function ReleaseManageTab({ rel, cover }: { rel: ReleaseDetail; cover?: string }) {
  const d = rel.draft;
  const rejected = rel.status === 'rejected';
  return (
    <div className="aq-detail-stack">
      {rel.status === 'draft' ? <SplitsCard releaseId={rel.id} /> : (
        <>
          <section className="aq-manage-group" aria-label="수익·홍보">
            <h3 className="aq-manage-group-title">수익·홍보</h3>
            <SplitsCard releaseId={rel.id} />
            {/* 이 앨범 곡의 작사·작곡 저작권 — 선택. 누르면 앨범 퍼블리싱 설정 */}
            <ReleasePublishingCard releaseId={rel.id} trackCount={rel.tracks.length} />
            <PreReleaseCard releaseId={rel.id} preorderStart={d?.preorder_start} releaseDate={rel.release_date} releaseTime={d?.release_time} />
            <SmartLinkCard releaseId={rel.id} promo={{
              title: rel.title, artist: rel.artist ?? '', releaseDate: rel.release_date ?? null, cover,
              releaseTime: d?.release_time || null, kind: kindLabel(d?.type) || undefined,
              lyrics: (d?.draftTracks ?? []).filter(t => t.lyrics?.trim()).map(t => ({ title: t.title, lyrics: t.lyrics })),
            }} />
            <PitchCard releaseId={rel.id} />
          </section>
          <section className="aq-manage-group" aria-label="권리 보호">
            <h3 className="aq-manage-group-title">권리 보호</h3>
            <ContentIdCard releaseId={rel.id} />
            <InfringementCard releaseId={rel.id} />
          </section>
          <ReleaseAddons releaseId={rel.id} editable={!rejected && rel.status !== 'cancelled'} />
          <section className="aq-manage-group" aria-label="서비스·계약">
            <h3 className="aq-manage-group-title">서비스·계약</h3>
            <CorrectionCard releaseId={rel.id} />
            <TakedownCard releaseId={rel.id} />
            {!rejected && rel.status !== 'cancelled' && rel.status !== 'closed' && <TermCard releaseId={rel.id} />}
          </section>
        </>
      )}
    </div>
  );
}
