// 수익 정산 정보 등록 마법사 — 라이브 openPaymentSetup/renderPaymentSetup 대응
import { useEffect, useMemo, useRef, useState } from 'react';
import { Modal } from './Modal';
import { BankLogo } from './BankLogo';
import { useToast } from './Toast';
import { setPayment, TYPE_LABEL, type PaymentInfo } from '../store/payment';
import { getProfileSnapshot } from '../store/profile';

const FINANCIAL_INSTITUTIONS: Record<string, { label: string; note: string; items: string[] }> = {
  bank: {
    label: '은행', note: '은행·인터넷은행·특수은행·상호금융',
    items: ['IBK기업은행','신한은행','KB국민은행','하나은행','우리은행','NH농협은행','SC제일은행','KDB산업은행','카카오뱅크','부산은행','iM뱅크','경남은행','토스뱅크','케이뱅크','수협은행','한국씨티은행','새마을금고','신협','우체국','산림조합','전북은행','광주은행','제주은행'],
  },
  savings: {
    label: '저축은행', note: '주요 저축은행을 자산 규모와 이용 빈도 순으로 정리했어요.',
    items: ['SBI저축은행','OK저축은행','한국투자저축은행','웰컴저축은행','애큐온저축은행','페퍼저축은행','신한저축은행','KB저축은행','하나저축은행','우리금융저축은행','NH저축은행','IBK저축은행','BNK저축은행','다올저축은행','키움YES저축은행','JT친애저축은행','JT저축은행','상상인저축은행','상상인플러스저축은행','대신저축은행','DB저축은행','OSB저축은행','모아저축은행','푸른저축은행','스마트저축은행','예가람저축은행','바로저축은행','유안타저축은행','동원제일저축은행','청주저축은행'],
  },
  securities: {
    label: '증권사', note: '주요 증권사를 자기자본·고객 이용 규모를 고려해 정리했어요.',
    items: ['미래에셋증권','한국투자증권','NH투자증권','삼성증권','KB증권','신한투자증권','키움증권','하나증권','메리츠증권','대신증권','토스증권','카카오페이증권','우리투자증권','한화투자증권','유안타증권','현대차증권','교보증권','DB증권','iM증권','LS증권','SK증권','신영증권','유진투자증권','IBK투자증권','BNK투자증권','다올투자증권','부국증권','흥국증권','케이프투자증권','상상인증권'],
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

const AGREE_ITEMS = [
  '정산금 지급 및 계좌 확인 약관',
  '개인정보 수집·이용 동의',
  '금융정보 처리 동의',
  '수령인 정보 정확성 확인',
];

interface Draft {
  recipient: string;
  type: PaymentInfo['type'];
  bank: string;
  last4: string;
  account: string;
}

function stampNow(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function PaymentSetupModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Draft>(() => {
    const profileName = getProfileSnapshot().name || '';
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

  useEffect(() => {
    // 실제 스크롤 컨테이너(.modal-inner)를 위로 — window.scrollTo는 모달 안에서 무효
    const scroller = flowRef.current?.closest('.modal-inner') as HTMLElement | null;
    scroller?.scrollTo({ top: 0 });
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

  const back = () => {
    if (step === 0) { onClose(); return; }
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

  const nextFrom3 = () => {
    if (!agrees.every(Boolean)) {
      toast('필수 약관을 모두 확인해 주세요.');
      return;
    }
    setPayment({
      recipient: draft.recipient,
      type: draft.type,
      bank: draft.bank,
      accountNumber: draft.account,
      last4: draft.last4,
      registeredAt: stampNow(),
    });
    setDraft(d => ({ ...d, account: '' }));
    setStep(4);
  };

  const done = () => {
    onClose();
    toast('수익 정산 정보가 등록됐어요.');
  };

  const allAgreed = agrees.every(Boolean);
  // 라이브: step2의 다음 버튼은 계좌번호·예금주가 모두 입력될 때까지 비활성화
  const step2Ready = !!draft.account.replace(/\D/g, '') && !!draft.recipient.trim();
  const nextDisabled = step === 1 ? true : step === 2 ? !step2Ready : step === 3 ? !allAgreed : false;

  const actionLabel = step === 0 ? '다음으로'
    : step === 1 ? '금융기관을 선택해 주세요'
    : step === 2 ? '계좌 확인'
    : '동의하고 등록';

  return (
    <Modal title="수익 정산 정보 등록" onClose={onClose} modalClass="aq-payout-setup-mode">
      <div className="aq-pay-flow" ref={flowRef}>
        <header className="aq-pay-top">
          <button type="button" aria-label="이전으로" onClick={back}>‹</button>
          <div className="aq-pay-progress" aria-label={`수령 정보 등록 ${step + 1}단계`}>
            <span style={{ ['--pay-progress' as string]: `${Math.min(100, (step + 1) * 25)}%` }} />
          </div>
          <small>{Math.min(step + 1, 4)} / 4</small>
        </header>
        <main className="aq-pay-main">
          {step === 0 && (
            <>
              <p className="eyebrow">PAYOUT PROFILE</p>
              <h1>수익을 받을<br />정보를 알려주세요.</h1>
              <p className="aq-pay-lead">계약과 정산 서류에 표시할 수령인 정보를 입력해 주세요.</p>
              <div className="field aq-pay-field">
                <label htmlFor="aqPayRecipient">수령인 이름 또는 상호</label>
                <input
                  id="aqPayRecipient" maxLength={120} autoComplete="name"
                  value={draft.recipient}
                  onChange={e => setDraft(d => ({ ...d, recipient: e.target.value }))}
                  placeholder="예금주 또는 법인명"
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
            </>
          )}

          {step === 1 && (
            <>
              <p className="eyebrow">PAYOUT ACCOUNT</p>
              <h1>수익을 받을<br />금융기관을 선택해 주세요.</h1>
              <p className="aq-pay-lead">은행, 저축은행, 증권사 계좌를 등록할 수 있어요.</p>
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
              <p className="aq-bank-category-note">{cat.note}</p>
              <div className="aq-bank-search">
                <input
                  id="aqBankSearch" type="search"
                  placeholder={`${cat.label} 검색`} aria-label="금융기관 검색"
                  value={query} onChange={e => setQuery(e.target.value)}
                />
              </div>
              <div id="aqBankGroups">
                <section className="aq-bank-group" hidden={visibleBanks.length === 0}>
                  <h2>{cat.label}</h2>
                  <div className="aq-bank-grid">
                    {cat.items.map(name => (
                      <button
                        key={name} type="button"
                        className={`aq-bank${draft.bank === name ? ' selected' : ''}`}
                        data-bank={name}
                        hidden={!!q && !name.toLowerCase().includes(q)}
                        onClick={() => { setDraft(d => ({ ...d, bank: name })); setStep(2); }}
                      >
                        <BankLogo name={name} />
                        <strong>{name}</strong>
                      </button>
                    ))}
                  </div>
                </section>
              </div>
            </>
          )}

          {step === 2 && (
            <>
              <p className="eyebrow">ACCOUNT NUMBER</p>
              <h1>{draft.bank}<br />계좌번호를 입력해 주세요.</h1>
              <div className="aq-selected-bank">
                <BankLogo name={draft.bank} />
                <div><strong>{draft.bank}</strong><small>수익 정산 계좌</small></div>
              </div>
              <div className="field aq-pay-field">
                <label htmlFor="aqPayAccount">계좌번호</label>
                <input
                  id="aqPayAccount" inputMode="numeric" autoComplete="off" maxLength={20}
                  value={draft.account}
                  onChange={e => setDraft(d => ({ ...d, account: e.target.value.replace(/\D/g, '') }))}
                  placeholder="숫자만 입력"
                />
              </div>
              <div className="field aq-pay-field">
                <label htmlFor="aqPayOwner">예금주</label>
                <input
                  id="aqPayOwner" maxLength={120}
                  value={draft.recipient}
                  onChange={e => setDraft(d => ({ ...d, recipient: e.target.value }))}
                  placeholder="예금주명"
                />
              </div>
              <p className="aq-pay-privacy">
                계좌번호 전체값을 등록하고 화면에는 마스킹해 표시해요. 실제 서비스에서는 암호화된 서버 저장소에 안전하게 보관돼요.
              </p>
            </>
          )}

          {step === 3 && (
            <>
              <p className="eyebrow">AGREEMENT</p>
              <h1>수령 계좌 등록에<br />동의해 주세요.</h1>
              <p className="aq-pay-lead">정산금 지급과 계좌 확인에 필요한 항목이에요.</p>
              <label className="aq-agree-all">
                <input
                  id="aqAgreeAll" type="checkbox" checked={allAgreed}
                  onChange={e => setAgrees([e.target.checked, e.target.checked, e.target.checked, e.target.checked])}
                />
                <span><strong>필수 약관 전체 동의</strong><small>아래 항목을 모두 확인했어요.</small></span>
              </label>
              <div className="aq-agree-list">
                {AGREE_ITEMS.map((label, i) => (
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
                    <button type="button" aria-label="약관 보기">›</button>
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
            </>
          )}

          {step === 4 && (
            <div className="aq-pay-success">
              <div className="aq-success-check">✓</div>
              <p className="eyebrow">PAYOUT READY</p>
              <h1>수익을 받을 정보가<br />등록됐어요.</h1>
              <div className="aq-pay-review">
                <BankLogo name={draft.bank} />
                <div>
                  <strong>{draft.bank} · •••• {draft.last4}</strong>
                  <small>{draft.recipient}</small>
                </div>
              </div>
              <button type="button" className="button" id="aqPayDone" onClick={done}>확인</button>
            </div>
          )}
        </main>
        {step < 4 && (
          <footer className="aq-pay-actions">
            <button
              type="button" className="button" id="aqPayNext"
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
    </Modal>
  );
}
