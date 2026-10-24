// 사전 공개 카드 (발매 상세 '관리') — 신청서의 '사전 공개 시작일'로 플랫폼에 발매 예정 페이지가 열리는 일정과,
// 그동안 팬이 플랫폼에서 하는 일(미리 저장·미리 추가·예약 구매). 정하지 않았으면 수정 요청으로 부탁할 수 있다.
import { Link } from '../lib/router';
import { ManageHead } from './ManageHead';
import { BrandMark } from './BrandMark';
import { Glyph } from './Glyph';
import { formatKoreanDate } from '../lib/date';
import { PRE_RELEASE_PLATFORMS, preReleaseState } from '../lib/preRelease';

export function PreReleaseCard({ releaseId, preorderStart, releaseDate, releaseTime }: {
  releaseId: string; preorderStart?: string | null; releaseDate?: string | null; releaseTime?: string | null;
}) {
  const st = preReleaseState(preorderStart, releaseDate);
  // 발매가 지났거나 날짜가 없으면 할 일이 없다
  if (st.phase === 'released' || !releaseDate) return null;
  const status = st.phase === 'open' ? { tone: 'green' as const, label: '공개 중' }
    : st.phase === 'scheduled' ? { tone: 'blue' as const, label: `${st.daysToOpen}일 뒤 열림` }
    : { tone: 'gray' as const, label: '정하지 않음' };

  return (
    <section className="ui-card aq-manage-card aq-prerelease" aria-labelledby="preReleaseHeading">
      <ManageHead id="preReleaseHeading" icon="calendar" title="플랫폼 사전 공개" status={status} />
      {st.phase === 'none' ? (
        <>
          <p className="ui-field-help">발매 전에 Spotify·Apple Music 같은 플랫폼에 발매 예정 페이지를 열어 두면, 팬이 그 앱에서 미리 저장하고 발매되는 순간 바로 들을 수 있어요. 이 발매는 사전 공개 시작일을 정하지 않았어요.</p>
          <Link className="ui-btn is-sm" to={`/inquiries?new=edit&release=${encodeURIComponent(releaseId)}`}>
            사전 공개 요청하기<Glyph name="chevron-right" size={13} />
          </Link>
          <p className="small muted">플랫폼 전달 일정에 따라 반영되지 않을 수 있어요. 다음 발매부터는 신청할 때 ‘사전 공개 시작일’을 골라 주세요.</p>
        </>
      ) : (
        <>
          <ol className="aq-prerelease-steps">
            <li className={st.phase === 'open' ? 'is-done' : 'is-now'}>
              <strong>{formatKoreanDate(preorderStart!)}</strong>
              <span>사전 공개 시작{st.phase === 'scheduled' ? ` · D-${st.daysToOpen}` : ''}</span>
            </li>
            <li className={st.phase === 'open' ? 'is-now' : undefined}>
              <strong>{formatKoreanDate(releaseDate)}{releaseTime ? ` ${releaseTime}` : ''}</strong>
              <span>발매 · D-{st.daysToRelease}</span>
            </li>
          </ol>
          <ul className="ui-list is-inset aq-prerelease-platforms">
            {PRE_RELEASE_PLATFORMS.map(p => (
              <li key={p.id} className="aq-prerelease-platform">
                <BrandMark id={p.id} label={p.name} size={28} />
                <span className="min-0"><strong>{p.name}</strong><small>{p.what}</small></span>
              </li>
            ))}
          </ul>
          <p className="small muted">
            {st.phase === 'open'
              ? '플랫폼의 발매 예정 페이지 링크를 스마트링크에 넣으면, 팬이 그 앱에서 바로 미리 저장할 수 있어요.'
              : '열리면 플랫폼의 발매 예정 페이지 링크를 스마트링크에 넣어 주세요. 표시 시점은 플랫폼마다 조금 달라요.'}
          </p>
        </>
      )}
    </section>
  );
}
