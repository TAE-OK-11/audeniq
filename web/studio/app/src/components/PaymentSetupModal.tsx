// 수익 정산 정보 등록 마법사 — 라이브 openPaymentSetup/renderPaymentSetup 대응
import { useEffect, useMemo, useRef, useState } from 'react';
import { LegalModal } from './LegalDoc';
import { Modal } from './Modal';
import { BankLogo } from './BankLogo';
import { useToast } from './Toast';
import { setPayment, TYPE_LABEL, type PaymentInfo } from '../store/payment';
import { getProfileSnapshot } from '../store/profile';
import { stampNow } from '../lib/date';
import { localStamp } from '../lib/format';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { Glyph } from './Glyph';
import { requestPortalRefresh } from '../store/portalSync';
import { getAccountSnapshot } from '../store/account';

// 은행: 주요 시중은행 → iM·부산·경남 → 인터넷은행 → 씨티·수협·산업 → 상호금융·저축은행 → 전북·광주·제주은행
// 증권사: 주요 대형사(한투·미래·NH·삼성·KB·신한·하나)와 토스증권을 앞에, 나머지는 자기자본·이용 규모 순
const FINANCIAL_INSTITUTIONS: Record<string, { label: string; items: string[] }> = {
  bank: {
    label: '은행',
    items: ['IBK기업은행','KB국민은행','신한은행','하나은행','우리은행','NH농협은행','SC제일은행','iM뱅크','부산은행','경남은행','토스뱅크','케이뱅크','카카오뱅크','한국씨티은행','수협은행','KDB산업은행','새마을금고','신협','우체국','산림조합','저축은행','전북은행','광주은행','제주은행'],
  },
  securities: {
    label: '증권사',
    items: ['한국투자증권','미래에셋증권','NH투자증권','삼성증권','KB증권','신한투자증권','하나증권','토스증권','키움증권','메리츠증권','카카오페이증권','대신증권','한화투자증권','교보증권','우리투자증권','신영증권','유안타증권','DB증권','iM증권','유진투자증권','현대차증권','LS증권','IBK투자증권','BNK투자증권','다올투자증권','SK증권','부국증권','케이프투자증권','흥국증권','상상인증권'],
  },
};

const ACCOUNT_LENGTH_RULES: Record<string, number[]> = {
  '우리은행': [12, 13, 14], 'KB국민은행': [14], '하나은행': [14], '카카오뱅크': [13],
  '케이뱅크': [12], '토스뱅크': [12], '부산은행': [13], '경남은행': [13],
  '광주은행': [12, 13], '전북은행': [13], 'SC제일은행': [11], 'KDB산업은행': [14],
  '한국씨티은행': [10, 12, 13], 'SBI저축은행': [12, 14],
};

function accountLengthValid(bank: string, account: string): boolean {
  const rule = ACCOUNT_LENGTH_RULES[bank];
  return rule ? rule.includes(account.length) : account.length >= 10 && account.length <= 16;
}

const PAY_TYPES: { value: PaymentInfo['type']; label: string; desc: string }[] = [
  { value: 'personal', label: '개인', desc: '본인 명의 계좌로 받아요.' },
  { value: 'business', label: '개인사업자', desc: '사업자 명의 또는 대표자 계좌로 받아요.' },
  { value: 'corporate', label: '법인', desc: '법인 명의 계좌로 받아요.' },
];

// 항목마다 '정산금 지급 및 계좌 확인 약관'의 해당 조를 열어 보여 준다 (anchor: 조 제목에 든 말)
const AGREE_ITEMS: { label: string; anchor?: string }[] = [
  { label: '정산금 지급 및 계좌 확인 약관' },
  { label: '개인정보 수집·이용 동의', anchor: '개인정보의 보호' },
  { label: '금융정보 처리 동의', anchor: '금융계좌 확인' },
  { label: '수령인 정보 정확성 확인', anchor: '계좌 등록' },
];

interface Draft {
  recipient: string;
  type: PaymentInfo['type'];
  bank: string;
  last4: string;
  account: string;
}


export function PaymentSetupModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [legal, setLegal] = useState<{ anchor?: string } | null>(null);
  const [draft, setDraft] = useState<Draft>(() => {
    // 본인인증으로 확인한 실명이 있으면 그 이름을, 없으면 활동명을 기본값으로
    const profileName = getAccountSnapshot().realName || getProfileSnapshot().name || '';
    return { recipient: profileName, type: 'personal', bank: '', last4: '', account: '' };
  });
  const [category, setCategory] = useState('bank');
  const [query, setQuery] = useState('');
  const [agrees, setAgrees] = useState<boolean[]>([false, false, false, false]);
  const [payError, setPayError] = useState<{ title: string; detail: string } | null>(null);
  const errorCloseRef = useRef<HTMLButtonElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);
  const [tabPill, setTabPill] = useState({ left: 0, width: 0, height: 0 });

  // 선택된 탭으로 흰색 pill이 미끄러지듯 이동
  useEffect(() => {
    if (step !== 1) return;
    const update = () => {
      const container = tabsRef.current;
      if (!container) return;
      const active = container.querySelector<HTMLElement>('[aria-selected="true"]');
      if (!active) return;
      setTabPill({ left: active.offsetLeft, width: active.offsetWidth, height: active.offsetHeight });
    };
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  }, [step, category]);

  const flowRef = useRef<HTMLDivElement>(null);
  // 단계 이동 방향 — 다음은 오른쪽에서, 이전은 왼쪽에서 들어온다
  // (단계가 바뀔 때만 갱신 — 입력으로 다시 렌더돼도 애니메이션이 다시 시작되지 않게)
  const stepDir = useRef({ step, dir: 'fwd' });
  if (stepDir.current.step !== step) stepDir.current = { step, dir: step > stepDir.current.step ? 'fwd' : 'back' };
  const dir = stepDir.current.dir;
  const [registeredAt, setRegisteredAt] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    // 단계가 바뀌면 실제로 스크롤되는 영역(.aq-pay-main과 모달 바깥 컨테이너)을 모두 맨 위로
    // (이전 단계 스크롤이 남아 제목 위쪽이 상단 바에 가려지던 문제 방지)
    // iOS Safari는 계좌번호 입력 키보드가 내려가며 화면을 다시 맞추는 동안 스크롤을 밀어 두므로,
    // 화면이 자리 잡을 때까지(애니메이션·키보드 닫힘) 몇 번 더 맞춘다. 사용자가 스크롤을 시작하면 멈춘다.
    const flow = flowRef.current;
    const main = flow?.querySelector<HTMLElement>('.aq-pay-main');
    if (!flow || !main) return;
    let userScrolled = false;
    const reset = () => {
      if (userScrolled) return;
      main.scrollTop = 0;
      for (let el = flow.parentElement; el && el !== document.body; el = el.parentElement) {
        if (el.scrollTop) el.scrollTop = 0;
      }
    };
    const stop = () => { userScrolled = true; };
    main.addEventListener('touchstart', stop, { passive: true });
    main.addEventListener('wheel', stop, { passive: true });
    window.visualViewport?.addEventListener('resize', reset);
    reset();
    const frame = requestAnimationFrame(reset);
    const timers = [120, 480].map(ms => window.setTimeout(reset, ms));
    const done = window.setTimeout(() => window.visualViewport?.removeEventListener('resize', reset), 900);
    return () => {
      cancelAnimationFrame(frame);
      timers.forEach(window.clearTimeout);
      window.clearTimeout(done);
      main.removeEventListener('touchstart', stop);
      main.removeEventListener('wheel', stop);
      window.visualViewport?.removeEventListener('resize', reset);
    };
  }, [step]);

  useEffect(() => {
    if (payError) errorCloseRef.current?.focus();
  }, [payError]);

  const cat = FINANCIAL_INSTITUTIONS[category];
  const q = query.trim().toLowerCase();
  const visibleBanks = useMemo(
    () => cat.items.filter(name => !q || name.toLowerCase().includes(q)),
    [cat, q],
  );

  // 고른 카드가 잠깐 선택 상태로 보인 뒤 다음 단계로 넘어간다 (누른 것을 눈으로 확인하게)
  const pickTimer = useRef(0);
  useEffect(() => () => window.clearTimeout(pickTimer.current), []);
  const pickBank = (name: string) => {
    if (pickTimer.current) return;
    setDraft(d => ({ ...d, bank: name }));
    pickTimer.current = window.setTimeout(() => { pickTimer.current = 0; setStep(2); }, 160);
  };

  const back = () => {
    if (step === 0 || step === 4) { onClose(); return; }
    // 입력 중이던 칸의 포커스를 먼저 풀어 iOS 키보드가 이전 단계 화면을 밀어 올리지 않게 한다
    (document.activeElement as HTMLElement | null)?.blur?.();
    setStep(s => s - 1);
  };

  const nextFrom0 = () => {
    if (!draft.recipient.trim() || !draft.type) {
      toast('수령인 이름과 유형을 확인해 주세요.');
      return;
    }
    setDraft(d => ({ ...d, recipient: d.recipient.trim() }));
    setStep(1);
  };

  const nextFrom2 = () => {
    const account = draft.account.replace(/\D/g, '');
    const owner = draft.recipient.trim();
    if (!accountLengthValid(draft.bank, account)) {
      setPayError({
        title: '계좌번호를 다시 확인해 주세요.',
        detail: `${draft.bank} 계좌번호 길이와 맞지 않아요. 숫자만 입력했는지 확인해 주세요.`,
      });
      return;
    }
    if (!owner) {
      setPayError({
        title: '예금주를 입력해 주세요.',
        detail: '실제 계좌의 예금주명과 동일하게 입력해 주세요.',
      });
      return;
    }
    setDraft(d => ({ ...d, account, recipient: owner, last4: account.slice(-4) }));
    setStep(3);
  };

  const nextFrom3 = async () => {
    if (!agrees.every(Boolean)) {
      toast('필수 약관을 모두 확인해 주세요.');
      return;
    }
    if (saving) return;
    const at = stampNow();
    const info = {
      recipient: draft.recipient,
      type: draft.type,
      bank: draft.bank,
      accountNumber: draft.account,
      last4: draft.last4,
      registeredAt: at,
    };
    if (!MOCK) {
      // 실서버: 계좌번호 전체는 서버로만 보내고(암호화 저장) 화면에는 끝자리만 남긴다
      setSaving(true);
      try {
        await portal.savePayoutAccount(info, draft.account);
      } catch (err) {
        toast(errorMessage(err, '계좌를 등록하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error');
        return;
      } finally {
        setSaving(false);
      }
      requestPortalRefresh();
    }
    setRegisteredAt(localStamp(at));
    setPayment(info);
    setDraft(d => ({ ...d, account: '' }));
    setStep(4);
  };

  const done = () => {
    onClose();
    toast('수익 정산 정보가 등록됐어요.');
  };

  const allAgreed = agrees.every(Boolean);
  // 라이브: step2의 다음 버튼은 계좌번호가 입력될 때까지 비활성화 (예금주는 1단계 수령인 이름을 그대로 쓴다)
  const step2Ready = !!draft.account.replace(/\D/g, '') && !!draft.recipient.trim();
  const nextDisabled = saving || (step === 1 ? true : step === 2 ? !step2Ready : step === 3 ? !allAgreed : false);

  const actionLabel = step === 0 ? '다음으로'
    : step === 1 ? '금융기관을 선택해 주세요'
    : step === 2 ? '계좌 확인'
    : saving ? '등록하는 중'
    : '동의하고 등록';

  return (
    <Modal title="수익 정산 정보 등록" onClose={onClose} modalClass="aq-payout-setup-mode">
      <div className="aq-pay-flow" ref={flowRef}>
        <header className="aq-pay-top">
          <button type="button" aria-label="이전으로" onClick={back}><Glyph name="chevron-left" size={20} /></button>
          <div className="aq-pay-progress" aria-label={`수령 정보 등록 ${step + 1}단계`}>
            <span style={{ ['--pay-progress' as string]: `${Math.min(100, (step + 1) * 25)}%` }} />
          </div>
          <small>{Math.min(step + 1, 4)} / 4</small>
        </header>
        <main className={`aq-pay-main aq-step-anim is-${dir}`} key={step}>
          {step === 0 && (
            <>
              <header className="aq-pay-head">
                <p className="eyebrow">수령인 정보</p>
                <h1>수익을 받을<br />정보를 알려주세요.</h1>
                <p className="aq-pay-lead">계약과 정산 서류에 표시할 수령인 정보를 입력해 주세요.</p>
              </header>
              <div className="aq-pay-body">
                <div className="ui-field aq-pay-field">
                  <label htmlFor="aqPayRecipient" className="sr-only">수령인 이름 또는 상호</label>
                  <input
                    id="aqPayRecipient" maxLength={120} autoComplete="name"
                    value={draft.recipient}
                    onChange={e => setDraft(d => ({ ...d, recipient: e.target.value }))}
                    placeholder="수령인 이름 또는 상호"
                  />
                </div>
                <div className="aq-pay-type-list" role="radiogroup" aria-label="수령인 유형">
                  {PAY_TYPES.map(t => (
                    <label key={t.value} className="aq-pay-type">
                      <input
                        type="radio" name="aqPayType" value={t.value}
                        checked={draft.type === t.value}
                        onChange={() => setDraft(d => ({ ...d, type: t.value }))}
                      />
                      <span><strong>{t.label}</strong><small>{t.desc}</small></span>
                    </label>
                  ))}
                </div>
              </div>
            </>
          )}

          {step === 1 && (
            <>
              <header className="aq-pay-head">
                <p className="eyebrow">수령 계좌</p>
                <h1>수익을 받을<br />금융기관을 선택해 주세요.</h1>
                <p className="aq-pay-lead">은행, 저축은행, 증권사 계좌를 등록할 수 있어요.</p>
              </header>
              <div className="aq-pay-body">
                <div className="aq-bank-sticky">
                <div className="aq-bank-tabs" ref={tabsRef} role="tablist" aria-label="금융기관 종류">
                  <span
                    className="aq-bank-tab-pill" aria-hidden="true"
                    style={{
                      transform: `translateX(${tabPill.left}px)`,
                      width: tabPill.width || undefined,
                      height: tabPill.height || undefined,
                      opacity: tabPill.width ? 1 : 0,
                    }}
                  />
                  {Object.entries(FINANCIAL_INSTITUTIONS).map(([key, value]) => (
                    <button
                      key={key} type="button"
                      data-fin-category={key}
                      aria-selected={key === category}
                      onClick={() => { setCategory(key); setQuery(''); }}
                    >
                      {value.label}
                    </button>
                  ))}
                </div>
                <div className="aq-bank-search">
                  <input
                    id="aqBankSearch" type="search"
                    placeholder={`${cat.label} 검색`} aria-label="금융기관 검색"
                    value={query} onChange={e => setQuery(e.target.value)}
                  />
                </div>
                </div>
                <div id="aqBankGroups">
                  <section className="aq-bank-group" hidden={visibleBanks.length === 0}>
                    <h2>{cat.label}</h2>
                    {/* 탭을 바꾸면 목록을 새로 그려 카드가 차례로 떠오르게 한다 */}
                    <div className="aq-bank-grid" key={category}>
                      {cat.items.map((name, i) => (
                        <button
                          key={name} type="button"
                          className={`aq-bank${draft.bank === name ? ' selected' : ''}`}
                          style={{ ['--i' as string]: Math.min(i, 18) }}
                          data-bank={name}
                          hidden={!!q && !name.toLowerCase().includes(q)}
                          onClick={() => pickBank(name)}
                        >
                          <BankLogo name={name} />
                          <strong>{name}</strong>
                        </button>
                      ))}
                    </div>
                  </section>
                </div>
              </div>
            </>
          )}

          {step === 2 && (
            <>
              <header className="aq-pay-head">
                <p className="eyebrow">계좌번호</p>
                <h1>{draft.bank}<br />계좌번호를 입력해 주세요.</h1>
              </header>
              <div className="aq-pay-body">
                <div className="aq-selected-bank">
                  <BankLogo name={draft.bank} />
                  <div><strong>{draft.bank}</strong><small>예금주 {draft.recipient}</small></div>
                </div>
                <div className="ui-field aq-pay-field">
                  <label htmlFor="aqPayAccount" className="sr-only">계좌번호</label>
                  <input
                    id="aqPayAccount" inputMode="numeric" autoComplete="off" maxLength={20}
                    value={draft.account}
                    onChange={e => setDraft(d => ({ ...d, account: e.target.value.replace(/\D/g, '') }))}
                    placeholder="계좌번호 (숫자만 입력)"
                  />
                </div>
                <p className="aq-pay-privacy">
                  화면에는 계좌번호 뒤 4자리만 표시해요. 실제 서비스에서는 암호화된 서버 저장소에 안전하게 보관돼요.
                </p>
              </div>
            </>
          )}

          {step === 3 && (
            <>
              <header className="aq-pay-head">
                <p className="eyebrow">약관 동의</p>
                <h1>수령 계좌 등록에<br />동의해 주세요.</h1>
                <p className="aq-pay-lead">정산금 지급과 계좌 확인에 필요한 항목이에요.</p>
              </header>
              <div className="aq-pay-body">
                <label className="aq-agree-all">
                  <input
                    id="aqAgreeAll" type="checkbox" checked={allAgreed}
                    onChange={e => setAgrees([e.target.checked, e.target.checked, e.target.checked, e.target.checked])}
                  />
                  <span><strong>필수 약관 전체 동의</strong><small>아래 항목을 모두 확인했어요.</small></span>
                </label>
                <div className="aq-agree-list">
                  {AGREE_ITEMS.map(({ label, anchor }, i) => (
                    <label key={label}>
                      <input
                        type="checkbox" data-aq-agree checked={agrees[i]}
                        onChange={e => {
                          const arr = [...agrees];
                          arr[i] = e.target.checked;
                          setAgrees(arr);
                        }}
                      />
                      <span>{label}</span>
                      <button type="button" aria-label={`${label} 보기`} onClick={e => { e.preventDefault(); setLegal({ anchor }); }}><Glyph name="chevron-right" size={18} /></button>
                    </label>
                  ))}
                </div>
                <div className="aq-pay-review">
                  <BankLogo name={draft.bank} />
                  <div>
                    <strong>{draft.bank} · •••• {draft.last4}</strong>
                    <small>{draft.recipient} · {TYPE_LABEL[draft.type]}</small>
                  </div>
                </div>
              </div>
            </>
          )}

          {step === 4 && (
            <div className="aq-pay-success">
              <svg className="aq-success-mark" viewBox="0 0 88 88" aria-hidden="true">
                <circle className="aq-success-ring" cx="44" cy="44" r="40" />
                <path className="aq-success-tick" d="M27 45.5 39.5 58 62 33" />
              </svg>
              <h1>정산 계좌를 등록했어요</h1>
              <p className="aq-pay-success-sub">정산이 확정되면 이 계좌로 수익을 보내 드려요.</p>
              <div className="aq-pay-receipt">
                <div className="aq-pay-receipt-head">
                  <BankLogo name={draft.bank} />
                  <div className="min-0">
                    <small>{draft.bank}</small>
                    <strong>•••• {draft.last4}</strong>
                  </div>
                </div>
                <dl>
                  <div><dt>예금주</dt><dd>{draft.recipient}</dd></div>
                  <div><dt>수령 유형</dt><dd>{TYPE_LABEL[draft.type]}</dd></div>
                  <div><dt>등록 일시</dt><dd>{registeredAt}</dd></div>
                </dl>
              </div>
              <p className="aq-pay-success-note">계좌는 정산·지급이나 계정 정보에서 언제든 바꿀 수 있어요.</p>
            </div>
          )}
        </main>
        {step === 4 ? (
          // 완료 화면도 확인 버튼을 하단에 고정 — 작은 화면에서 스크롤하지 않아도 바로 보이게
          <footer className="aq-pay-actions is-final">
            <button type="button" className="ui-btn is-primary" id="aqPayDone" onClick={done}>확인</button>
          </footer>
        ) : (
          <footer className="aq-pay-actions">
            <button
              type="button" className="ui-btn is-primary" id="aqPayNext"
              disabled={nextDisabled}
              onClick={step === 0 ? nextFrom0 : step === 1 ? undefined : step === 2 ? nextFrom2 : nextFrom3}
            >
              {actionLabel}
            </button>
          </footer>
        )}
        {payError && (
          <div className="aq-pay-error-overlay" role="alertdialog" aria-modal="true" aria-labelledby="aqPayErrorTitle">
            <div className="aq-pay-error-dialog">
              <strong id="aqPayErrorTitle">{payError.title}</strong>
              <p>{payError.detail}</p>
              <button type="button" id="aqPayErrorClose" ref={errorCloseRef} onClick={() => setPayError(null)}>
                확인
              </button>
            </div>
          </div>
        )}
      </div>
      {legal && <LegalModal doc="settlement" anchor={legal.anchor} onClose={() => setLegal(null)} />}
    </Modal>
  );
}

