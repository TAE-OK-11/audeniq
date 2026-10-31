// 저작권 관리 신청 — 곡 상세·발매 상세·라이브러리·홈·계약 화면 어디서 들어와도 이 한 흐름 (기획안 6.1, 6.2).
// 1 대상 선택 → 2 권리 확인 → 3 조건 확인 → 4 신청서 서명 → 접수한 신청서(/publishing/applications/:id?done=1).
// 배급처럼 신청서와 계약서는 따로다: 계약서는 담당자가 승인한 뒤 계약서 화면에서 서명한다.
// 수수료·기간은 서버 계약 정책(/publishing/policy)에서 받고, 같은 신청은 멱등 키로 한 번만 접수된다.
// 고르던 내용은 이 브라우저에 남겨 두어 새로고침·연결 끊김 뒤에도 이어서 할 수 있다.
// 모양은 배급 신청과 같다: 신청서 틀(WizardShell), 선택 줄(ui-check), 동의 묶음(aq-agree-box), 손 서명(SignaturePad), 아래 고정 버튼(step-actions)
import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate, useSearchParams } from '../lib/router';
import { pubApi, type Work } from '../api/publishing';
import { ApiError, errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { useAuth } from '../api/auth';
import { useAccount } from '../store/account';
import { useProfile } from '../store/profile';
import { compactSignature } from '../lib/application';
import { readJSON, removeKey, writeJSON } from '../lib/storage';
import { uid } from '../lib/store';
import { APPLICATION_AGREE, APP_LABEL, APP_TONE, isReleased, pct, societyName, splitIssues, splitReady, termLabel } from '../lib/publishing';
import { money } from '../lib/format';
import { ReviewCard, WizardShell, type WizardStep } from '../components/WizardShell';
import { SkeletonBlock } from '../components/Skeleton';
import { Glyph } from '../components/Glyph';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { SplitBar, roleText } from '../components/publishing/parts';
import { SocietyJoin } from '../components/publishing/SocietyJoin';
import { AttachmentModal } from '../components/publishing/Attachments';
import { penAsOthers, workIssues } from '../lib/publishingPrep';
import { Badge, Callout, EmptyState, StatGrid, StatTile } from '../ui';
import '../styles/split/Upload.css';
import '../styles/split/Publishing.css';

// 단계 제목은 배급 신청서와 같은 말투 — 두 줄 큰 제목 + 한 줄 안내 (틀은 components/WizardShell)
const STEPS: WizardStep[] = [
  { short: '곡 선택', title: '어떤 곡을\n맡길까요?', sub: '여러 곡을 한 번에 신청할 수 있어요.' },
  { short: '권리 확인', title: '권리 정보를\n확인해 주세요.', sub: '지분과 관리할 나라·권리를 골라요.' },
  { short: '조건 확인', title: '신청 조건을\n확인해 주세요.', sub: '수수료는 실제로 받은 사용료에서만 떼요.' },
  { short: '신청서 서명', title: '신청서에\n서명해 주세요.', sub: '계약서는 담당자가 확인을 마친 뒤 따로 보내 드려요.' },
];
// 배급처럼 신청서와 계약서는 따로 — 여기서는 신청 내용이 맞다는 확인과 신청서 서명만. 계약은 승인 뒤 계약서 화면에서
const AGREE = APPLICATION_AGREE;

interface Saved { step: number; workIds: string[]; territories: string[]; rights: string[]; key: string; from?: string }
const SAVE_KEY = 'publishing-apply';
// 지난번에 고른 관리 범위 — 다음 신청에서 국가·권리를 처음부터 다시 고르지 않게 (계정 정보의 작가 정보처럼 한 번 고르면 다시 쓴다)
const SCOPE_KEY = 'publishing-apply-scope';
interface Scope { territories: string[]; rights: string[] }
const activeApp = (w: Work) => !!w.application && ['submitted', 'in_review', 'needs_fix', 'approved'].includes(w.application.status);
// 저작권 관리는 발매된 곡(내 발매 · 다른 아티스트 발매)만 — 발매 전 곡은 Open A&R로 피칭한다 (서버도 WORK_NOT_RELEASED로 막는다)
const canPick = (w: Work) => !activeApp(w) && isReleased(w) && w.aiUse !== 'generated';
// 원곡이 따로 있는 곡(편곡·번안·메들리) — 원곡 권리자 허락이 있어야 관리할 수 있다
const DERIVED = ['arrangement', 'translation', 'medley'];

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
  // 이어 쓰기는 같은 자리에서 들어왔을 때만 — 다른 발매·곡에서 들어왔는데 예전에 고르던 곡과 단계가 그대로 열렸다
  const from = fromRelease ? `release:${fromRelease}` : preset ? `work:${preset}` : '';
  const resume = saved.current && !bundled.length && (saved.current.from ?? '') === from && (!preset || saved.current.workIds.includes(preset)) ? saved.current : null;
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
  const [drawn, setDrawn] = useState(false);
  const pad = useRef<SignaturePadHandle>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState('');

  // 정책을 받으면 기본값: 지난 신청과 같은 범위(지금 정책에 있는 것만), 처음이면 모든 지역·권리
  const lastScope = useRef(readJSON<Scope | null>(SCOPE_KEY, null));
  const [fromLast, setFromLast] = useState(false);
  useEffect(() => {
    if (!policy) return;
    const lt = lastScope.current?.territories.filter(c => policy.territories.some(x => x.code === c)) ?? [];
    const lr = lastScope.current?.rights.filter(k => policy.rights.some(x => x.key === k)) ?? [];
    setTerritories(t => (t.length ? t : lt.length ? lt : policy.territories.map(x => x.code)));
    setRights(r => (r.length ? r : lr.length ? lr : policy.rights.map(x => x.key)));
    if (!resume && lt.length && lr.length) setFromLast(true);
  }, [policy]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    writeJSON(SAVE_KEY, { step, workIds: picked, territories, rights, key, from } satisfies Saved);
  }, [step, picked, territories, rights, key, from]);

  // 단계를 옮기면 맨 위로 — 긴 계약서를 읽고 내려온 자리에서 다음 단계가 시작되지 않게
  // 지나온 단계는 위 진행 칸·왼쪽 목록에서 눌러 돌아갈 수 있다
  const [reached, setReached] = useState(step);
  useEffect(() => { setReached(r => Math.max(r, step)); window.scrollTo({ top: 0, behavior: 'smooth' }); }, [step]);

  // 고를 수 있는 곡이 먼저 — 신청할 수 없는(발매 전·진행 중) 곡이 위에 쌓여 고를 곡을 찾기 어려웠다
  const mine = useMemo(() => (works ?? []).filter(w => w.myRole === 'owner' || w.myRole === 'editor')
    .sort((a, b) => Number(!canPick(a)) - Number(!canPick(b)) || Number(!isReleased(b)) - Number(!isReleased(a))), [works]);
  const chosen = mine.filter(w => picked.includes(w.id) && canPick(w));
  // 신청을 막는 일 — 지분(합계·역할·작곡가·이의)과 창작 증빙 (서버 WORK_SPLIT_INCOMPLETE · WORK_EVIDENCE_REQUIRED와 같다)
  const blocked = chosen.filter(w => !splitReady(w.contributors) || workIssues(w).some(i => i.blocking && i.field !== 'fix'));
  const [attachFor, setAttachFor] = useState<Work | null>(null);
  // 내 필명인데 다른 사람으로 들어간 참여자 — 남처럼 확인을 기다리고 내 몫이 남의 몫으로 등록된다
  const { data: me } = useAsync(() => pubApi.myCreator(), [], { key: 'my-creator' });
  const penAsOther = (w: Work) => penAsOthers(w.contributors, me);
  const conflicts = chosen.filter(w => w.hasPriorPublisher);
  const pendingPeople = chosen.flatMap(w => w.contributors.filter(c => !c.isMe && c.confirm !== 'confirmed').map(c => ({ w, c })));

  const canNext = [
    chosen.length > 0 && !chosen.some(activeApp),
    blocked.length === 0 && territories.length > 0 && rights.length > 0,
    true,
    agreed.length === AGREE.length && drawn && !!account.verified,
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
      writeJSON(SCOPE_KEY, { territories, rights } satisfies Scope);
      // 배급처럼 접수한 신청서를 바로 보여 준다 — 맨 위 '신청했어요' 카드와 남은 단계, 그 아래 서명한 신청서
      nav(`/publishing/applications/${encodeURIComponent(app.id)}?done=1`, { replace: true });
    } catch (e) {
      if (e instanceof ApiError && e.code === 'POLICY_CHANGED') { reloadPolicy(); setStep(2); }
      setFailure(errorMessage(e, '신청을 보내지 못했어요. 같은 신청은 한 번만 접수되니 안심하고 다시 시도해 주세요.'));
    } finally { setBusy(false); }
  };

  if (policyError) {
    return <div className="view"><div className="ui-empty"><h2>신청 조건을 불러오지 못했어요.</h2><p>{policyError}</p><button type="button" className="ui-btn is-soft" onClick={reloadPolicy}>다시 불러오기</button></div></div>;
  }
  if (!policy || (loading && !works)) return <div className="view"><SkeletonBlock height={90} /><SkeletonBlock height={360} /></div>;

  const terrLabel = (c: string) => policy.territories.find(t => t.code === c)?.label ?? c;
  const rightLabel = (k: string) => policy.rights.find(r => r.key === k)?.label ?? k;
  const toggle = (list: string[], set: (v: string[]) => void, v: string) => set(list.includes(v) ? list.filter(x => x !== v) : [...list, v]);
  const fee = 100000 * policy.commissionRate / 100;

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
          : <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={!canNext || busy} onClick={() => void submit()}>{busy ? '신청하는 중' : '신청서 서명하고 제출'}</button>}
      </>}
    >
      {step === 0 && (
        <>
          <p className="ui-field-help">{releaseTitle ? `‘${releaseTitle}’ 발매 곡을 골라 두었어요. ` : ''}발매된 곡만 신청할 수 있어요. 지분이 맞지 않는 곡은 먼저 고쳐 주세요.</p>
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
                const demo = !isReleased(w);
                const aiOnly = w.aiUse === 'generated';
                return (
                  <label key={w.id} className="ui-check is-plain">
                    <input type="checkbox" checked={picked.includes(w.id) && !demo && !aiOnly} disabled={busyApp || demo || aiOnly} onChange={() => toggle(picked, setPicked, w.id)} />
                    {/* 상태 색은 앱 전체 규칙 그대로 — 심사 중 보라, 보완 주황, 승인 초록. 고를 수 있는 곡은 '신청 가능'만 파랑 */}
                    <span className="aq-pbl-song-pick">
                      <span className="min-0">
                        <strong>{w.title}</strong>
                        <small>{w.contributors.map(c => `${c.name} ${pct(c.share)}`).join(' · ') || '참여자 없음'}</small>
                      </span>
                      {busyApp ? <Badge tone={APP_TONE[w.application!.status]}>{APP_LABEL[w.application!.status]}</Badge>
                        : demo ? <Badge tone="gray">발매 전 · A&R 대상</Badge>
                        : aiOnly ? <Badge tone="gray">AI 생성 · 신청 불가</Badge>
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
          {blocked.length > 0 && <Callout tone="danger"><b>아직 신청할 수 없는 곡이 있어요.</b> {blocked.map(w => `‘${w.title}’`).join(', ')} — 지분을 맞추고 창작 증빙 자료를 하나 이상 올려 주세요. 고르던 내용은 그대로 남아요.</Callout>}
          {conflicts.length > 0 && <Callout tone="warn"><b>기존 저작권 관리 계약과 겹칠 수 있어요.</b> {conflicts.map(w => `‘${w.title}’(${w.priorPublisher || '기존 퍼블리셔'})`).join(', ')} — 신청은 되고, 담당자가 먼저 검토해요.</Callout>}
          {chosen.some(w => w.contributors.some(c => c.isMe && !c.society)) && <SocietyJoin />}
          {/* 원곡 허락을 확인했다고 적은 곡은 빼고 — 남은 곡만 짚는다 */}
          {chosen.some(w => DERIVED.includes(w.workType) && !w.originalWork?.permission) && <Callout tone="warn"><b>원곡 허락을 확인하지 않은 곡이 있어요.</b> {chosen.filter(w => DERIVED.includes(w.workType) && !w.originalWork?.permission).map(w => `‘${w.title}’`).join(', ')} — 편곡·번안·메들리는 원곡 권리자의 허락이 있어야 관리할 수 있어요. 곡 정보에 원곡과 허락 여부를 적고, 허락서를 자료로 올려 두세요.</Callout>}
          {chosen.some(w => w.samples?.length) && <Callout tone="neutral"><b>샘플·인용이 있는 곡이 있어요.</b> {chosen.filter(w => w.samples?.length).map(w => `‘${w.title}’`).join(', ')} — 담당자가 원곡 권리자 몫을 함께 확인해요.</Callout>}
          {pendingPeople.length > 0 && <Callout tone="neutral">공동 참여자 {pendingPeople.length}명이 아직 지분을 확인하지 않았어요. 확인 전에는 ‘확인 필요’ 상태로 심사해요.</Callout>}
          {chosen.map(w => {
            const issues = splitIssues(w.contributors).filter(i => i.level === 'error');
            // 곡별 점검표 — 막는 일(창작 증빙)과 미리 해 두면 좋은 일(가사·창작일·합의서·IPI)
            const checks = workIssues(w).filter(i => i.field === 'evidence' || (i.field === 'ipi' && !i.blocking));
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
                        <span className="ui-row-title">{c.name}{c.isMe && (c.penName ? ' (나 · 필명)' : ' (나)')} · {pct(c.share)}</span>
                        <span className="ui-row-meta"><span>{roleText(c)}</span>{c.society && <span>{societyName(c.society)}</span>}</span>
                      </span>
                      {c.confirm === 'confirmed' ? <Badge tone="green">확인</Badge> : c.confirm === 'disputed' ? <Badge tone="red">이의</Badge> : <Badge tone="amber">확인 필요</Badge>}
                    </div>
                  ))}
                </div>
                {issues.map(i => <small key={i.text} className="ui-field-help is-error">{i.text}</small>)}
                <div className="aq-pbl-checks">
                  <p className="aq-pbl-checks-head">
                    <span>창작 증빙 · 등록 정보</span>
                    <span className="muted">{w.attachments.length ? `자료 ${w.attachments.length}개` : '자료 없음'}{w.createdOn ? ` · 창작 ${w.createdOn}` : ''}</span>
                  </p>
                  {penAsOther(w).map(c => (
                    <p key={c.id} className="aq-pbl-checks-pen"><Glyph name="user" size={14} />‘{c.name}’은(는) 내 필명인데 다른 사람으로 들어가 있어요. <Link to={`/works/${w.id}?tab=rights`}>곡 화면에서 나로 합치기</Link></p>
                  ))}
                  {checks.length ? (
                    <ul>
                      {checks.map(i => <li key={i.text} className={i.blocking ? 'is-block' : ''}><Glyph name={i.blocking ? 'warning' : 'info'} size={14} />{i.text}</li>)}
                    </ul>
                  ) : <p className="ui-field-help">등록에 필요한 정보와 창작 증빙이 모두 있어요.</p>}
                  {(w.myRole === 'owner' || w.myRole === 'editor') && (
                    <button type="button" className="ui-btn is-soft is-sm" onClick={() => setAttachFor(w)}><Glyph name="clip" size={14} className="aq-glyph-lead" />증빙 자료 올리기</button>
                  )}
                </div>
              </div>
            );
          })}
          <div className="ui-card">
            {fromLast && <p className="ui-field-help aq-pbl-import-note"><Glyph name="repeat" size={14} />지난 신청과 같은 범위로 골라 두었어요. 바꿔도 돼요.</p>}
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
          <ReviewCard head="신청할 곡" onEdit={() => setStep(0)}>{chosen.map(w => `${w.title} · ${w.contributors.map(c => `${c.name} ${pct(c.share)}`).join(', ')}`).join(' / ')}</ReviewCard>
          <ReviewCard head="관리 범위" onEdit={() => setStep(1)}>{territories.map(terrLabel).join(', ')} · {rights.map(rightLabel).join(', ')}</ReviewCard>
          <ReviewCard head="조건" onEdit={() => setStep(2)}>관리 수수료 {policy.commissionRate}% · 최초 계약 기간 {termLabel(policy.initialTermMonths)}</ReviewCard>
          <Callout tone="neutral">신청서에 서명해도 바로 계약되지 않아요. 담당자가 참여자·지분을 확인하고 승인하면, 확정된 조건으로 만든 저작권 관리 계약서를 계약서 화면에서 확인하고 서명해요.</Callout>
          {/* 관리 단체는 실제 사람으로 등록한다 — 필명으로 올라간 곡도 본인인증한 실명과 함께 접수한다 (서버 APPLICATION_IDENTITY_REQUIRED) */}
          {!account.verified ? (
            <Callout tone="danger" action={<Link to="/profile?open=verify" className="ui-btn is-white is-sm">본인인증</Link>}>
              저작권 관리 신청서에 서명하려면 본인인증이 필요해요. 관리 단체에는 필명이 아니라 인증한 실명으로 등록돼요.
            </Callout>
          ) : <p className="ui-field-help">관리 단체에는 본인인증한 실명으로 등록하고, 필명은 곡에 적은 이름 그대로 함께 올려요. 정산 계좌는 배급과 같아 저작권료도 같은 잔액에 쌓여요.</p>}
          <div className="ui-field">
            <label htmlFor="pubSignPad">서명 <span className="required">*</span></label>
            <SignaturePad ref={pad} id="pubSignPad" label="신청인 서명 입력" height={170} onChange={setDrawn} />
            <small className="ui-field-help">신청인 <b>{signerName || '이름 없음'}</b>{account.realName.trim() ? ' (본인인증 실명)' : ' — 본인인증을 하면 실명으로 기록돼요'}</small>
          </div>
          <div className="aq-agree-box">
            <label className="ui-check is-plain aq-agree-all-line">
              <input type="checkbox" checked={agreed.length === AGREE.length} onChange={e => setAgreed(e.target.checked ? AGREE.map(a => a[0]) : [])} />
              <span><strong>아래 내용에 모두 동의해요</strong></span>
            </label>
            {AGREE.map(([k, t]) => (
              <label key={k} className="ui-check is-plain">
                <input type="checkbox" checked={agreed.includes(k)} onChange={() => toggle(agreed, setAgreed, k)} />
                <span>{t}</span>
              </label>
            ))}
          </div>
          {failure && (
            <Callout tone="danger" action={<button type="button" className="ui-btn is-white is-sm" onClick={() => void submit()}>다시 시도</button>}>{failure}</Callout>
          )}
        </>
      )}

      {attachFor && <AttachmentModal works={[attachFor]} workId={attachFor.id} onClose={() => setAttachFor(null)} onAdded={() => reloadWorks()} />}
    </WizardShell>
  );
}
