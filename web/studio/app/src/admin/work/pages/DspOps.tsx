// DSP 운영 — DSP마다 계약·연동 상태, 최근 30일 전송 실적(성공·실패·반려·평균 소요), 배급 대기·차단,
// 열린 예외, 담당자·연락처(관계 관리), DSP 전용 SLA를 한 화면에. 새로 저장하는 것은 없고 모두 원천에서 읽는다.
import { useSearchParams, Link } from '../../../lib/router';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, DELIVERY_TABS, Empty, ErrorBox, PageHead, Section, Skeleton, StatusChip, SubTabs } from '../../ui';
import { ago } from '../../labels';
import { workApi } from '../api';
import { CONTRACT_STATUS, EXCEPTION_KIND, REL_STATUS, SEVERITY, minutesLabel, pct, remaining, slaTypeLabel } from '../labels';
import { Person, usePeople } from '../ui';
import type { DspOpsItem } from '../types';

function health(d: DspOpsItem): { tone: 'red' | 'amber' | 'green' | 'gray'; label: string } {
  if (d.exceptions.critical > 0 || d.metrics.failure_rate >= 0.2) return { tone: 'red', label: '장애' };
  if (d.exceptions.open > 0 || d.staging.blocked > 0 || d.metrics.failure_rate > 0.05 || d.metrics.rejected > 0) return { tone: 'amber', label: '주의' };
  if (d.metrics.total > 0) return { tone: 'green', label: '정상' };
  return { tone: 'gray', label: '전송 없음' };
}

function Card({ d, active, onPick }: { d: DspOpsItem; active: boolean; onPick: () => void }) {
  const h = health(d);
  return (
    <button type="button" className={`adm-w-dsp${active ? ' is-active' : ''}`} onClick={onPick} aria-pressed={active}>
      <span className="adm-w-dsp-top"><b>{d.name}</b><Chip tone={h.tone}>{h.label}</Chip></span>
      <span className="adm-w-dsp-nums">
        <span><strong>{d.metrics.total}</strong>전송</span>
        <span><strong className={d.metrics.failed ? 'is-late-text' : ''}>{pct(d.metrics.failure_rate)}</strong>실패율</span>
        <span><strong>{d.exceptions.open}</strong>예외</span>
      </span>
      <small className="muted">{d.code}{d.relationship ? ` · ${REL_STATUS[d.relationship.status][0]}` : ' · 관계 미등록'}</small>
    </button>
  );
}

function Detail({ d, window }: { d: DspOpsItem; window: number }) {
  const people = usePeople();
  const rel = d.relationship;
  return (
    <div className="adm-w-dsp-detail">
      <div className="adm-w-stats">
        <div className="adm-w-stat"><strong>{d.metrics.delivered}</strong><span>전송 성공</span><small>{d.metrics.last_delivered_at ? `마지막 ${ago(d.metrics.last_delivered_at)}` : `최근 ${window}일`}</small></div>
        <div className={`adm-w-stat${d.metrics.failed ? ' is-alert' : ' is-muted'}`}><strong>{d.metrics.failed}</strong><span>전송 실패</span><small>{d.metrics.last_failure_at ? `마지막 ${ago(d.metrics.last_failure_at)}` : '없음'}</small></div>
        <div className={`adm-w-stat${d.metrics.rejected ? ' is-warn' : ' is-muted'}`}><strong>{d.metrics.rejected}</strong><span>DSP 반려</span><small>진행 중 {d.metrics.in_flight}</small></div>
        <div className="adm-w-stat"><strong>{d.metrics.avg_minutes != null ? minutesLabel(Math.round(d.metrics.avg_minutes)) : '—'}</strong><span>평균 전송 시간</span><small>대기 {d.staging.pending} · 차단 {d.staging.blocked}</small></div>
      </div>
      <div className="adm-w-dsp-grid">
        <Section title="계약·연동" meta="플랫폼별 조건과 같은 원천">
          <dl className="adm-w-fields">
            <div><dt>계약 경로</dt><dd>{d.contract_route?.route ?? '미정'}{d.contract_route?.contract_live ? <Chip tone="green">계약 유효</Chip> : <Chip tone="gray">계약 전</Chip>}</dd></div>
            <div><dt>전송 방식</dt><dd>{d.route?.transport ?? '—'}{d.route && (d.route.delivery_enabled ? <Chip tone="green">전송 켜짐</Chip> : <Chip tone="gray">전송 꺼짐</Chip>)}</dd></div>
            <div><dt>연동 단계</dt><dd>{d.route?.onboarding_stage ?? '—'}{d.route?.onboarding_gaps?.length ? <small>남은 항목: {d.route.onboarding_gaps.join(', ')}</small> : null}</dd></div>
            <div><dt>전용 SLA</dt><dd>{d.sla.length ? d.sla.map(s => `${slaTypeLabel(s.type)} ${minutesLabel(s.target_minutes)}`).join(' · ') : <span className="muted">공통 정책</span>}</dd></div>
          </dl>
          <Link to="/admin/dsps" className="adm-btn soft small">플랫폼별 조건 보기</Link>
        </Section>
        <Section title="담당·연락처" meta="관계 관리">
          {rel ? <>
            <dl className="adm-w-fields">
              <div><dt>담당자</dt><dd><Person id={rel.owner_id} people={people} /></dd></div>
              <div><dt>상태</dt><dd><StatusChip value={REL_STATUS[rel.status]} />{rel.contract && <StatusChip value={CONTRACT_STATUS[rel.contract.status]} />}</dd></div>
              <div><dt>주 연락처</dt><dd>{rel.primary_contact ? <>{rel.primary_contact.name}{rel.primary_contact.email && <small><a href={`mailto:${rel.primary_contact.email}`}>{rel.primary_contact.email}</a></small>}</> : '—'}</dd></div>
              <div><dt>마지막 연락</dt><dd>{rel.last_contact_at ? ago(rel.last_contact_at) : '기록 없음'}{rel.next_contact_at && <small>다음 연락 {remaining(rel.next_contact_at)}</small>}</dd></div>
            </dl>
            <Link to={`/admin/crm/${rel.id}`} className="adm-btn soft small">관계 열기</Link>
          </> : (
            <p className="small muted">이 DSP의 담당자·연락처가 아직 없어요. <Link to="/admin/crm?kind=DSP">관계 관리</Link>에서 등록하세요.</p>
          )}
        </Section>
      </div>
      <Section title="열린 예외" meta={`${d.open_exceptions?.length ?? 0}건`}>
        {!d.open_exceptions?.length ? <p className="small muted">열린 예외가 없어요.</p> : (
          <div className="adm-list">
            {d.open_exceptions.map(e => (
              <Link key={e.id} to={`/admin/exceptions?focus=${encodeURIComponent(e.id)}`} className="adm-row adm-w-policy">
                <span className="adm-min">
                  <span className="adm-row-title">{e.title}</span>
                  <span className="adm-row-meta">{e.kind && <span>{EXCEPTION_KIND[e.kind].label}</span>}{e.detail && <span className="adm-w-ellipsis">{e.detail}</span>}</span>
                </span>
                <span className="adm-row-end">{e.severity && <StatusChip value={SEVERITY[e.severity]} />}</span>
              </Link>
            ))}
          </div>
        )}
      </Section>
      <Section title="최근 전송 실패" meta="최근 20건">
        {!d.recent_failures?.length ? <p className="small muted">최근 실패한 전송이 없어요.</p> : (
          <div className="adm-list">
            {d.recent_failures.map(f => (
              <div key={f.job_id} className="adm-row adm-w-policy">
                <span className="adm-min">
                  <span className="adm-row-title">{f.title ?? '배급 패키지'}</span>
                  <span className="adm-row-meta"><span>{f.org_name}</span><span>시도 {f.attempts}회</span><span>{ago(f.updated_at)}</span>{f.last_error && <span className="adm-w-ellipsis">{f.last_error}</span>}</span>
                </span>
                <span className="adm-row-end"><Chip tone={f.status === 'DEAD_LETTER' ? 'red' : 'amber'}>{f.status === 'DEAD_LETTER' ? '중단' : '실패'}</Chip></span>
              </div>
            ))}
          </div>
        )}
      </Section>
    </div>
  );
}

export function DspOps() {
  const [params, setParams] = useSearchParams();
  const focus = params.get('dsp') ?? '';
  const { data, loading, error, reload } = useAsync(() => workApi.dspOps(focus || undefined), [focus]);
  const items = data?.items ?? [];
  const pick = (code: string) => { const next = new URLSearchParams(params); next.set('dsp', code); setParams(next, { replace: true }); };
  const sorted = [...items].sort((a, b) => b.exceptions.open - a.exceptions.open || b.metrics.failure_rate - a.metrics.failure_rate || b.metrics.total - a.metrics.total);
  const current = items.find(d => d.code === focus);
  const troubled = items.filter(d => health(d).tone === 'red' || health(d).tone === 'amber').length;

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="배급"
        title="DSP 운영"
        sub={`DSP별 계약·연동, 최근 ${data?.window_days ?? 30}일 전송 실적, 열린 예외, 담당자와 연락처를 한곳에서 봐요.`}
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DELIVERY_TABS} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={5} /> : items.length === 0 ? <Empty title="등록된 DSP가 없어요" /> : (
        <>
          {troubled > 0 && <div className="adm-alert adm-alert-row"><span><Glyph name="warning" size={14} /> 주의가 필요한 DSP가 <b>{troubled}</b>곳 있어요.</span></div>}
          <div className="adm-w-dsp-list">
            {sorted.map(d => <Card key={d.code} d={d} active={d.code === focus} onPick={() => pick(d.code)} />)}
          </div>
          {current ? <Detail d={current} window={data?.window_days ?? 30} /> : <p className="small muted" style={{ marginTop: 18 }}>DSP를 고르면 실패 기록·예외·연락처를 자세히 보여 줘요.</p>}
        </>
      )}
    </div>
  );
}
