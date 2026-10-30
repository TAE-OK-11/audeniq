// 홈의 퍼블리싱 — 쓰는 사람에게는 작품 현황과 배급·퍼블리싱 수익을 나눠 보여 주고,
// 아직 안 쓰는 사람에게는 작품이 없는 발매 곡 하나로 '이 곡의 저작권도 관리할까요?'만 (기획안 3.1, 3.2).
// 모양은 홈의 다른 카드 그대로: 수익은 '정산' 카드(큰 금액 + Meter + 항목·값), 제안은 '다가오는 발매' 카드(곡 한 줄 + 버튼).
// 제안은 닫으면 다시 나오지 않고, 닫아도 배급 기능은 그대로다
import { useState } from 'react';
import { Link } from '../../lib/router';
import type { PublishingSummary } from '../../api/publishing';
import { readJSON, writeJSON } from '../../lib/storage';
import { money } from '../../lib/format';
import { Money } from '../Money';
import { Glyph } from '../Glyph';
import { IconTile, KeyValue, Meter } from '../../ui';
import '../../styles/split/Dashboard.css';

const DISMISS_KEY = 'publishing-suggest-dismissed';

export function PublishingSuggest({ s }: { s: NonNullable<PublishingSummary['suggestion']> }) {
  const [hidden, setHidden] = useState(() => readJSON<string[]>(DISMISS_KEY, []).includes(s.trackId));
  if (hidden) return null;
  const close = () => { writeJSON(DISMISS_KEY, [...readJSON<string[]>(DISMISS_KEY, []), s.trackId]); setHidden(true); };
  const to = `/works/new?release=${encodeURIComponent(s.releaseId)}&track=${encodeURIComponent(s.trackId)}`;
  return (
    <section className="ui-card aq-home-upcoming" aria-labelledby="pubSuggestTitle">
      <div className="ui-section-head">
        <h2 id="pubSuggestTitle">이 곡의 저작권도 관리할까요?</h2>
        <button type="button" className="aq-head-link" onClick={close}>닫기</button>
      </div>
      <Link to={to} className="aq-upcoming-link aq-row-link">
        <IconTile><Glyph name="music" size={20} /></IconTile>
        <div className="min-0">
          <span className="ui-row-title">{s.title}</span>
          <span className="ui-row-meta">{s.releaseTitle} · 작사·작곡 지분 정리 전</span>
        </div>
        <Glyph name="chevron-right" size={16} />
      </Link>
      <p className="ui-row-meta">지분을 정리하면 방송·스트리밍 저작권료까지 받을 수 있어요. 안 써도 배급은 그대로예요.</p>
      <Link to={to} className="ui-btn is-white is-block">작품 관리 시작</Link>
    </section>
  );
}

export function PublishingHomeCard({ s }: { s: PublishingSummary }) {
  const { works, revenue } = s;
  const dist = revenue.distribution.confirmed;
  const pub = revenue.publishing.confirmed;
  return (
    <section className="ui-card aq-home-money" aria-labelledby="pubHomeTitle">
      <div className="ui-section-head">
        <h2 id="pubHomeTitle">저작물</h2>
        <Link to="/releases?tab=works" className="aq-head-link">작품 보기<Glyph name="chevron-right" size={14} /></Link>
      </div>
      <div className="aq-report-total">
        <small>확정 수익 (배급 + 퍼블리싱)</small>
        <strong><Money value={dist + pub} animate /></strong>
        {revenue.publishing.estimated > 0 && <p className="ui-row-meta">확정 전인 퍼블리싱 예상 {money(revenue.publishing.estimated)}은 넣지 않았어요.</p>}
      </div>
      <Meter total={dist + pub} parts={[{ value: dist }, { value: pub, tone: 'soft' }]} label={`배급 ${dist}원, 퍼블리싱 ${pub}원`} />
      <KeyValue cols={2} items={[
        ['배급', <Money value={dist} />], ['퍼블리싱', <Money value={pub} />],
        ['전체 작품', `${works.total}개`], ['관리 중 · 등록 진행', `${works.managed}개 · ${works.inProgress}개`],
      ]} />
      <Link to="/settlement?type=publishing" className="ui-btn is-white is-block">퍼블리싱 정산 보기</Link>
    </section>
  );
}
