// 퍼블리싱 신청 — 작품 상세·발매 상세·라이브러리·홈·계약 화면 어디서 들어와도 이 한 흐름 (기획안 6.1, 6.2).
// 1 대상 선택 → 2 권리 확인 → 3 계약 조건 → 4 전자계약 → 5 완료.
// 수수료·기간은 서버 계약 정책(/publishing/policy)에서 받고, 같은 신청은 멱등 키로 한 번만 접수된다.
// 고르던 내용은 이 브라우저에 남겨 두어 새로고침·연결 끊김 뒤에도 이어서 할 수 있다.
// 모양은 배급 신청과 같다: 단계 막대(StepBar), 선택 줄(ui-check), 동의 묶음(aq-agree-box), 손 서명(SignaturePad), 아래 고정 버튼(step-actions)
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type PublishingApplication, type Work } from '../api/publishing';
import { ApiError, errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { useAuth } from '../api/auth';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { uid } from '../lib/store';
import { APP_LABEL, pct, splitIssues, splitReady, termLabel } from '../lib/publishing';
import { money } from '../lib/format';
import { StepBar } from '../components/StepBar';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { SplitBar, roleText } from '../components/publishing/parts';
import { Badge, Callout, EmptyState, KeyValue, StatGrid, StatTile, Steps } from '../ui';
import { CheckIcon } from '../components/Check';
import '../styles/split/Upload.css';

const STEPS = ['신청 대상 선택', '권리 정보 확인', '계약 조건 확인', '전자계약', '신청 완료'];
const AGREE = [
  ['terms', '퍼블리싱 관리 계약서의 내용을 모두 읽고 이해했어요.'],
  ['rights', '신청한 작품에 대해 적은 지분만큼 권리를 가지고 있어요.'],
  ['exclusive', '계약 기간 동안 같은 권리를 다른 퍼블리셔에게 맡기지 않아요.'],
  ['share', '공동 참여자의 지분은 각자의 확인을 거쳐 반영된다는 것을 알아요.'],
] as const;

interface Saved { step: number; workIds: string[]; territories: string[]; rights: string[]; key: string }
const SAVE_KEY = 'publishing-apply';
const activeApp = (w: Work) => !!w.application && ['submitted', 'in_review', 'needs_fix', 'approved'].includes(w.application.status);

export function PublishingApply() {
  const [params] = useSearchParams();
  const nav = useNavigate();
  const { user } = useAuth();
  const preset = params.get('work');

  const { data: policy, error: policyError, reload: reloadPolicy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const { data: works, loading } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });

  const saved = useRef(readJSON<Saved | null>(SAVE_KEY, null));
  const resume = saved.current && (!preset || saved.current.workIds.includes(preset)) ? saved.current : null;
  const [step, setStep] = useState(resume?.step && resume.step < 4 ? resume.step : 0);
  const [picked, setPicked] = useState<string[]>(resume?.workIds ?? (preset ? [preset] : []));
  const [territories, setTerritories] = useState<string[]>(resume?.territories ?? []);
  const [rights, setRights] = useState<string[]>(resume?.rights ?? []);
  const [key] = useState(resume?.key ?? uid('pub'));
  const [agreed, setAgreed] = useState<string[]>([]);
  const [readAll, setReadAll] = useState(false);
  const [drawn, setDrawn] = useState(false);
  const pad = useRef<SignaturePadHandle>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<PublishingApplication | null>(null);
  const [failure, setFailure] = useState('');
  const topRef = useRef<HTMLDivElement>(null);

  // 정책을 받으면 기본값: 모든 지역·권리
  useEffect(() => {
    if (!policy) return;
    setTerritories(t => (t.length ? t : policy.territories.map(x => x.code)));
    setRights(r => (r.length ? r : policy.rights.map(x => x.key)));
  }, [policy]);

  useEffect(() => {
    if (result) return;
    writeJSON(SAVE_KEY, { step, workIds: picked, territories, rights, key } satisfies Saved);
  }, [step, picked, territories, rights, key, result]);

  // 단계를 옮기면 맨 위로 — 긴 계약서를 읽고 내려온 자리에서 다음 단계가 시작되지 않게
  useEffect(() => { topRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' }); }, [step]);

  const mine = useMemo(() => (works ?? []).filter(w => w.myRole === 'owner' || w.myRole === 'editor'), [works]);
  const chosen = mine.filter(w => picked.includes(w.id) && !activeApp(w));
  const blocked = chosen.filter(w => !splitReady(w.contributors));
  const conflicts = chosen.filter(w => w.hasPriorPublisher);
  const pendingPeople = chosen.flatMap(w => w.contributors.filter(c => !c.isMe && c.confirm !== 'confirmed').map(c => ({ w, c })));

  const canNext = [
    chosen.length > 0 && !chosen.some(activeApp),
    blocked.length === 0 && territories.length > 0 && rights.length > 0,
    true,
    readAll && agreed.length === AGREE.length && drawn,
  ][step];

  const submit = async () => {
    if (!policy || busy) return;
    setBusy(true);
    setFailure('');
    try {
      const app = await pubApi.submitApplication({
        workIds: chosen.map(w => w.id), territories, rights, policyVersion: policy.version, agreed,
        signature: { name: user?.email ?? '', method: 'drawn', data: pad.current?.toDataURL() },
        idempotencyKey: key,
      });
      removeKey(SAVE_KEY);
      setResult(app);
      setStep(4);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'POLICY_CHANGED') { reloadPolicy(); setStep(2); }
      setFailure(errorMessage(e, '신청을 보내지 못했어요. 같은 신청은 한 번만 접수되니 안심하고 다시 시도해 주세요.'));
    } finally { setBusy(false); }
  };

  if (policyError) {
    return <div className="view"><div className="ui-empty"><h2>계약 조건을 불러오지 못했어요.</h2><p>{policyError}</p><button type="button" className="ui-btn is-soft" onClick={reloadPolicy}>다시 불러오기</button></div></div>;
  }
  if (!policy || (loading && !works)) return <div className="view"><SkeletonBlock height={90} /><SkeletonBlock height={360} /></div>;

  const terrLabel = (c: string) => policy.territories.find(t => t.code === c)?.label ?? c;
  const rightLabel = (k: string) => policy.rights.find(r => r.key === k)?.label ?? k;
  const toggle = (list: string[], set: (v: string[]) => void, v: string) => set(list.includes(v) ? list.filter(x => x !== v) : [...list, v]);
  const fee = 100000 * policy.commissionRate / 100;

  return (
    <div className="view" ref={topRef}>
      {step < 4 && (
        <div className="spaced-actions">
          <button type="button" className="ui-btn is-white is-sm aq-back-link" onClick={() => (step ? setStep(step - 1) : nav(-1))}>
            <Glyph name="arrow-left" size={15} className="aq-glyph-lead" />{step ? '이전 단계' : '돌아가기'}
          </button>
        </div>
      )}
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">퍼블리싱</p>
          <h1>{step === 4 ? '신청을 받았어요' : '작품 관리 신청'}</h1>
          <p>{step === 4 ? '심사가 끝나면 알림으로 알려 드려요.' : '작품의 저작권 등록과 사용료 징수를 AUDENIQ이 맡아요.'}</p>
        </div>
      </div>
      {step < 4 && <StepBar steps={STEPS.slice(0, 4)} current={step} />}

      {step === 0 && (
        <section className="ui-tab-panel ui-card" key="s0" aria-labelledby="s0t">
          <h2 id="s0t">어떤 작품을 맡길까요?</h2>
          <p className="ui-field-help">여러 작품을 한 번에 신청할 수 있어요. 지분이 맞지 않는 작품은 먼저 고쳐 주세요.</p>
          {mine.length ? (
            <div className="aq-agree-box">
              {mine.map(w => {
                const busyApp = activeApp(w);
                return (
                  <label key={w.id} className="ui-check is-plain">
                    <input type="checkbox" checked={picked.includes(w.id)} disabled={busyApp} onChange={() => toggle(picked, setPicked, w.id)} />
                    <span className="min-0">
                      <strong>{w.title}</strong>{' '}
                      {busyApp ? <Badge tone="blue">{APP_LABEL[w.application!.status]}</Badge>
                        : splitReady(w.contributors) ? <Badge tone="green">지분 확인</Badge> : <Badge tone="amber">지분 확인 필요</Badge>}
                      <br /><small className="muted">{w.contributors.map(c => `${c.name} ${pct(c.share)}`).join(' · ') || '참여자 없음'}</small>
                    </span>
                  </label>
                );
              })}
            </div>
          ) : (
            <EmptyState compact title="신청할 작품이 없어요" actions={<Link to="/works/new" className="ui-btn is-primary">새 작품 등록</Link>}>
              먼저 작품을 등록해 주세요. 발매 곡에서 바로 만들 수도 있어요.
            </EmptyState>
          )}
          <div className="row-actions"><Link to="/works/new" className="ui-btn is-text is-sm">목록에 없는 곡 — 새 저작물 만들기</Link></div>
        </section>
      )}

      {step === 1 && (
        <section className="ui-tab-panel" key="s1" aria-label="권리 정보 확인">
          {blocked.length > 0 && <Callout tone="danger"><b>지분이 맞지 않는 작품이 있어 신청할 수 없어요.</b> 지분은 저장돼 있으니 고친 뒤 이어서 신청해요.</Callout>}
          {conflicts.length > 0 && <Callout tone="warn"><b>기존 퍼블리싱 계약과 겹칠 수 있어요.</b> {conflicts.map(w => `‘${w.title}’(${w.priorPublisher || '기존 퍼블리셔'})`).join(', ')} — 신청은 되고, 담당자가 먼저 검토해요.</Callout>}
          {pendingPeople.length > 0 && <Callout tone="neutral">공동 참여자 {pendingPeople.length}명이 아직 지분을 확인하지 않았어요. 확인 전에는 ‘확인 필요’ 상태로 심사해요.</Callout>}
          {chosen.map(w => {
            const issues = splitIssues(w.contributors).filter(i => i.level === 'error');
            return (
              <div key={w.id} className="ui-card">
                <div className="ui-section-head">
                  <h2>{w.title}<small>{w.iswc ?? w.internalCode}</small></h2>
                  <Link to={`/works/${w.id}?tab=rights`} className="aq-head-link">고치기<Glyph name="chevron-right" size={14} /></Link>
                </div>
                <SplitBar list={w.contributors} />
                <div className="ui-list is-inset">
                  {w.contributors.map(c => (
                    <div key={c.id} className="ui-row is-flat">
                      <span className="min-0">
                        <span className="ui-row-title">{c.name}{c.isMe && ' (나)'} · {pct(c.share)}</span>
                        <span className="ui-row-meta"><span>{roleText(c)}</span><span>{c.society ?? '단체 없음'}</span><span>{c.ipi ? `IPI ${c.ipi}` : 'IPI 없음'}</span></span>
                      </span>
                      {c.confirm === 'confirmed' ? <Badge tone="green">확인</Badge> : c.confirm === 'disputed' ? <Badge tone="red">이의</Badge> : <Badge tone="amber">확인 필요</Badge>}
                    </div>
                  ))}
                </div>
                {issues.map(i => <small key={i.text} className="ui-field-help is-error">{i.text}</small>)}
              </div>
            );
          })}
          <div className="ui-card">
            <h2>관리 대상 국가</h2>
            <div className="ui-chips is-wrap" role="group" aria-label="관리 대상 국가">
              {policy.territories.map(t => (
                <button key={t.code} type="button" className="ui-chip" aria-pressed={territories.includes(t.code)} onClick={() => toggle(territories, setTerritories, t.code)}>{t.label}</button>
              ))}
            </div>
            <h2>관리할 권리</h2>
            <div className="aq-agree-box">
              {policy.rights.map(r => (
                <label key={r.key} className="ui-check is-plain">
                  <input type="checkbox" checked={rights.includes(r.key)} onChange={() => toggle(rights, setRights, r.key)} />
                  <span><strong>{r.label}</strong><br /><small className="muted">{r.desc}</small></span>
                </label>
              ))}
            </div>
          </div>
        </section>
      )}

      {step === 2 && (
        <section className="ui-tab-panel" key="s2" aria-label="계약 조건">
          <StatGrid cols={2}>
            <StatTile label="관리 수수료" value={policy.commissionRate} unit="%" hint="징수한 사용료에서만 떼요. 가입비·월 이용료는 없어요." />
            <StatTile label="최초 계약 기간" value={termLabel(policy.initialTermMonths)} hint={`${policy.renewalOptions.map(termLabel).join(' · ')} 단위로 연장할 수 있어요.`} />
          </StatGrid>
          <div className="ui-card">
            <h2>계약 내용</h2>
            <KeyValue cols={2} items={[
              ['적용 작품', chosen.map(w => w.title).join(', ')],
              ['관리 대상 국가', territories.map(terrLabel).join(', ')],
              ['관리할 권리', rights.map(rightLabel).join(', ')],
              ['예시', `${money(100000)} 징수 → 수수료 ${money(fee)} · 받는 금액 ${money(100000 - fee)}`],
            ]} />
          </div>
          <Callout tone="neutral">계약이 끝나도 그동안의 등록·정산 기록은 그대로 남아요. 종료 후에는 새 작품을 관리에 넣을 수 없어요.</Callout>
        </section>
      )}

      {step === 3 && (
        <section className="ui-tab-panel ui-card" key="s3" aria-labelledby="s3t">
          <h2 id="s3t">퍼블리싱 관리 계약서</h2>
          <div className="aq-terms-body" tabIndex={0} aria-label="퍼블리싱 관리 계약서" style={{ maxHeight: 320, overflowY: 'auto' }} onScroll={e => {
            const el = e.currentTarget;
            if (el.scrollTop + el.clientHeight >= el.scrollHeight - 24) setReadAll(true);
          }}>
            <p><b>제1조 (목적)</b> 이 계약은 권리자(이하 ‘작가’)가 아래 작품의 저작권 관리를 주식회사 AUDENIQ(이하 ‘회사’)에 맡기고, 회사가 이를 관리하는 데 필요한 사항을 정한다.</p>
            <p><b>제2조 (대상 작품)</b> {chosen.map(w => `‘${w.title}’(${w.iswc ?? w.internalCode})`).join(', ')}. 작가는 각 작품에 신고한 지분만큼의 권리를 가진다.</p>
            <p><b>제3조 (관리 범위)</b> 회사는 {territories.map(terrLabel).join(', ')}에서 {rights.map(rightLabel).join(', ')}을(를) 관리한다. 관리 단체 등록, 사용료 징수, 이용 허락 협의를 포함한다.</p>
            <p><b>제4조 (수수료)</b> 회사는 실제 징수한 사용료의 {policy.commissionRate}%를 관리 수수료로 공제하고 나머지를 작가에게 지급한다. 예상 금액은 지급 대상이 아니다.</p>
            <p><b>제5조 (기간)</b> 계약 기간은 승인일부터 {termLabel(policy.initialTermMonths)}로 하며, 작가가 요청하면 {policy.renewalOptions.map(termLabel).join(', ')} 단위로 연장한다.</p>
            <p><b>제6조 (공동 저작)</b> 공동 참여자의 지분은 각 참여자의 확인을 거쳐 반영한다. 확인되지 않은 지분은 확인될 때까지 사용료 지급을 보류할 수 있다.</p>
            <p><b>제7조 (종료)</b> 어느 한쪽이 종료를 요청하면 관리 단체 절차에 따라 관리를 마친다. 종료 전 발생한 사용료는 종료 후에도 정산한다.</p>
            <p><b>제8조 (기존 계약)</b> 작가는 다른 퍼블리셔와의 계약을 신고해야 하며, 충돌이 있으면 회사가 검토한 뒤 관리 범위를 조정할 수 있다.</p>
            <p className="muted">— 계약서 끝 —</p>
          </div>
          {!readAll && <p className="ui-field-help">계약서를 끝까지 읽으면 동의할 수 있어요.</p>}
          <div className="ui-field">
            <label htmlFor="pubSignPad">서명 <span className="required">*</span></label>
            <SignaturePad ref={pad} id="pubSignPad" label="신청인 서명 입력" height={170} onChange={setDrawn} />
          </div>
          <div className="aq-agree-box">
            <label className="ui-check is-plain aq-agree-all-line">
              <input type="checkbox" disabled={!readAll} checked={agreed.length === AGREE.length} onChange={e => setAgreed(e.target.checked ? AGREE.map(a => a[0]) : [])} />
              <span><strong>아래 내용에 모두 동의해요</strong></span>
            </label>
            {AGREE.map(([k, t]) => (
              <label key={k} className="ui-check is-plain">
                <input type="checkbox" disabled={!readAll} checked={agreed.includes(k)} onChange={() => toggle(agreed, setAgreed, k)} />
                <span>{t}</span>
              </label>
            ))}
          </div>
          {failure && (
            <Callout tone="danger" action={<button type="button" className="ui-btn is-white is-sm" onClick={() => void submit()}>다시 시도</button>}>{failure}</Callout>
          )}
        </section>
      )}

      {step === 4 && result && (
        <section className="ui-tab-panel" key="s4" aria-live="polite">
          <EmptyState icon={<CheckIcon size={22} />} tone="green" title={`신청번호 ${result.number}`}
            actions={<><Link to={`/works/${result.workIds[0]}?tab=register`} className="ui-btn is-primary">진행 상황 보기</Link><Link to="/contracts" className="ui-btn is-soft">계약서</Link></>}>
            {result.workTitles.join(', ')} · {result.workTitles.length}개 작품
          </EmptyState>
          <div className="ui-card">
            <Steps steps={['제출 완료', '심사', '승인', '외부 등록']} current={1} label="처리 진행 상황" />
            <p className="ui-field-help">보통 영업일 3~5일 안에 심사 결과를 알려 드려요. 보완이 필요하면 알림과 작품 화면에 함께 보여요. 서명한 계약서는 계약서 화면에서 받을 수 있어요.</p>
          </div>
        </section>
      )}

      {step < 4 && (
        <div className="ui-actions">
          <button type="button" className="ui-btn is-soft" onClick={() => (step ? setStep(step - 1) : nav(-1))} disabled={busy}>{step ? '이전' : '나가기'}</button>
          {step < 3
            ? <button type="button" className="ui-btn is-primary" disabled={!canNext} onClick={() => setStep(step + 1)}>다음</button>
            : <button type="button" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={!canNext || busy} onClick={() => void submit()}>{busy ? '신청하는 중' : '서명하고 신청'}</button>}
        </div>
      )}
    </div>
  );
}
