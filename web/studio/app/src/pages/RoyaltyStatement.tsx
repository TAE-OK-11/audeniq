// 분기별 수익 명세서 (/reports/publishing/statement?q=2026-Q3) — 세금·회계용으로 내려받는 한 장.
// 배급 수익(정산 잔액에 들어온 날 기준)과 저작권료(사용 월 기준, 곡별 징수액 · 수수료 · 받을 금액)를 함께 보여 주고 합계를 낸다.
// 저작권료가 정산 잔액에 적립된 줄은 배급 쪽에서 빼서 두 번 세지 않는다.
// 신청서·합의서와 같은 종이 모양(aq-paper) — 인쇄·PDF로 저장한다. 데이터는 음악 리포트 › 저작권료 · 정산 내역과 같은 목록
import { useMemo } from 'react';
import { useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type Royalty } from '../api/publishing';
import * as portal from '../api/portal';
import { useAsync } from '../hooks/useAsync';
import { useProfile } from '../store/profile';
import { useAccount } from '../store/account';
import { revenueTypeOf, statementsStore, type Statement } from '../store/settlement';
import { MOCK } from '../lib/mode';
import { money, localStamp } from '../lib/format';
import { ROYALTY_KIND_LABEL, ROYALTY_STATUS_LABEL, societyName } from '../lib/publishing';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { EmptyState } from '../ui';
import '../styles/split/Publishing.css';

/** '2026-08' → '2026-Q3' */
export const quarterOf = (period: string) => `${period.slice(0, 4)}-Q${Math.ceil(Number(period.slice(5, 7)) / 3)}`;
const quarterLabel = (q: string) => `${q.slice(0, 4)}년 ${q.slice(6)}분기`;

/** 정산 내역 전체 — 체험 모드는 기록한 목록, 실서버는 쪽을 이어 받는다(명세서 한 장에 필요한 만큼, 최대 30쪽) */
async function allStatements(): Promise<Statement[]> {
  if (MOCK) return statementsStore.get();
  const out: Statement[] = [];
  let next: string | undefined;
  for (let i = 0; i < 30; i++) {
    const page = await portal.fetchStatementsPage(next);
    out.push(...page.items);
    if (!page.next) break;
    next = page.next;
  }
  return out;
}

export function RoyaltyStatement() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const profile = useProfile();
  const account = useAccount();
  const { data: rows } = useAsync(() => pubApi.listRoyalties().catch(() => [] as Royalty[]), [], { key: 'royalties:all' });
  const { data: ledger } = useAsync(() => allStatements().catch(() => [] as Statement[]), [], { key: 'statements:all' });
  // 배급 = 정산 잔액에 들어온 줄 중 저작권료 적립이 아닌 것
  const dist = useMemo(() => (ledger ?? []).filter(s => revenueTypeOf(s) === 'distribution' && s.amount !== 0 && /^\d{4}-\d{2}/.test(s.created)), [ledger]);
  const quarters = useMemo(() => Array.from(new Set([
    ...(rows ?? []).map(r => quarterOf(r.period)), ...dist.map(s => quarterOf(s.created)),
  ])).sort().reverse(), [rows, dist]);
  const q = params.get('q') && quarters.includes(params.get('q')!) ? params.get('q')! : quarters[0];
  const shown = (rows ?? []).filter(r => quarterOf(r.period) === q);
  const distShown = dist.filter(s => quarterOf(s.created) === q).sort((a, b) => a.created.localeCompare(b.created));
  const byWork = useMemo(() => {
    const m = new Map<string, { title: string; rows: Royalty[] }>();
    for (const r of shown) { const g = m.get(r.workId) ?? { title: r.workTitle, rows: [] }; g.rows.push(r); m.set(r.workId, g); }
    return [...m.values()].sort((a, b) => a.title.localeCompare(b.title, 'ko'));
  }, [shown]);
  const sum = (k: 'collected' | 'fee' | 'net', list: Royalty[] = shown) => list.reduce((n, r) => n + r[k], 0);
  const distTotal = distShown.reduce((n, s) => n + s.amount, 0);
  const today = localStamp(new Date().toISOString()).replace(/ 오[전후].*$/, '');

  if (!rows || !ledger) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  let no = 0;
  return (
    <div id="view-royalty-statement" className="view aq-doc-view">
      <div className="aq-doc-toolbar">
        <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => nav(-1)}>
          <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />돌아가기
        </button>
        <div className="row-actions">
          {quarters.length > 0 && (
            <select className="ui-select" aria-label="분기" value={q} onChange={e => setParams({ q: e.target.value }, { replace: true })}>
              {quarters.map(x => <option key={x} value={x}>{quarterLabel(x)}</option>)}
            </select>
          )}
          <button type="button" className="ui-btn is-primary" disabled={!shown.length && !distShown.length} onClick={() => window.print()}>인쇄 · PDF 저장</button>
        </div>
      </div>
      {!quarters.length ? (
        <EmptyState title="아직 명세서를 만들 수익이 없어요">배급 수익이 정산되거나 관리 단체가 저작권료를 나눠 주면 분기마다 명세서를 내려받을 수 있어요.</EmptyState>
      ) : (
        <article className="aq-paper" aria-label="수익 명세서">
          <header className="aq-paper-head">
            <div className="aq-paper-brand">
              <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
              <dl className="aq-paper-meta">
                <div><dt>기간</dt><dd>{quarterLabel(q)}</dd></div>
                <div><dt>발행일</dt><dd>{today}</dd></div>
              </dl>
            </div>
            <h1>수익 명세서</h1>
            <p className="aq-paper-en">Revenue Statement · Distribution &amp; Publishing</p>
          </header>

          <section className="aq-paper-sec">
            <h2><span>{++no}</span>받는 분</h2>
            <table className="aq-paper-kv"><tbody>
              <tr><th>이름</th><td>{account.realName || profile.name || '—'}</td><th>활동명</th><td>{profile.name || '—'}</td></tr>
            </tbody></table>
          </section>

          <section className="aq-paper-sec">
            <h2><span>{++no}</span>합계</h2>
            <table className="aq-paper-kv"><tbody>
              <tr><th>배급 수익</th><td>{money(distTotal)}</td><th>저작권료</th><td>{money(sum('net'))}</td></tr>
              <tr><th>합계</th><td colSpan={3}><b>{money(distTotal + sum('net'))}</b>{shown.some(r => r.status === 'estimated') && ' (확정 전 예상 저작권료 포함)'}</td></tr>
            </tbody></table>
          </section>

          {distShown.length > 0 && (
            <section className="aq-paper-sec">
              <h2><span>{++no}</span>배급 수익 <small>{distShown.length}건 · 정산 잔액에 들어온 날 기준</small></h2>
              <div className="aq-paper-scroll">
                <table className="aq-paper-grid aq-statement-grid is-dist">
                  <thead><tr><th>정산일</th><th>내용</th><th>플랫폼 · 기간</th><th>금액</th></tr></thead>
                  <tbody>
                    {distShown.map(s => (
                      <tr key={s.id}>
                        <td className="aq-paper-title">{s.created}</td>
                        <td data-label="내용">{s.note || '정산금'}</td>
                        <td data-label="플랫폼·기간">{[s.platform, s.period].filter(Boolean).join(' · ') || '—'}</td>
                        <td data-label="금액" className="num">{money(s.amount)}</td>
                      </tr>
                    ))}
                    <tr className="aq-statement-sub"><td colSpan={3}>배급 수익 소계</td><td className="num"><b>{money(distTotal)}</b></td></tr>
                  </tbody>
                </table>
              </div>
            </section>
          )}

          {byWork.length > 0 && (
            <section className="aq-paper-sec">
              <h2><span>{++no}</span>곡별 저작권료 <small>{byWork.length}곡 · {shown.length}건 · 사용 월 기준</small></h2>
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
                        <td className="num is-minor">{money(sum('collected', g.rows))}</td><td className="num is-minor">{money(sum('fee', g.rows))}</td><td className="num"><b>{money(sum('net', g.rows))}</b></td><td className="is-minor" />
                      </tr>,
                    ])}
                  </tbody>
                </table>
              </div>
            </section>
          )}

          <footer className="aq-paper-foot">
            <p>배급 수익은 AUDENIQ 수수료를 뺀 뒤 정산 잔액에 들어온 금액이에요. 저작권료는 관리 단체가 나눠 준 사용료에서 계약서의 관리 수수료를 뺀 금액이고, 확정·지급된 금액은 같은 정산 잔액에 쌓여요. 이 명세서는 AUDENIQ STUDIO에 기록된 내역으로 만들었어요.</p>
          </footer>
        </article>
      )}
    </div>
  );
}
