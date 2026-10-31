// 저작권 관리 신청 — 곡 상세·발매 상세·라이브러리·홈·계약 화면 어디서 들어와도 이 한 흐름 (기획안 6.1, 6.2).
// 1 대상 선택 → 2 권리 확인 → 3 계약 조건 → 4 전자계약 → 5 완료.
// 수수료·기간은 서버 계약 정책(/publishing/policy)에서 받고, 같은 신청은 멱등 키로 한 번만 접수된다.
// 고르던 내용은 이 브라우저에 남겨 두어 새로고침·연결 끊김 뒤에도 이어서 할 수 있다.
// 모양은 배급 신청과 같다: 신청서 틀(WizardShell), 선택 줄(ui-check), 동의 묶음(aq-agree-box), 손 서명(SignaturePad), 아래 고정 버튼(step-actions)
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type PublishingApplication, type Work } from '../api/publishing';
import { ApiError, errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { useAuth } from '../api/auth';
import { useAccount } from '../store/account';
import { useProfile } from '../store/profile';
import { compactSignature } from '../lib/application';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { uid } from '../lib/store';
import { APP_LABEL, APP_TONE, REG_STEPS, pct, splitIssues, splitReady, termLabel } from '../lib/publishing';
import { money } from '../lib/format';
import { ReviewCard, WizardShell, type WizardStep } from '../components/WizardShell';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { SplitBar, roleText } from '../components/publishing/parts';
import { Badge, Callout, EmptyState, StatGrid, StatTile } from '../ui';
import { CheckIcon } from '../components/Check';
import { Burst } from '../components/Burst';
import '../styles/split/Upload.css';
import '../styles/split/Publishing.css';

// 단계 제목은 배급 신청서와 같은 말투 — 두 줄 큰 제목 + 한 줄 안내 (틀은 components/WizardShell)
const STEPS: WizardStep[] = [
  { short: '곡 선택', title: '어떤 곡을\n맡길까요?', sub: '여러 곡을 한 번에 신청할 수 있어요.' },
  { short: '권리 확인', title: '권리 정보를\n확인해 주세요.', sub: '지분과 관리할 나라·권리를 골라요.' },
  { short: '계약 조건', title: '계약 조건을\n확인해 주세요.', sub: '수수료는 실제로 받은 사용료에서만 떼요.' },
  { short: '전자계약', title: '계약서에\n서명해 주세요.', sub: '본인인증·정산 계좌는 배급에서 등록한 것을 그대로 써요.' },
];
const AGREE = [
  ['terms', '저작권 관리 계약서의 내용을 모두 읽고 이해했어요.'],
  ['rights', '신청한 곡에 대해 적은 지분만큼 권리를 가지고 있어요.'],
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
  const account = useAccount();
  const profile = useProfile();
  const signerName = account.realName.trim() || profile.name.trim() || user?.email || '';
  const preset = params.get('work');
  // 앨범 퍼블리싱 탭에서 여러 곡을 골라 '신청 대상으로 묶기'로 왔으면 그 곡들만
  const bundled = params.get('works')?.split(',').filter(Boolean) ?? [];
  // 발매 상세·계약 묶음·발매 신청에서 왔으면 그 발매 곡들의 곡을 미리 고른다
  const fromRelease = params.get('release');

  const { data: policy, error: policyError, reload: reloadPolicy } = useAsync(() => pubApi.policy(), [], { key: 'pub-policy' });
  const { data: works, loading, reload: reloadWorks } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [preparing, setPreparing] = useState(false);

  const saved = useRef(readJSON<Saved | null>(SAVE_KEY, null));
  const resume = saved.current && !bundled.length && (!preset || saved.current.workIds.includes(preset)) ? saved.current : null;
  const [step, setStep] = useState(resume?.step && resume.step < 4 ? resume.step : 0);
  const [picked, setPicked] = useState<string[]>(resume?.workIds ?? (bundled.length ? bundled : preset ? [preset] : []));
  // 발매에서 왔으면 그 발매에 연결된(아직 신청 전인) 곡을 모두 고른다
  useEffect(() => {
    if (!fromRelease || !works || bundled.length) return;
    const ids = works.filter(w => w.recordings.some(r => r.releaseId === fromRelease) && (w.myRole === 'owner' || w.myRole === 'editor') && !activeApp(w)).map(w => w.id);
    if (ids.length) setPicked(p => Array.from(new Set([...p, ...ids])));
  }, [fromRelease, works]);
  const releaseTitle = fromRelease ? works?.flatMap(w => w.recordings).find(r => r.releaseId === fromRelease)?.releaseTitle : undefined;
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
  // 지나온 단계는 위 진행 칸·왼쪽 목록에서 눌러 돌아갈 수 있다
  const [reached, setReached] = useState(step);
  useEffect(() => { setReached(r => Math.max(r, step)); window.scrollTo({ top: 0, behavior: 'smooth' }); }, [step]);

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
        // 배급 신청서와 같게 — 서명은 작게 줄여 보내고(서버 6만 자 한도), 이름은 본인인증 실명 → 활동명 순
        signature: { name: signerName, method: 'drawn', data: await compactSignature(pad.current?.toDataURL() ?? '') },
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

  // 신청을 받았으면 신청서 틀을 벗고 결과만
  if (step === 4 && result) {
    return (
      <div className="view aq-pbl-done" ref={topRef}>
        <div className="ui-page-head">
          <div>
            <p className="eyebrow">곡 저작권</p>
            <h1>신청을 받았어요</h1>
            <p>신청번호 <span className="aq-nowrap">{result.number}</span></p>
          </div>
        </div>
        {/* 발매 신청 완료(신청서 화면의 aq-doc-done)와 같은 카드 — 무엇을 했는지와 남은 단계를 한 장에.
            단계 이름은 곡 화면의 진행 단계(REG_STEPS)와 같은 말을 쓴다 */}
        <div className="aq-doc-done" role="status" aria-live="polite">
          <span className="aq-doc-done-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="22" height="22"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" /></svg>
            <Burst />
          </span>
          <div className="min-0">
            <strong>{result.workTitles.length > 1 ? `‘${result.workTitles[0]}’ 외 ${result.workTitles.length - 1}곡` : `‘${result.workTitles[0] ?? '곡'}’`} 저작권 관리를 신청했어요</strong>
            <span>{result.workTitles.join(', ')} · 남은 단계는 이래요.</span>
            <ol className="aq-doc-next" aria-label="다음 단계">
              <li className="is-done"><i aria-hidden="true"><CheckIcon size={10} /></i>{REG_STEPS[0]} — 방금 마쳤어요</li>
              <li><i aria-hidden="true">2</i>{REG_STEPS[1]} — 담당자가 참여자·지분을 확인해요. 보통 영업일 3~5일, 끝나면 알림으로 알려 드려요</li>
              <li><i aria-hidden="true">3</i>{REG_STEPS[2]} — 보완이 필요하면 곡 화면에 보완 요청이 보여요</li>
              <li><i aria-hidden="true">4</i>{REG_STEPS[3]} · {REG_STEPS[4]} — 관리 단체 등록이 끝나야 저작권료를 받아요</li>
            </ol>
          </div>
        </div>
        <div className="ui-actions is-fill aq-pbl-done-actions">
          <Link to="/contracts" className="ui-btn is-soft">계약서 보기</Link>
          <Link to={`/works/${result.workIds[0]}?tab=register`} className="ui-btn is-primary">진행 상황 보기</Link>
        </div>
      </div>
    );
  }

  const back = () => (step ? setStep(step - 1) : nav(-1));
  return (
    <WizardShell
      className="aq-pbl-wizard"
      label="저작권 관리 신청" title="저작권 관리 신청" steps={STEPS} step={Math.min(step, 3)} reached={Math.max(reached, step)}
      onStep={setStep} onBack={back}
      summary={{ title: releaseTitle ?? (chosen.length ? chosen[0].title : '곡을 골라 주세요'), lines: [`${chosen.length}곡 선택`, `수수료 ${policy.commissionRate}%`] }}
      railNote="입력 내용은 이 브라우저에 임시 저장돼요."
      actions={<>
        <button type="button" id="wizardBack" className="ui-btn is-soft" onClick={back} disabled={busy}>{step ? '이전' : '나가기'}</button>
        {step < 3
          ? <button type="button" id="wizardNext" className="ui-btn is-primary" disabled={!canNext} onClick={() => setStep(step + 1)}>다음으로</button>
          : <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={!canNext || busy} onClick={() => void submit()}>{busy ? '신청하는 중' : '서명하고 신청'}</button>}
      </>}
    >
      {step === 0 && (
        <>
          <p className="ui-field-help">{releaseTitle ? `‘${releaseTitle}’ 발매 곡을 골라 두었어요. ` : ''}지분이 맞지 않는 곡은 먼저 고쳐 주세요.</p>
          {fromRelease && (
            <Callout tone="neutral" action={
              <button type="button" className={`ui-btn is-white is-sm${preparing ? ' is-busy' : ''}`} disabled={preparing}
                onClick={async () => { setPreparing(true); try { await pubApi.prepareWorksFromRelease(fromRelease, true); reloadWorks(); } finally { setPreparing(false); } }}>
                {preparing ? '만드는 중' : '크레딧으로 곡 정보 만들기'}
              </button>
            }>
              저작권 정보가 없는 곡은 발매 크레딧에서 가져올 수 있어요.
            </Callout>
          )}
          {mine.length ? (
            <div className="aq-agree-box aq-pbl-song-picker">
              {mine.map(w => {
                const busyApp = activeApp(w);
                return (
                  <label key={w.id} className="ui-check is-plain">
                    <input type="checkbox" checked={picked.includes(w.id)} disabled={busyApp} onChange={() => toggle(picked, setPicked, w.id)} />
                    {/* 상태 색은 앱 전체 규칙 그대로 — 심사 중 보라, 보완 주황, 승인 초록. 고를 수 있는 곡은 '신청 가능'만 파랑 */}
                    <span className="aq-pbl-song-pick">
                      <span className="min-0">
                        <strong>{w.title}</strong>
                        <small>{w.contributors.map(c => `${c.name} ${pct(c.share)}`).join(' · ') || '참여자 없음'}</small>
                      </span>
                      {busyApp ? <Badge tone={APP_TONE[w.application!.status]}>{APP_LABEL[w.application!.status]}</Badge>
                        : splitReady(w.contributors) ? <Badge tone="blue">신청 가능</Badge> : <Badge tone="amber">지분 확인 필요</Badge>}
                    </span>
                  </label>
                );
              })}
            </div>
          ) : (
            <EmptyState compact title="신청할 곡이 없어요" actions={<Link to="/works/new" className="ui-btn is-primary">새 곡 등록</Link>}>
              먼저 곡을 등록해 주세요. 발매 곡에서 바로 만들 수도 있어요.
            </EmptyState>
          )}
          <div className="row-actions"><Link to="/works/new" className="ui-btn is-text is-sm">목록에 없는 곡 — 새 곡 만들기</Link></div>
        </>
      )}

      {step === 1 && (
        <>
          {blocked.length > 0 && <Callout tone="danger"><b>지분이 맞지 않는 곡이 있어 신청할 수 없어요.</b> 지분은 저장돼 있으니 고친 뒤 이어서 신청해요.</Callout>}
          {conflicts.length > 0 && <Callout tone="warn"><b>기존 저작권 관리 계약과 겹칠 수 있어요.</b> {conflicts.map(w => `‘${w.title}’(${w.priorPublisher || '기존 퍼블리셔'})`).join(', ')} — 신청은 되고, 담당자가 먼저 검토해요.</Callout>}
          {pendingPeople.length > 0 && <Callout tone="neutral">공동 참여자 {pendingPeople.length}명이 아직 지분을 확인하지 않았어요. 확인 전에는 ‘확인 필요’ 상태로 심사해요.</Callout>}
          {chosen.map(w => {
            const issues = splitIssues(w.contributors).filter(i => i.level === 'error');
            return (
              <div key={w.id} className="ui-card">
                <div className="ui-section-head">
                  <h2>{w.title}</h2>
                  <Link to={`/works/${w.id}?tab=rights`} className="aq-head-link">고치기<Glyph name="chevron-right" size={14} /></Link>
                </div>
                <SplitBar list={w.contributors} />
                <div className="ui-list is-inset">
                  {w.contributors.map(c => (
                    <div key={c.id} className="ui-row is-flat">
                      <span className="min-0">
                        <span className="ui-row-title">{c.name}{c.isMe && ' (나)'} · {pct(c.share)}</span>
                        <span className="ui-row-meta"><span>{roleText(c)}</span>{c.society && <span>{c.society}</span>}</span>
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
        </>
      )}

      {step === 2 && (
        <>
          <StatGrid cols={2}>
            <StatTile label="관리 수수료" value={policy.commissionRate} unit="%" hint="징수한 사용료에서만 떼요. 가입비·월 이용료는 없어요." />
            <StatTile label="최초 계약 기간" value={termLabel(policy.initialTermMonths)} hint={`${policy.renewalOptions.map(termLabel).join(' · ')} 단위로 연장할 수 있어요.`} />
          </StatGrid>
          <ReviewCard head="신청할 곡" onEdit={() => setStep(0)}>{chosen.map(w => w.title).join(', ')}</ReviewCard>
          <ReviewCard head="관리 대상 국가" onEdit={() => setStep(1)}>{territories.map(terrLabel).join(', ')}</ReviewCard>
          <ReviewCard head="관리할 권리" onEdit={() => setStep(1)}>{rights.map(rightLabel).join(', ')}</ReviewCard>
          <Callout tone="neutral">{money(100000)} 징수 시 수수료 {money(fee)}를 제외한 {money(100000 - fee)}를 받아요.</Callout>
          <Callout tone="neutral">계약이 끝나도 그동안의 등록·정산 기록은 그대로 남아요. 종료 후에는 새 곡을 관리에 넣을 수 없어요.</Callout>
        </>
      )}

      {step === 3 && (
        <>
          <h2 className="subhead">저작권 관리 계약서</h2>
          <div className="aq-terms-body" tabIndex={0} aria-label="저작권 관리 계약서" style={{ maxHeight: 320, overflowY: 'auto' }} onScroll={e => {
            const el = e.currentTarget;
            if (el.scrollTop + el.clientHeight >= el.scrollHeight - 24) setReadAll(true);
          }}>
            <p><b>제1조 (목적)</b> 이 계약은 권리자(이하 ‘작가’)가 아래 곡의 저작권 관리를 주식회사 AUDENIQ(이하 ‘회사’)에 맡기고, 회사가 이를 관리하는 데 필요한 사항을 정한다.</p>
            <p><b>제2조 (대상 곡)</b> {chosen.map(w => `‘${w.title}’(${w.iswc ?? w.internalCode})`).join(', ')}. 작가는 각 곡에 신고한 지분만큼의 권리를 가진다.</p>
            <p><b>제3조 (관리 범위)</b> 회사는 {territories.map(terrLabel).join(', ')}에서 {rights.map(rightLabel).join(', ')}을(를) 관리한다. 관리 단체 등록, 사용료 징수, 이용 허락 협의를 포함한다.</p>
            <p><b>제4조 (수수료)</b> 회사는 실제 징수한 사용료의 {policy.commissionRate}%를 관리 수수료로 공제하고 나머지를 작가에게 지급한다. 예상 금액은 지급 대상이 아니다.</p>
            <p><b>제5조 (기간)</b> 계약 기간은 승인일부터 {termLabel(policy.initialTermMonths)}로 하며, 작가가 요청하면 {policy.renewalOptions.map(termLabel).join(', ')} 단위로 연장한다.</p>
            <p><b>제6조 (공동 저작)</b> 공동 참여자의 지분은 각 참여자의 확인을 거쳐 반영한다. 확인되지 않은 지분은 확인될 때까지 사용료 지급을 보류할 수 있다.</p>
            <p><b>제7조 (종료)</b> 어느 한쪽이 종료를 요청하면 관리 단체 절차에 따라 관리를 마친다. 종료 전 발생한 사용료는 종료 후에도 정산한다.</p>
            <p><b>제8조 (기존 계약)</b> 작가는 다른 퍼블리셔와의 계약을 신고해야 하며, 충돌이 있으면 회사가 검토한 뒤 관리 범위를 조정할 수 있다.</p>
            <p className="muted">— 계약서 끝 —</p>
          </div>
          {!readAll && <p className="ui-field-help">계약서를 끝까지 읽으면 동의할 수 있어요.</p>}
          <p className="ui-field-help">본인인증·정산 계좌는 배급에서 등록한 것을 그대로 써요. 저작권료도 같은 잔액에 쌓여 한 번에 받아요.</p>
          <div className="ui-field">
            <label htmlFor="pubSignPad">서명 <span className="required">*</span></label>
            <SignaturePad ref={pad} id="pubSignPad" label="신청인 서명 입력" height={170} onChange={setDrawn} />
            <small className="ui-field-help">신청인 <b>{signerName || '이름 없음'}</b>{account.realName.trim() ? ' (본인인증 실명)' : ' — 본인인증을 하면 실명으로 기록돼요'}</small>
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
        </>
      )}

    </WizardShell>
  );
}
