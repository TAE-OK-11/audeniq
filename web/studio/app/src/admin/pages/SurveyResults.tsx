// AUDENIQ 설문 결과 — survey.audeniq.com 응답을 문항별로 모아 본다 (ADMIN 전용, 읽기 전용).
// Worker가 설문 D1(SURVEY_DB)을 읽어 문항·응답을 함께 준다. 연락처는 동의한 응답에만 있다.
import { useMemo, useState } from 'react';
import { useAsync } from '../../hooks/useAsync';
import { surveyAdmin, type SurveyResults as Results } from '../../api/content';
import { ApiError } from '../../api/errors';
import { MOCK } from '../../lib/mode';
import { answerLabels, tallyAll, tallyBeta, textAnswers, toCsv, withinDays, type QuestionTally } from '../../lib/surveyResults';
import { useSearchParams } from '../../lib/router';
import type { SurveyResponseRow } from '../../api/content';
import { ago, when } from '../labels';
import { Chip, Empty, ErrorBox, PageHead, Section, Seg, Skeleton } from '../ui';
import { Glyph } from '../../components/Glyph';

type Period = 'all' | '30' | '7';
const PERIODS = [['all', '전체'], ['30', '최근 30일'], ['7', '최근 7일']] as const;
type View = 'chart' | 'table' | 'responses';
const VIEWS = [['chart', '그래프'], ['table', '숫자표'], ['responses', '응답 하나씩']] as const;

async function load(): Promise<Results> {
  try {
    return await surveyAdmin.results();
  } catch (e) {
    // 체험 모드(Worker 없음)에서만 예시 결과
    if (MOCK && e instanceof ApiError && [0, 404, 405, 502, 503, 504].includes(e.status)) return demo();
    if (e instanceof ApiError && e.code === 'SURVEY_UNAVAILABLE') {
      throw new ApiError('설문 DB가 아직 STUDIO에 연결되지 않았어요. web/studio/wrangler.jsonc에 SURVEY_DB를 추가해 주세요.', 503, e.code);
    }
    throw e;
  }
}

export function SurveyResults() {
  const { data, loading, error, reload } = useAsync(load, []);
  const [period, setPeriod] = useState<Period>('all');
  const [params, setParams] = useSearchParams();
  const view: View = (['chart', 'table', 'responses'] as const).find(v => v === params.get('view')) ?? 'chart';
  const openId = params.get('id');
  const setView = (v: View) => setParams(v === 'chart' ? {} : { view: v }, { replace: true });
  const openResponse = (id: string | null) => setParams(id ? { view: 'responses', id } : { view: 'responses' });
  const rows = useMemo(() => (data ? withinDays(data.items, period === 'all' ? null : Number(period), data.now) : []), [data, period]);
  const tallies = useMemo(() => (data ? tallyAll(data, rows) : []), [data, rows]);
  const beta = useMemo(() => (data ? tallyBeta(data, rows) : null), [data, rows]);
  const week = data ? withinDays(data.items, 7, data.now).length : 0;
  const contacts = rows.filter(r => r.contact);
  const wantsBeta = beta ? beta.options[0].count + beta.options[1].count : 0;

  const download = () => {
    if (!data) return;
    const url = URL.createObjectURL(new Blob([toCsv(data, rows)], { type: 'text/csv;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = `audeniq-survey-${data.now.slice(0, 10)}.csv`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="설문"
        title="설문 결과"
        sub="survey.audeniq.com 사전 수요조사 응답이에요. 연락처는 수집에 동의한 응답에만 있어요."
        actions={<>
          <button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>
          <button type="button" className="adm-btn primary small" onClick={download} disabled={!rows.length}><Glyph name="download" size={14} /> CSV 내려받기</button>
        </>}
      />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={4} /> : !data ? null : data.items.length === 0 ? (
        <Empty icon={<Glyph name="chart" size={22} />} title="아직 응답이 없어요">설문에 응답이 들어오면 여기에 모여요.</Empty>
      ) : (
        <>
          <div className="adm-stats">
            <Stat label="전체 응답" value={data.total} hint={data.total > data.items.length ? `최근 ${data.items.length}건만 집계` : undefined} />
            <Stat label="최근 7일" value={week} />
            <Stat label="베타 참여 희망" value={wantsBeta} hint={period === 'all' ? undefined : '선택한 기간'} />
            <Stat label="연락처 남김" value={contacts.length} hint={period === 'all' ? undefined : '선택한 기간'} />
          </div>
          <div className="adm-survey-bar">
            <Seg value={view} options={VIEWS} onChange={setView} label="보기 방식" />
            <span className="adm-survey-period">
              <Seg value={period} options={PERIODS} onChange={setPeriod} label="기간" />
              <span className="small muted">{rows.length}건</span>
            </span>
          </div>
          {view === 'responses' ? (
            openId ? (
              <ResponseDetail data={data} rows={rows} id={openId} onOpen={openResponse} />
            ) : (
              <ResponseList data={data} rows={rows} onOpen={openResponse} />
            )
          ) : (
            <>
            <Section title="문항별 응답" meta={`${tallies.length}문항`}>
              {view === 'table' ? (
                <NumberTable tallies={[...tallies.map((t, i) => ({ no: `Q${i + 1}`, t })), ...(beta ? [{ no: '베타', t: beta }] : [])]} />
              ) : (
                <div className="adm-survey-grid">
                  {tallies.map((t, i) => <QuestionCard key={t.key} no={`Q${i + 1}`} t={t} />)}
                  {beta && <QuestionCard no="베타" t={beta} />}
                </div>
              )}
            </Section>

            {data.text.map(t => {
              const list = textAnswers(rows, t.key);
              return (
                <Section key={t.key} title={t.title} meta={`${list.length}건`}>
                  {list.length === 0 ? <p className="small muted">아직 적힌 내용이 없어요.</p> : (
                    <ul className="adm-survey-texts">
                      {list.map(a => <li key={a.id}><p>{a.text}</p><span className="small muted" title={when(a.created_at)}>{ago(a.created_at)}</span></li>)}
                    </ul>
                  )}
                </Section>
              );
            })}

            <Section title="베타·출시 안내 연락처" meta={`${contacts.length}명`}>
              {contacts.length === 0 ? <p className="small muted">연락처를 남긴 응답이 없어요.</p> : (
                <div className="adm-card white adm-table-wrap">
                  <table className="adm-table">
                    <thead><tr><th>연락처</th><th>참여 의향</th><th>제출</th></tr></thead>
                    <tbody>
                      {contacts.map(r => (
                        <tr key={r.id}>
                          <td><b>{r.contact}</b></td>
                          <td>{data.beta.options[Number(r.beta)] ?? '—'}</td>
                          <td className="small" title={when(r.created_at)}>{ago(r.created_at)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Section>
            </>
          )}
        </>
      )}
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: number; hint?: string }) {
  return (
    <div className="adm-stat">
      <span className="adm-stat-top"><span className="adm-stat-icon"><Glyph name="chart" size={18} /></span></span>
      <span>
        <strong>{value.toLocaleString('ko-KR')}</strong>
        <span className="adm-stat-label">{label}</span>
        {hint && <span className="adm-stat-hint small muted">{hint}</span>}
      </span>
    </div>
  );
}

function QuestionCard({ no, t }: { no: string; t: QuestionTally }) {
  const max = Math.max(1, ...t.options.map(o => o.count));
  return (
    <article className="adm-card adm-survey-q">
      <div className="adm-survey-q-top">
        <span className="adm-survey-no">{no}</span>
        <Chip tone={t.type === 'multi' ? 'violet' : 'blue'}>{t.type === 'multi' ? '여러 개 선택' : '하나 선택'}</Chip>
        <span className="small muted">{t.answered}명 응답</span>
      </div>
      <h3>{t.title}</h3>
      <ul className="adm-survey-opts">
        {t.options.map(o => (
          <li key={o.label} className={o.count === max && o.count > 0 ? 'is-top' : ''}>
            <div className="adm-survey-opt-row"><span>{o.label}</span><b>{o.count}<small> · {Math.round(o.share * 100)}%</small></b></div>
            <span className="adm-survey-track" aria-hidden="true"><span style={{ width: `${(o.count / max) * 100}%` }} /></span>
          </li>
        ))}
      </ul>
      {t.others.length > 0 && (
        <details className="adm-survey-others">
          <summary>기타 응답 {t.others.length}건</summary>
          <ul>{t.others.map((o, i) => <li key={i}>{o}</li>)}</ul>
        </details>
      )}
    </article>
  );
}

/** 숫자표 — 문항·선택지·인원·비율을 한 표로 */
function NumberTable({ tallies }: { tallies: { no: string; t: QuestionTally }[] }) {
  return (
    <div className="adm-card white adm-table-wrap">
      <table className="adm-table adm-survey-table">
        <thead><tr><th>문항</th><th>선택지</th><th className="num">인원</th><th className="num">비율</th></tr></thead>
        {tallies.map(({ no, t }) => (
          <tbody key={t.key}>
            {t.options.map((o, i) => (
              <tr key={o.label} className={i === 0 ? 'is-first' : ''}>
                {i === 0 && (
                  <th scope="rowgroup" rowSpan={t.options.length} className="adm-survey-table-q">
                    <span className="adm-survey-no">{no}</span>
                    <b>{t.title}</b>
                    <span className="small muted">{t.answered}명 응답 · {t.type === 'multi' ? '여러 개 선택' : '하나 선택'}</span>
                  </th>
                )}
                <td>{o.label}</td>
                <td className="num"><b>{o.count}</b></td>
                <td className="num">{(o.share * 100).toFixed(1)}%</td>
              </tr>
            ))}
          </tbody>
        ))}
      </table>
    </div>
  );
}

/** 응답 목록 — 한 줄에 한 사람, 누르면 전체 답을 본다 */
function ResponseList({ data, rows, onOpen }: { data: Results; rows: SurveyResponseRow[]; onOpen: (id: string) => void }) {
  const q1 = data.questions[0];
  const q2 = data.questions[1];
  if (!rows.length) return <Empty title="이 기간에는 응답이 없어요" />;
  return (
    <div className="adm-card white adm-table-wrap">
      <table className="adm-table adm-survey-list">
        <thead><tr><th>#</th><th>제출</th>{q1 && <th>활동 형태</th>}{q2 && <th>발매 경험</th>}<th>베타</th><th>연락처</th><th aria-label="열기" /></tr></thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={r.id} onClick={() => onOpen(r.id)} className="is-click">
              <td className="small muted">{rows.length - i}</td>
              <td className="small" title={when(r.created_at)}>{when(r.created_at)}</td>
              {q1 && <td>{answerLabels(q1, r).join(', ') || '—'}</td>}
              {q2 && <td>{answerLabels(q2, r).join(', ') || '—'}</td>}
              <td>{data.beta.options[Number(r.beta)] ?? '—'}</td>
              <td>{r.contact ? <Chip tone="green">있음</Chip> : <span className="muted">—</span>}</td>
              <td><button type="button" className="adm-btn soft small" onClick={e => { e.stopPropagation(); onOpen(r.id); }}>보기</button></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** 응답 1건 — 모든 문항과 고른 답, 이전·다음 응답으로 이동 */
function ResponseDetail({ data, rows, id, onOpen }: { data: Results; rows: SurveyResponseRow[]; id: string; onOpen: (id: string | null) => void }) {
  const index = rows.findIndex(r => r.id === id);
  const r = index >= 0 ? rows[index] : data.items.find(x => x.id === id);
  if (!r) return <Empty title="응답을 찾을 수 없어요"><button type="button" className="adm-btn soft small" onClick={() => onOpen(null)}>목록으로</button></Empty>;
  const prev = index > 0 ? rows[index - 1] : null;
  const next = index >= 0 && index < rows.length - 1 ? rows[index + 1] : null;
  return (
    <div className="adm-survey-detail">
      <div className="adm-survey-detail-nav">
        <button type="button" className="adm-btn soft small" onClick={() => onOpen(null)}>← 목록</button>
        <span className="small muted">{index >= 0 ? `${rows.length - index} / ${rows.length}` : ''}</span>
        <span className="adm-survey-detail-step">
          <button type="button" className="adm-btn soft small" disabled={!prev} onClick={() => prev && onOpen(prev.id)}>더 최근</button>
          <button type="button" className="adm-btn soft small" disabled={!next} onClick={() => next && onOpen(next.id)}>이전 응답</button>
        </span>
      </div>
      <div className="adm-card white">
        <dl className="adm-survey-meta">
          <div><dt>제출</dt><dd>{when(r.created_at)} <span className="small muted">({ago(r.created_at)})</span></dd></div>
          <div><dt>응답 ID</dt><dd><span className="adm-code">{r.id}</span></dd></div>
          <div><dt>베타 참여</dt><dd>{data.beta.options[Number(r.beta)] ?? '—'}</dd></div>
          <div><dt>연락처</dt><dd>{r.contact ? <b>{r.contact}</b> : <span className="muted">남기지 않음</span>}</dd></div>
        </dl>
      </div>
      <ol className="adm-survey-answers">
        {data.questions.map((q, i) => {
          const labels = answerLabels(q, r);
          return (
            <li key={q.key} className={labels.length ? '' : 'is-empty'}>
              <span className="adm-survey-no">Q{i + 1}</span>
              <div>
                <p className="adm-survey-answers-q">{q.title}</p>
                {labels.length
                  ? <div className="adm-survey-picked">{labels.map(l => <span key={l}>{l}</span>)}</div>
                  : <span className="small muted">응답 없음 (건너뜀)</span>}
              </div>
            </li>
          );
        })}
        {data.text.map(t => {
          const v = r.answers[t.key];
          const text = typeof v === 'string' ? v.trim() : '';
          return (
            <li key={t.key} className={text ? '' : 'is-empty'}>
              <span className="adm-survey-no">글</span>
              <div>
                <p className="adm-survey-answers-q">{t.title}</p>
                {text ? <p className="adm-survey-free">{text}</p> : <span className="small muted">적지 않음</span>}
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** 체험 모드 예시 — 실제 설문 문항 일부 */
function demo(): Results {
  const now = new Date().toISOString();
  const day = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString();
  return {
    now, total: 3,
    questions: [
      { key: 'q1', title: '현재 어떤 형태로 음악 활동을 하고 계신가요?', type: 'multi', options: ['솔로 아티스트', '싱어송라이터', '래퍼 / 힙합 아티스트', '밴드', '작곡가 / 프로듀서', '기타'] },
      { key: 'q2', title: '정식으로 음원을 발매한 경험이 있나요?', type: 'single', options: ['현재도 지속적으로 발매하고 있다', '과거에 발매한 경험이 있다', '첫 음원 발매를 준비하고 있다', '아직 발매 경험은 없지만 향후 계획이 있다', '현재 발매 계획은 없다'] },
    ],
    text: [{ key: 'q14', title: 'AUDENIQ에 바라는 점 · 개선되었으면 하는 점' }],
    beta: { title: '비공개 베타 참여 의향', type: 'single', options: ['적극적으로 참여하고 싶다', '자세한 조건을 확인한 후 참여하고 싶다', '서비스 출시 소식만 받아보고 싶다', '참여 의향 없음'] },
    items: [
      { id: 'demo-1', version: 4, created_at: day(1), answers: { q1: ['0', '1'], q2: '0', q14: '정산 내역을 더 자주 볼 수 있으면 좋겠어요.' }, beta: '0', contact: '@demo_artist', contact_consent: true },
      { id: 'demo-2', version: 4, created_at: day(3), answers: { q1: ['4', '5'], q1_other: '세션 연주자', q2: '2', q14: '' }, beta: '1', contact: null, contact_consent: false },
      { id: 'demo-3', version: 4, created_at: day(20), answers: { q1: ['3'], q2: '1', q14: '' }, beta: '2', contact: null, contact_consent: false },
    ],
  };
}
