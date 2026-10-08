// AUDENIQ 설문 결과 — survey.audeniq.com 응답을 문항별로 모아 본다 (ADMIN 전용, 읽기 전용).
// Worker가 설문 D1(SURVEY_DB)을 읽어 문항·응답을 함께 준다. 연락처는 동의한 응답에만 있다.
import { useMemo, useState } from 'react';
import { useAsync } from '../../hooks/useAsync';
import { surveyAdmin, type SurveyResults as Results } from '../../api/content';
import { ApiError } from '../../api/errors';
import { MOCK } from '../../lib/mode';
import { tallyAll, tallyBeta, textAnswers, toCsv, withinDays, type QuestionTally } from '../../lib/surveyResults';
import { ago, when } from '../labels';
import { Chip, Empty, ErrorBox, PageHead, Section, Seg, Skeleton } from '../ui';
import { Glyph } from '../../components/Glyph';

type Period = 'all' | '30' | '7';
const PERIODS = [['all', '전체'], ['30', '최근 30일'], ['7', '최근 7일']] as const;

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
            <Seg value={period} options={PERIODS} onChange={setPeriod} label="기간" />
            <span className="small muted">{rows.length}건 집계</span>
          </div>

          <Section title="문항별 응답" meta={`${tallies.length}문항`}>
            <div className="adm-survey-grid">
              {tallies.map((t, i) => <QuestionCard key={t.key} no={`Q${i + 1}`} t={t} />)}
              {beta && <QuestionCard no="베타" t={beta} />}
            </div>
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
