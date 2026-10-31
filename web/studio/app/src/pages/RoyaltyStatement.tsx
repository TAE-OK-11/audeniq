// 분기별 저작권료 명세서 (/reports/publishing/statement?q=2026-Q3) — 세금·회계용으로 내려받는 한 장.
// 그 분기(사용 월 기준)의 저작권료를 곡별로 묶어 징수액 · 수수료 · 받을 금액과 합계를 보여 준다.
// 신청서·합의서와 같은 종이 모양(aq-paper) — 인쇄·PDF로 저장한다. 데이터는 음악 리포트 › 저작권료와 같은 목록
import { useMemo } from 'react';
import { useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type Royalty } from '../api/publishing';
import { useAsync } from '../hooks/useAsync';
import { useProfile } from '../store/profile';
import { useAccount } from '../store/account';
import { money, localStamp } from '../lib/format';
import { ROYALTY_KIND_LABEL, ROYALTY_STATUS_LABEL, societyName } from '../lib/publishing';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { EmptyState } from '../ui';
import '../styles/split/Publishing.css';

/** '2026-08' → '2026-Q3' */
export const quarterOf = (period: string) => `${period.slice(0, 4)}-Q${Math.ceil(Number(period.slice(5, 7)) / 3)}`;
const quarterLabel = (q: string) => `${q.slice(0, 4)}년 ${q.slice(6)}분기`;

export function RoyaltyStatement() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const profile = useProfile();
  const account = useAccount();
  const { data: rows } = useAsync(() => pubApi.listRoyalties(), [], { key: 'royalties:all' });
  const quarters = useMemo(() => Array.from(new Set((rows ?? []).map(r => quarterOf(r.period)))).sort().reverse(), [rows]);
  const q = params.get('q') && quarters.includes(params.get('q')!) ? params.get('q')! : quarters[0];
  const shown = (rows ?? []).filter(r => quarterOf(r.period) === q);
  const byWork = useMemo(() => {
    const m = new Map<string, { title: string; rows: Royalty[] }>();
    for (const r of shown) { const g = m.get(r.workId) ?? { title: r.workTitle, rows: [] }; g.rows.push(r); m.set(r.workId, g); }
    return [...m.values()].sort((a, b) => a.title.localeCompare(b.title, 'ko'));
  }, [shown]);
  const sum = (k: 'collected' | 'fee' | 'net', list: Royalty[] = shown) => list.reduce((n, r) => n + r[k], 0);
  const today = localStamp(new Date().toISOString()).replace(/ 오[전후].*$/, '');

  if (!rows) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  return (
    <div id="view-royalty-statement" className="view aq-doc-view">
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav('/reports?type=publishing')}>
          <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />음악 리포트
        </button>
        <div className="row-actions">
          {quarters.length > 0 && (
            <select className="ui-select" aria-label="분기" value={q} onChange={e => setParams({ q: e.target.value }, { replace: true })}>
              {quarters.map(x => <option key={x} value={x}>{quarterLabel(x)}</option>)}
            </select>
          )}
          <button type="button" className="ui-btn is-primary" disabled={!shown.length} onClick={() => window.print()}>인쇄 · PDF 저장</button>
        </div>
      </div>
      {!quarters.length ? (
        <EmptyState title="아직 명세서를 만들 저작권료가 없어요">관리 단체가 사용료를 나눠 주면 분기마다 명세서를 내려받을 수 있어요.</EmptyState>
      ) : (
        <article className="aq-paper" aria-label="저작권료 명세서">
          <header className="aq-paper-head">
            <div className="aq-paper-brand">
              <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
              <dl className="aq-paper-meta">
                <div><dt>기간</dt><dd>{quarterLabel(q)} (사용 월 기준)</dd></div>
                <div><dt>발행일</dt><dd>{today}</dd></div>
              </dl>
            </div>
            <h1>저작권료 명세서</h1>
            <p className="aq-paper-en">Publishing Royalty Statement</p>
          </header>

          <section className="aq-paper-sec">
            <h2><span>1</span>받는 분</h2>
            <table className="aq-paper-kv"><tbody>
              <tr><th>이름</th><td>{account.realName || profile.name || '—'}</td><th>활동명</th><td>{profile.name || '—'}</td></tr>
            </tbody></table>
          </section>

          <section className="aq-paper-sec">
            <h2><span>2</span>합계</h2>
            <table className="aq-paper-kv"><tbody>
              <tr><th>징수액</th><td>{money(sum('collected'))}</td><th>관리 수수료</th><td>{money(sum('fee'))}</td></tr>
              <tr><th>받을 금액</th><td colSpan={3}><b>{money(sum('net'))}</b>{shown.some(r => r.status === 'estimated') && ' (확정 전 예상 금액 포함)'}</td></tr>
            </tbody></table>
          </section>

          <section className="aq-paper-sec">
            <h2><span>3</span>곡별 내역 <small>{byWork.length}곡 · {shown.length}건</small></h2>
            <div className="aq-paper-scroll">
              <table className="aq-paper-grid aq-statement-grid">
                <thead><tr><th>곡 · 종류</th><th>단체 · 국가 · 월</th><th>징수액</th><th>수수료</th><th>받을 금액</th><th>상태</th></tr></thead>
                <tbody>
                  {byWork.flatMap(g => [
                    ...g.rows.map((r, i) => (
                      <tr key={r.id}>
                        <td className="aq-paper-title">{i === 0 && <strong>{g.title}<br /></strong>}{ROYALTY_KIND_LABEL[r.kind] ?? r.kind}</td>
                        <td data-label="단체·국가·월">{societyName(r.source)} · {r.country} · {r.period}</td>
                        <td data-label="징수액" className="num">{money(r.collected)}</td>
                        <td data-label="수수료" className="num">{money(r.fee)}</td>
                        <td data-label="받을 금액" className="num">{money(r.net)}</td>
                        <td data-label="상태">{ROYALTY_STATUS_LABEL[r.status]}</td>
                      </tr>
                    )),
                    <tr key={`${g.title}-sum`} className="aq-statement-sub">
                      <td colSpan={2}>‘{g.title}’ 소계</td>
                      <td className="num">{money(sum('collected', g.rows))}</td><td className="num">{money(sum('fee', g.rows))}</td><td className="num"><b>{money(sum('net', g.rows))}</b></td><td />
                    </tr>,
                  ])}
                </tbody>
              </table>
            </div>
          </section>

          <footer className="aq-paper-foot">
            <p>관리 단체가 나눠 준 사용료에서 계약서의 관리 수수료를 뺀 금액이에요. 확정·지급된 금액은 배급 수익과 같은 정산 잔액에 쌓여요. 이 명세서는 AUDENIQ STUDIO에 기록된 내역으로 만들었어요.</p>
          </footer>
        </article>
      )}
    </div>
  );
}
