// DSP 운영 — 송출 workflow와 별개로 DSP 자체를 관리. 장애율 · 평균 승인 시간 · 반려 사유 · 계약 상태 ·
// API/DDEX 연동 상태 · 담당자 · 최근 문의 · SLA.
import { useState } from 'react';
import { Link, useSearchParams } from '../../../lib/router';
import { useToast } from '../../../components/Toast';
import { errorMessage } from '../../../api/errors';
import { useAsync } from '../../../hooks/useAsync';
import { Glyph } from '../../../components/Glyph';
import { Chip, Empty, ErrorBox, PageHead, Seg, Skeleton, StatusChip, SubTabs, useStaff } from '../../ui';
import { ago, day, INQUIRY_STATUS, pick } from '../../labels';
import { workApi } from '../api';
import { CONTRACT_STAGE, CONTRACT_STAGES, DSP_TABS, HEALTH, INTEGRATION } from '../labels';
import { BarList, Sparkline } from '../charts';
import { NotesPanel, Person, PersonSelect, usePeople } from '../ui';
import type { ContractStage, DspOps as Dsp, DspOpsPatch } from '../types';

const rate = (d: Dsp) => (d.deliveries_30d ? (d.failures_30d / d.deliveries_30d) * 100 : 0);
const pct = (v: number) => `${v.toFixed(v < 10 ? 1 : 0)}%`;
const isBad = (d: Dsp) => d.health === 'DOWN' || rate(d) >= 5;
const isWarn = (d: Dsp) => !isBad(d) && (d.health === 'DEGRADED' || rate(d) >= 3 || d.sla_breaches_30d > 0);

function DspDetail({ d, onChanged }: { d: Dsp; onChanged: () => void }) {
  const toast = useToast();
  const people = usePeople();
  const { me } = useStaff();
  const [sla, setSla] = useState(String(d.sla_hours));
  const save = async (body: DspOpsPatch, msg: string) => {
    try { await workApi.updateDspOps(d.code, body); toast(msg, 'success'); onChanged(); } catch (err) { toast(errorMessage(err, '바꾸지 못했어요.'), 'error'); }
  };
  const canEdit = me.role === 'ADMIN' || me.role === 'OPERATOR';
  const totalRej = d.rejections.reduce((s, r) => s + r.count, 0);

  return (
    <section className="adm-card adm-w-dsp-detail" aria-label={`${d.name} 운영 상세`}>
      <div className="adm-check-top">
        <span className="adm-min">
          <span className="adm-codes"><StatusChip value={HEALTH[d.health]} /><Chip tone="gray">{INTEGRATION[d.integration]}</Chip><StatusChip value={CONTRACT_STAGE[d.contract_stage]} /></span>
          <h2 style={{ marginTop: 8 }}>{d.name}</h2>
          <span className="adm-row-meta"><span>{d.region === 'KR' ? '국내' : '해외'}</span><span>마지막 동기화 {d.last_sync_at ? ago(d.last_sync_at) : '없음'}</span></span>
        </span>
        <span className="adm-codes">
          <Link to={`/admin/tasks?view=all&status=all&related_kind=dsp&related_id=${encodeURIComponent(d.code)}`} className="adm-next-btn">관련 업무<Glyph name="chevron-right" size={14} /></Link>
          <Link to="/admin/dsps" className="adm-next-btn">플랫폼 조건<Glyph name="chevron-right" size={14} /></Link>
        </span>
      </div>

      <div className="adm-w-dsp-metrics">
        <div><small>30일 장애율</small><strong className={isBad(d) ? 'is-bad' : undefined}>{pct(rate(d))}</strong><span>{d.failures_30d} / {d.deliveries_30d}건</span><Sparkline values={d.failure_trend} label="최근 8주 장애율" unit="%" bad={isBad(d)} width={160} /></div>
        <div><small>평균 승인 시간</small><strong>{d.avg_approval_hours == null ? '—' : `${d.avg_approval_hours}시간`}</strong><span>전송 → DSP 승인</span></div>
        <div><small>문의 응답 SLA</small><strong>{d.sla_hours}시간</strong><span>30일 초과 {d.sla_breaches_30d}건</span></div>
        <div><small>열린 문의</small><strong>{d.open_inquiries}</strong><span>DSP에 보낸 문의</span></div>
      </div>

      <div className="adm-w-grid2 adm-w-dsp-cols">
        <div>
          <div className="adm-section-top"><h2>반려 사유<small>30일 {totalRej}건</small></h2></div>
          {d.rejections.length ? <BarList items={d.rejections.map(r => ({ key: r.reason, label: r.reason, value: r.count }))} format={v => `${v}건`} /> : <p className="small muted">최근 30일 반려가 없어요.</p>}
        </div>
        <div>
          <div className="adm-section-top"><h2>최근 문의</h2></div>
          {d.recent_inquiries.length ? (
            <ul className="adm-w-mini-list">
              {d.recent_inquiries.map(q => <li key={q.id}><span className="adm-min"><b>{q.subject}</b><small>{ago(q.at)}</small></span><StatusChip value={pick(INQUIRY_STATUS, q.status)} /></li>)}
            </ul>
          ) : <p className="small muted">최근 문의가 없어요.</p>}
        </div>
      </div>

      <div className="adm-section-top" style={{ marginTop: 20 }}><h2>운영 정보</h2></div>
      <dl className="adm-kv">
        <div><dt><label htmlFor="dsOwner">담당자</label></dt><dd>{canEdit ? <PersonSelect id="dsOwner" value={d.owner_id} onChange={v => save({ owner_id: v }, '담당자를 바꿨어요.')} people={people} empty="담당자 없음" /> : <Person id={d.owner_id} people={people} />}</dd></div>
        <div><dt><label htmlFor="dsStage">계약 상태</label></dt><dd>
          <select id="dsStage" className="adm-select adm-w-select" disabled={!canEdit} value={d.contract_stage} onChange={e => save({ contract_stage: e.target.value as ContractStage }, '계약 상태를 바꿨어요.')}>{CONTRACT_STAGES.map(s => <option key={s} value={s}>{CONTRACT_STAGE[s][0]}</option>)}</select>
        </dd></div>
        <div><dt><label htmlFor="dsRenew">계약 갱신일</label></dt><dd>
          <input id="dsRenew" type="date" className="adm-input" disabled={!canEdit} key={d.contract_renewal_at ?? 'none'} defaultValue={d.contract_renewal_at?.slice(0, 10) ?? ''} onBlur={e => { const v = e.target.value ? new Date(`${e.target.value}T00:00:00+09:00`).toISOString() : null; if ((v ?? '').slice(0, 10) !== (d.contract_renewal_at ?? '').slice(0, 10)) void save({ contract_renewal_at: v }, '갱신일을 바꿨어요.'); }} />
        </dd></div>
        <div><dt><label htmlFor="dsSla">문의 응답 SLA(시간)</label></dt><dd>
          <input id="dsSla" type="number" min={1} max={720} className="adm-input" disabled={!canEdit} value={sla} onChange={e => setSla(e.target.value)} onBlur={() => { const n = Number(sla); if (n >= 1 && n !== d.sla_hours) void save({ sla_hours: n }, 'SLA를 바꿨어요.'); }} />
        </dd></div>
        <div><dt><label htmlFor="dsHealth">연동 상태</label></dt><dd>
          <select id="dsHealth" className="adm-select adm-w-select" disabled={!canEdit} value={d.health} onChange={e => save({ health: e.target.value as Dsp['health'] }, '연동 상태를 바꿨어요.')}>{(Object.keys(HEALTH) as Dsp['health'][]).map(h => <option key={h} value={h}>{HEALTH[h][0]}</option>)}</select>
        </dd></div>
        <div><dt>갱신까지</dt><dd>{d.contract_renewal_at ? `${day(d.contract_renewal_at)} (${Math.ceil((Date.parse(d.contract_renewal_at) - Date.now()) / 86_400_000)}일)` : '—'}</dd></div>
      </dl>

      <div style={{ marginTop: 22 }}><NotesPanel kind="dsp" targetId={d.code} title={`${d.name} 내부 메모`} /></div>
    </section>
  );
}

export function DspOps() {
  const [params, setParams] = useSearchParams();
  const sel = params.get('dsp');
  const [sort, setSort] = useState<'risk' | 'name' | 'volume'>('risk');
  const people = usePeople();
  const { data, loading, error, reload } = useAsync(() => workApi.dspOps(), []);
  const items = [...(data?.items ?? [])].sort((a, b) =>
    sort === 'name' ? a.name.localeCompare(b.name) : sort === 'volume' ? b.deliveries_30d - a.deliveries_30d
      : Number(isBad(b)) - Number(isBad(a)) || Number(isWarn(b)) - Number(isWarn(a)) || rate(b) - rate(a));
  const current = items.find(d => d.code === sel);

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="배급"
        title="DSP 운영"
        sub="플랫폼마다 장애율, 승인 시간, 반려 사유, 계약·연동 상태와 담당자를 한 화면에서 봐요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      <SubTabs tabs={DSP_TABS} />
      <div className="adm-queue-tools">
        <Seg label="정렬" value={sort} onChange={setSort} options={[['risk', '문제 많은 순'], ['volume', '전송량 순'], ['name', '이름 순']] as const} />
      </div>
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? <Empty icon={<Glyph name="globe" size={22} />} title="DSP 운영 정보가 없어요" /> : (
        <>
          {current && <DspDetail key={current.code} d={current} onChanged={reload} />}
          <div className="adm-dsp-grid adm-w-dsp-grid">
            {items.map(d => {
              const bad = isBad(d);
              const warn = isWarn(d);
              return (
                <button key={d.code} type="button" className={`adm-dsp adm-w-dsp${bad ? ' is-bad' : warn ? ' is-warn' : ''}${d.code === sel ? ' is-active' : ''}`} aria-pressed={d.code === sel} onClick={() => setParams(d.code === sel ? {} : { dsp: d.code }, { replace: true })}>
                  <div className="adm-dsp-top">
                    <h3>{d.name}</h3>
                    <StatusChip value={HEALTH[d.health]} />
                  </div>
                  <span className="small muted">{INTEGRATION[d.integration]} · {CONTRACT_STAGE[d.contract_stage][0]}</span>
                  <div className="adm-w-dsp-row">
                    <span><small>장애율</small><b className={bad ? 'is-bad' : undefined}>{pct(rate(d))}</b></span>
                    <Sparkline values={d.failure_trend} label={`${d.name} 최근 8주 장애율`} unit="%" bad={bad} />
                  </div>
                  <ul>
                    <li><span>평균 승인</span><b>{d.avg_approval_hours == null ? '—' : `${d.avg_approval_hours}시간`}</b></li>
                    <li><span>주요 반려</span><b>{d.rejections[0]?.reason ?? '없음'}</b></li>
                    <li><span>열린 문의</span><b>{d.open_inquiries}건 · SLA {d.sla_hours}h</b></li>
                    <li><span>담당자</span><b><Person id={d.owner_id} people={people} /></b></li>
                  </ul>
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
