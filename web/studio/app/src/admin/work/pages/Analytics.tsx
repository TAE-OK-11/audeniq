// 업무 분석 — 감사 기록(audit) 기반 업무량 분석. 하루 처리량 · 사람별 처리량 · 평균 처리 시간 · 단계별 체류 시간 ·
// 반려율 · 자동 처리율 · 병목 단계 · DSP별 실패율.
import { useState } from 'react';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Empty, ErrorBox, PageHead, Section, Seg, Skeleton, useStaff } from '../../ui';
import { workApi } from '../api';
import { TASK_TYPE } from '../labels';
import { BarList, DailyChart } from '../charts';
import { MiniStats, Person, usePeople } from '../ui';

const mins = (m: number) => (m < 60 ? `${Math.round(m)}분` : m < 1440 ? `${(m / 60).toFixed(m < 600 ? 1 : 0)}시간` : `${(m / 1440).toFixed(1)}일`);
const hours = (h: number) => (h < 1 ? `${Math.round(h * 60)}분` : h < 48 ? `${h.toFixed(h < 10 ? 1 : 0)}시간` : `${(h / 24).toFixed(1)}일`);
const pct = (v: number) => `${(v * 100).toFixed(v < 0.1 ? 1 : 0)}%`;

export function Analytics() {
  const [days, setDays] = useState<'7' | '30' | '90'>('30');
  const { me } = useStaff();
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.analytics(Number(days)), [days]);

  const total = data ? data.daily.reduce((s, d) => s + d.manual + d.auto, 0) : 0;
  // 병목 — 평균 체류 시간 × 지금 쌓인 건수가 가장 큰 단계
  const bottleneck = data ? [...data.stages].sort((a, b) => b.avg_hours * b.wip - a.avg_hours * a.wip)[0] : null;

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="운영"
        title="업무 분석"
        sub="감사 기록으로 처리량과 처리 시간을 계산해요. 어디서 일이 막히는지 보고 사람과 자동화를 배치하세요."
        actions={<>
          <Seg label="기간" value={days} onChange={setDays} options={[['7', '7일'], ['30', '30일'], ['90', '90일']] as const} />
          <button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>
        </>}
      />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={5} /> : !data ? <Empty icon={<Glyph name="chart" size={22} />} title="분석할 기록이 없어요" /> : (
        <>
          <MiniStats items={[
            { label: '하루 평균 처리', value: `${Math.round(total / data.days)}건`, hint: `${data.days}일 ${total.toLocaleString('ko-KR')}건` },
            { label: '평균 처리 시간', value: mins(data.avg_cycle_minutes), hint: '배정 → 완료' },
            { label: '자동 처리율', value: pct(data.auto_rate), hint: '사람 손을 거치지 않은 비율' },
            { label: '반려율', value: pct(data.reject_rate), tone: data.reject_rate > 0.12 ? 'warn' : undefined, hint: '심사 거절·보완 요청' },
          ]} />

          <Section title="하루 처리량" meta={`SLA 준수율 ${pct(data.sla_met_rate)}`}>
            <div className="adm-card"><DailyChart rows={data.daily} /></div>
          </Section>

          <div className="adm-w-grid2 adm-w-analytics">
            <Section title="단계별 체류 시간" meta="평균 · 지금 쌓인 건수">
              <div className="adm-card">
                {bottleneck && <div className="adm-alert adm-w-alert-warn"><b>병목: {bottleneck.label}</b> — 평균 {hours(bottleneck.avg_hours)}, 지금 {bottleneck.wip}건이 기다리고 있어요.</div>}
                <BarList
                  items={data.stages.map(s => ({ key: s.key, label: <>{s.label}<small className="adm-w-bars-sub">{s.wip}건 대기 · 상위 10% {hours(s.p90_hours)}</small></>, value: s.avg_hours, bad: s.key === bottleneck?.key }))}
                  format={hours}
                />
              </div>
            </Section>
            <Section title="DSP별 실패율" meta="최근 30일 전송 기준">
              <div className="adm-card">
                <BarList items={data.dsp_failure.map(d => ({ key: d.code, label: <>{d.name}<small className="adm-w-bars-sub">실패 {d.failures}건</small></>, value: d.rate * 100, bad: d.rate >= 0.05 }))} format={v => `${v.toFixed(1)}%`} />
              </div>
            </Section>
          </div>

          <Section title="사람별 처리량" meta={`${data.days}일`}>
            <div className="adm-card">
              <div className="adm-table-wrap">
                <table className="adm-table adm-w-people">
                  <thead><tr><th>담당자</th><th>완료</th><th aria-hidden="true" /><th>평균 처리 시간</th><th>진행 중</th><th>기한 초과</th></tr></thead>
                  <tbody>
                    {[...data.people].sort((a, b) => b.done - a.done).map(p => {
                      const top = Math.max(...data.people.map(x => x.done), 1);
                      return (
                        <tr key={p.user_id} className={p.user_id === me.user_id ? 'is-me' : undefined}>
                          <td><Person id={p.user_id} people={people} /></td>
                          <td><b>{p.done}건</b></td>
                          <td className="adm-w-people-bar" aria-hidden="true"><span style={{ width: `${(p.done / top) * 100}%` }} /></td>
                          <td>{mins(p.avg_minutes)}</td>
                          <td>{p.open}건</td>
                          <td className={p.overdue ? 'is-late-text' : undefined}>{p.overdue}건</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          </Section>

          <Section title="업무 유형별 건수" meta={`${data.days}일 완료 기준`}>
            <div className="adm-card">
              <BarList items={[...data.by_type].sort((a, b) => b.count - a.count).map(t => ({ key: t.type, label: TASK_TYPE[t.type], value: t.count }))} format={v => `${v}건`} />
            </div>
          </Section>
        </>
      )}
    </div>
  );
}
